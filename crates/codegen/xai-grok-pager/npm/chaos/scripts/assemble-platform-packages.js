#!/usr/bin/env node
// Assemble the six per-platform npm packages prior to `npm publish`.
//
// For each supported (platform, arch) target this:
//   1. Brotli-compresses the built binary into `../chaos-<platform>/bin/<bin>.br`
//   2. Stamps the sub-package's version to match the meta package
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

// npm/chaos/scripts -> repo root is five levels up
const repoRoot = process.env.CHAOS_ROOT
    || process.env.XAI_ROOT
    || path.resolve(__dirname, '..', '..', '..', '..', '..');
const npmRoot = path.resolve(__dirname, '..', '..');

const NOTICES_SOURCE = path.resolve(
    npmRoot, '..', '..', 'xai-grok-tools', 'THIRD_PARTY_NOTICES.md');
const NOTICES_NAME = 'THIRD_PARTY_NOTICES.md';

const META_PKG_JSON = path.resolve(__dirname, '..', 'package.json');
const meta = JSON.parse(fs.readFileSync(META_PKG_JSON, 'utf8'));
const VERSION = meta.version;
const META_NAME = meta.name; // chaos-code

function ensureDir(p) { fs.mkdirSync(path.dirname(p), { recursive: true }); }

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

    if (!fs.existsSync(NOTICES_SOURCE)) {
        console.error(`[assemble] Missing third-party notices file: ${NOTICES_SOURCE}`);
        return false;
    }
    fs.copyFileSync(NOTICES_SOURCE, path.join(pkgDir, NOTICES_NAME));

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

main().catch((err) => { console.error(err); process.exit(1); });
