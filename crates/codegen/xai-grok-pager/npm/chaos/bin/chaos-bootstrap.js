#!/usr/bin/env node
// Resolves the chaos binary and runs it, in order of preference:
//   1. the versioned symlink postinstall.js installs under the chaos home
//   2. bootstrap it from the per-platform chaos-code-<platform>
//      package, decompressing the compressed binary into the chaos home bin
//   3. decompress in place under node_modules (no resolvable version, or
//      an unwritable home)
//
// Binaries ship brotli-compressed to stay under npm's tarball size limit.
//
// Step 1 is also what lets `chaos update` and `scripts/install.sh` move the binary out from under
// npm: whoever wrote last is what runs, even when that is a different version than this package
// shipped. That is deliberate -- a self-update has to take effect without waiting for an npm
// publish -- and it is why this file cannot vouch for the bytes on that path. The digests in the
// platform package describe this package's own binary, not whatever replaced it. Two things are
// still checked here: when the link names exactly this version, the file's size has to be the size
// the package recorded, which catches a truncated or swapped-in file for the price of one `stat`
// and re-bootstraps it from the package; and anything this file writes in steps 2 and 3 goes
// through `install-lib.js`, which refuses bytes that do not hash to `bin/integrity.json`.
'use strict';

const { spawn } = require('child_process');
const path = require('path');
const fs = require('fs');
const lib = require('./install-lib.js');

const pkgName = 'chaos-code';
const IS_WINDOWS = process.platform === 'win32';
const EXE = IS_WINDOWS ? '.exe' : '';
const BIN_NAME = `chaos${EXE}`;
const GROK_HOME = lib.resolveChaosHome(process.env);
const CANONICAL_DIR = path.join(GROK_HOME, 'bin');
const CANONICAL_PATH = path.join(CANONICAL_DIR, BIN_NAME);

function readLocalVersion() {
    try { return require('../package.json').version; } catch { return undefined; }
}

// The version this package pinned for a platform sibling, for the error message only.
function readOptionalDependencyVersion(platformPkg) {
    try {
        const pkg = require('../package.json');
        return (pkg.optionalDependencies || {})[platformPkg];
    } catch { return undefined; }
}

// Returns null when npm skipped the matching optional dependency
// (unsupported platform, or --no-optional).
function resolvePlatformPackageDir() {
    const platformPkg = `${pkgName}-${process.platform}-${process.arch}`;
    try {
        return path.dirname(require.resolve(`${platformPkg}/package.json`));
    } catch {
        return null;
    }
}

/** The version the canonical link names, or null for a plain file or a foreign name. */
function canonicalVersion() {
    let target;
    try { target = fs.readlinkSync(CANONICAL_PATH); } catch { return null; }
    return lib.versionOfVersionedName(path.basename(target), 'chaos', IS_WINDOWS);
}

/**
 * The one byte-level check worth making before every launch.
 *
 * A hash of a 150 MB binary would be paid by every `chaos` invocation, for a check the install
 * already made and that an attacker with write access to the chaos home could undo by editing the
 * digests too. Size is free, and it is the thing that breaks when a download or an update is
 * interrupted.
 */
function recordedSizeMismatch(file, integrity, sourceName) {
    let size;
    try { size = fs.statSync(file).size; } catch { return `${file} cannot be stat'd`; }
    if (size === integrity.record.binary.bytes) return null;
    return `${file} is ${size} bytes, not the ${integrity.record.binary.bytes} ${sourceName} records`;
}

/** A digest refusal stops the launch: running the bytes anyway is the outcome being prevented. */
function refuseUnverifiedBytes(sourceDir, result) {
    if (result.reason === 'digest') {
        console.error(`${pkgName}: refusing to run ${path.basename(sourceDir)}: ${result.label} does not match bin/${lib.INTEGRITY_FILE}`);
        console.error(`  file:     ${result.path}`);
        console.error(`  expected: ${result.expected}`);
        console.error(`  actual:   ${result.actual}`);
        console.error(`  Re-install with \`npm install -g ${pkgName}\` after \`npm cache clean --force\`,`);
        console.error('  and report the package if the mismatch repeats.');
        return;
    }
    console.error(`${pkgName}: refusing to run ${path.basename(sourceDir)} (${result.reason}): ${result.detail || result.path}`);
}

const INTEGRITY_REFUSALS = new Set(['digest', 'integrity', 'size', 'decompress']);

function resolveBinary() {
    const version = readLocalVersion();
    const platformDir = resolvePlatformPackageDir();

    if (fs.existsSync(CANONICAL_PATH)) {
        if (!version || !platformDir) return CANONICAL_PATH;
        const integrity = lib.readIntegrity(platformDir, BIN_NAME);
        if (!integrity.ok) return CANONICAL_PATH;
        if (canonicalVersion() !== version) return CANONICAL_PATH;
        const mismatch = recordedSizeMismatch(CANONICAL_PATH, integrity, path.basename(platformDir));
        if (!mismatch) return CANONICAL_PATH;
        console.error(`${pkgName}: ${mismatch}; re-installing from the package`);
    }

    if (!platformDir) {
        // npm skips an optional dependency it cannot resolve instead of failing the
        // install, so `npm install` can report success with no binary present. Naming the
        // pinned version is what lets a user tell "skipped optional deps" apart from
        // "that version was never published under this name" without guessing.
        const platformPkg = `${pkgName}-${process.platform}-${process.arch}`;
        let pinned = readOptionalDependencyVersion(platformPkg);
        console.error(`${pkgName}: no platform binary installed for ${process.platform}-${process.arch}.`);
        console.error(`  Expected sibling package ${platformPkg}${pinned ? `@${pinned}` : ''}.`);
        console.error(`  npm installs optional dependencies best-effort, so this package can install`);
        console.error(`  cleanly with no binary. Causes, most common first:`);
        console.error(`    - npm was run with --no-optional or --omit=optional`);
        console.error(`    - ${pinned ? `${platformPkg}@${pinned}` : platformPkg} is not published under that name`);
        console.error(`      (npm squats some names with a security placeholder); check with:`);
        console.error(`        npm view ${platformPkg} versions`);
        console.error(`    - ${process.platform}-${process.arch} is not a published platform`);
        process.exit(1);
    }

    if (version) {
        const installed = lib.installVersionedBinary({
            sourceDir: platformDir,
            binName: 'chaos',
            version,
            canonicalDir: CANONICAL_DIR,
            isWindows: IS_WINDOWS,
        });
        if (installed.ok) return installed.canonicalPath;
        if (INTEGRITY_REFUSALS.has(installed.reason)) {
            refuseUnverifiedBytes(platformDir, installed);
            process.exit(1);
        }
        // An unwritable home or a locked binary is not a reason to refuse to run: fall through to
        // the copy beside the package, which needs neither.
    }

    const binaryPath = path.join(platformDir, 'bin', BIN_NAME);
    if (!fs.existsSync(binaryPath)) {
        const written = lib.writeVerifiedBinary(platformDir, BIN_NAME, binaryPath, { isWindows: IS_WINDOWS });
        if (!written.ok) {
            refuseUnverifiedBytes(platformDir, written);
            process.exit(1);
        }
    } else {
        // Something is beside the package. If the package says what those bytes should be, hold
        // them to it; without a record there is nothing here to check them against.
        const integrity = lib.readIntegrity(platformDir, BIN_NAME);
        if (integrity.ok) {
            const mismatch = recordedSizeMismatch(binaryPath, integrity, path.basename(platformDir));
            if (mismatch) {
                refuseUnverifiedBytes(platformDir, { reason: 'size', path: binaryPath, detail: mismatch });
                process.exit(1);
            }
        }
    }
    return binaryPath;
}

const execPath = resolveBinary();
const childEnv = { ...process.env, GROK_MANAGED_BY_NPM: '1' };
const child = spawn(execPath, process.argv.slice(2), { stdio: 'inherit', env: childEnv });
child.on('exit', (code, signal) => {
    if (signal) {
        process.kill(process.pid, signal);
    } else {
        process.exit(code ?? 0);
    }
});
