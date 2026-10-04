#!/usr/bin/env node
// Assemble the six per-platform npm packages prior to `npm publish`.
//
// For each supported (platform, arch) target this:
//   1. Brotli-compresses the built binary into `../chaos-<platform>/bin/<bin>.br`
//   2. Stamps the sub-package's version to match the meta package
//   3. Writes the assembled third-party notices into every package directory
//
// Each per-platform package is its own npm publish target. The meta package
// (`chaos-code`) lists all six as `optionalDependencies` pinned to
// the same version; npm installs only the one matching the host's
// `os` + `cpu` filters.
//
// Why brotli? npm's tarball ceiling is ~200 MB and the raw chaos binary is
// often 70–150 MB per platform. Brotli at max quality cuts that substantially,
// leaves headroom for binary growth, and is decoded by Node's
// built-in zlib.brotliDecompressSync (no native deps required).
//
// Source paths come from environment variables (set in CI) and fall back to
// the default cargo target dirs for local testing.
const fs = require('fs');
const path = require('path');
const { promisify } = require('util');
const zlib = require('zlib');

const brotliCompress = promisify(zlib.brotliCompress);

// npm/chaos/scripts -> repo root is six levels up. CI sets CHAOS_ROOT explicitly; the
// fallback has to stand on its own, because the notices document below is read from here.
const repoRoot = process.env.CHAOS_ROOT
    || process.env.XAI_ROOT
    || path.resolve(__dirname, '..', '..', '..', '..', '..', '..');
const npmRoot = path.resolve(__dirname, '..', '..');

const NOTICES_NAME = 'THIRD_PARTY_NOTICES.md';
// Two different notices travel with the binary, and the tarball has to carry both.
//
// The dependency notices live at the repository root and are what `cargo`-level
// attribution is owed for: one entry per third-party package linked into the binary,
// plus every license text those entries point at. They are maintained by
// `scripts/gen-third-party-notices.py` and audited by `scripts/ci/check-notices-*.py`.
//
// The ported-code notices belong to `xai-grok-tools`, whose tool implementations were
// translated out of other projects and modified. Clause 4(b) of Apache License 2.0
// requires a prominent notice of those modifications, so that file is not optional
// either, and it is not a substitute for the dependency notices: it describes where
// this crate's ported code came from, not what the binary links against.
const DEPENDENCY_NOTICES = path.resolve(repoRoot, 'THIRD-PARTY-NOTICES');
const PORTED_NOTICES = path.resolve(
    npmRoot, '..', '..', 'xai-grok-tools', 'THIRD_PARTY_NOTICES.md');

const META_PKG_JSON = path.resolve(__dirname, '..', 'package.json');
const meta = JSON.parse(fs.readFileSync(META_PKG_JSON, 'utf8'));
const VERSION = meta.version;
const META_NAME = meta.name; // chaos-code

function ensureDir(p) { fs.mkdirSync(path.dirname(p), { recursive: true }); }

/**
 * The notices document that travels inside every tarball.
 *
 * Both inputs are reproduced verbatim, because a notice that was paraphrased on the way to
 * the user is not the notice the author asked for. What this function adds is the header that
 * tells a reader which of the two they are looking at and where each came from.
 *
 * Refusing here is deliberate: an assembled package with no notices still installs, still
 * runs, and still ships code that its licenses say must travel with its attribution. That is
 * the one failure mode of a release nobody notices, which is why the inputs are checked
 * before a single byte of tarball exists.
 */
function buildNoticesBundle(dependencyText, portedText) {
    const nonEmpty = (label, text) => {
        if (!text || !text.trim()) {
            throw new Error(`[assemble] third-party notices input ${label} is empty`);
        }
        return text.trimEnd();
    };
    const dependencies = nonEmpty(DEPENDENCY_NOTICES, dependencyText);
    const ported = nonEmpty(PORTED_NOTICES, portedText);
    if (!/^PART I . PER-PACKAGE ENTRIES$/m.test(dependencies)) {
        throw new Error(
            `[assemble] ${DEPENDENCY_NOTICES} has no "PART I — PER-PACKAGE ENTRIES" heading, ` +
            'so it is not the dependency notices document this bundle is built from');
    }
    const header = [
        `# Third-party notices for ${META_NAME} ${VERSION}`,
        '',
        'The `chaos` binary you installed is a compiled work that includes code from the',
        'third-party packages recorded below. Part I gives the license and the copyright',
        'notice recorded for each of those packages; Part II reproduces the license texts',
        'themselves. The section at the end covers source code that was ported into this',
        'product from other projects and then modified.',
        '',
        'This file is assembled when the package is built, from two files in the source',
        'repository, and it is not edited inside the tarball:',
        '',
        '- `THIRD-PARTY-NOTICES`: the dependency notices, reproduced in full below.',
        '- `crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md`: the notices for ported',
        '  source code and for the bundled tool binaries, reproduced in full at the end.',
        '',
        'Source repository: https://github.com/chao2hang/chaos-code',
        '',
        '---',
        '',
    ].join('\n');
    const footer = [
        '',
        '---',
        '',
        '## Ported source code and bundled tool binaries',
        '',
        'Reproduced verbatim from',
        '`crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md` in the source repository.',
        '',
        ported,
        '',
    ].join('\n');
    return header + dependencies + footer;
}

/**
 * Write the bundle wherever it has to exist.
 *
 * Three places, for three different readers: the meta package directory and the six platform
 * package directories, because each is its own npm publish target and `package.json` `files`
 * only pulls the document in when it sits next to that manifest; and one copy at `npm/`,
 * which is the stable path the release workflow uploads as an artifact and attaches to the
 * GitHub Release for people who install the binary without npm.
 *
 * All six platform directories are written even when only some have a binary, so a partial
 * assembly cannot leave a directory that is publishable but silent about its dependencies.
 */
function writeNoticesBundles(targets) {
    const bundle = buildNoticesBundle(
        fs.readFileSync(DEPENDENCY_NOTICES, 'utf8'),
        fs.readFileSync(PORTED_NOTICES, 'utf8'),
    );
    const destinations = [
        path.join(npmRoot, NOTICES_NAME),
        path.join(npmRoot, 'chaos', NOTICES_NAME),
    ];
    for (const target of targets) {
        destinations.push(
            path.join(npmRoot, `chaos-${target.platform}-${target.arch}`, NOTICES_NAME),
        );
    }
    for (const destination of destinations) {
        ensureDir(destination);
        fs.writeFileSync(destination, bundle);
    }
    console.log(
        `[assemble] ${NOTICES_NAME}: ${(bundle.length / 1024).toFixed(0)} KB written to ` +
        `${destinations.length} file(s) from ${path.relative(repoRoot, DEPENDENCY_NOTICES)} ` +
        `+ ${path.relative(repoRoot, PORTED_NOTICES)}`,
    );
    return bundle;
}

async function packPlatform({ platform, arch, envVar, defaultSource, binName }) {
    const pkgDir = path.join(npmRoot, `chaos-${platform}-${arch}`);
    const pkgJsonPath = path.join(pkgDir, 'package.json');

    if (!fs.existsSync(pkgJsonPath)) {
        console.error(`[assemble] Missing per-platform package at ${pkgDir}`);
        return false;
    }

    const source = process.env[envVar] || defaultSource;
    if (!fs.existsSync(source)) {
        console.error(`[assemble] Missing binary for ${platform}-${arch}: ${source}`);
        console.error(`            Set ${envVar} or build to the default location.`);
        return false;
    }

    // Stamp the sub-package's version to match the meta package.
    const subPkg = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'));
    subPkg.version = VERSION;
    fs.writeFileSync(pkgJsonPath, JSON.stringify(subPkg, null, 4) + '\n');

    // Brotli-compress into the sub-package's bin/.
    const outBr = path.join(pkgDir, 'bin', `${binName}.br`);
    ensureDir(outBr);
    const raw = fs.readFileSync(source);
    const compressed = await brotliCompress(raw, {
        params: { [zlib.constants.BROTLI_PARAM_QUALITY]: zlib.constants.BROTLI_MAX_QUALITY },
    });
    fs.writeFileSync(outBr, compressed);
    console.log(
        `[assemble] ${META_NAME}-${platform}-${arch}@${VERSION}: ` +
        `${(raw.length / 1048576).toFixed(1)} MB -> ${(compressed.length / 1048576).toFixed(1)} MB ` +
        `(${path.relative(npmRoot, outBr)})`
    );
    return true;
}

async function main() {
    const targets = [
        {
            platform: 'darwin', arch: 'arm64', binName: 'chaos',
            envVar: 'CHAOS_DARWIN_ARM64',
            defaultSource: path.join(repoRoot, 'target', 'release', 'chaos'),
        },
        {
            platform: 'darwin', arch: 'x64', binName: 'chaos',
            envVar: 'CHAOS_DARWIN_X64',
            defaultSource: path.join(repoRoot, 'target', 'x86_64-apple-darwin', 'release', 'chaos'),
        },
        {
            platform: 'linux', arch: 'x64', binName: 'chaos',
            envVar: 'CHAOS_LINUX_X64',
            defaultSource: path.join(repoRoot, 'target', 'release', 'chaos'),
        },
        {
            platform: 'linux', arch: 'arm64', binName: 'chaos',
            envVar: 'CHAOS_LINUX_ARM64',
            defaultSource: path.join(repoRoot, 'target',
                'aarch64-unknown-linux-gnu', 'release', 'chaos'),
        },
        {
            platform: 'win32', arch: 'x64', binName: 'chaos.exe',
            envVar: 'CHAOS_WIN32_X64',
            defaultSource: path.join(repoRoot, 'target', 'x86_64-pc-windows-msvc', 'release', 'chaos.exe'),
        },
        {
            platform: 'win32', arch: 'arm64', binName: 'chaos.exe',
            envVar: 'CHAOS_WIN32_ARM64',
            defaultSource: path.join(repoRoot, 'target', 'aarch64-pc-windows-msvc', 'release', 'chaos.exe'),
        },
    ];

    // Compress in parallel — brotliCompress runs on the libuv thread pool so
    // calls genuinely overlap (set UV_THREADPOOL_SIZE>=6 in CI for full
    // parallelism; Node's default pool size is 4).
    //
    // Selection filters (first match wins):
    //   ONLY_PLATFORMS="linux-x64 darwin-arm64"  — CI partial matrix
    //   ONLY_HOST=1                              — current process.platform-arch
    //   (default)                                — all six targets
    //
    // When a filter is set, missing binaries for *unselected* targets are
    // ignored; selected targets must still succeed.
    const onlyHost = process.env.ONLY_HOST === '1' || process.env.ONLY_HOST === 'true';
    const hostKey = `${process.platform}-${process.arch}`;
    const onlyPlatforms = (process.env.ONLY_PLATFORMS || '')
        .split(/[\s,]+/)
        .map((s) => s.trim())
        .filter(Boolean);

    let selected;
    if (onlyPlatforms.length > 0) {
        const want = new Set(onlyPlatforms);
        selected = targets.filter((t) => want.has(`${t.platform}-${t.arch}`));
        const unknown = onlyPlatforms.filter(
            (p) => !targets.some((t) => `${t.platform}-${t.arch}` === p),
        );
        if (unknown.length) {
            console.error(`[assemble] unknown ONLY_PLATFORMS: ${unknown.join(', ')}`);
            process.exit(1);
        }
    } else if (onlyHost) {
        selected = targets.filter((t) => `${t.platform}-${t.arch}` === hostKey);
    } else {
        selected = targets;
    }

    if (selected.length === 0) {
        console.error(
            `[assemble] no targets selected` +
            (onlyHost ? ` (host ${hostKey})` : '') +
            (onlyPlatforms.length ? ` (ONLY_PLATFORMS=${onlyPlatforms.join(',')})` : ''),
        );
        process.exit(1);
    }

    // Written for every target, not only the selected ones: `publish-npm.sh` publishes
    // whatever platform directory holds a binary, so a directory assembled in an earlier
    // run must not be publishable while silent about its dependencies.
    writeNoticesBundles(targets);

    const results = await Promise.all(selected.map(packPlatform));
    const failed = results.filter((r) => !r).length;
    if (failed > 0) {
        console.error(`[assemble] ${failed} target(s) failed.`);
        process.exit(1);
    }

    const mode = onlyPlatforms.length
        ? `ONLY_PLATFORMS=${onlyPlatforms.join(',')}`
        : onlyHost
            ? `host only: ${hostKey}`
            : 'all targets';
    console.log(
        `[assemble] ${selected.length} per-platform package(s) assembled at version ${VERSION} (${mode}).`,
    );
}

// Exported so `scripts/ci/test-assemble-notices.sh` can drive the bundle builder directly;
// requiring this module must never assemble anything or touch the working tree.
module.exports = { buildNoticesBundle, writeNoticesBundles, NOTICES_NAME };

if (require.main === module) {
    main().catch((err) => { console.error(err); process.exit(1); });
}
