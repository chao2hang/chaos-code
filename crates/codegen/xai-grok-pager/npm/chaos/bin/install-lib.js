#!/usr/bin/env node
// The installation logic shared by `bin/postinstall.js` and `bin/chaos-bootstrap.js`.
//
// Both entry points make the same three promises about the same bytes: the file that lands under
// the chaos home is the file the release hashed, it is executable, and replacing it while another
// copy is running is atomic. The logic used to live twice, and `scripts/test-postinstall.js`
// carried a third copy that its tests ran against -- so the suite could stay green while the two
// shipped files did something else. This module is now the single implementation: both entry
// points call into it, and the fixtures exercise it.
//
// Integrity is why this file exists at all. npm verifies the tarball it downloads, then extracts
// it; nothing ever checked that the binary decompressed out of the tarball was the binary the
// release build hashed, or that the file installed under the chaos home still matched it when the
// launcher ran it. `bin/integrity.json`, written by `scripts/assemble-platform-packages.js`,
// records the digests, and every write below is refused unless the bytes match.
'use strict';

const fs = require('fs');
const path = require('path');
const os = require('os');
const zlib = require('zlib');
const crypto = require('crypto');

const INTEGRITY_FILE = 'integrity.json';
const INTEGRITY_SCHEMA = 'chaos-npm-integrity/1';
const HEX64 = /^[0-9a-f]{64}$/;

function sha256Hex(buffer) {
    return crypto.createHash('sha256').update(buffer).digest('hex');
}

/**
 * SHA-256 of a file, read in chunks.
 *
 * The binaries are 70-150 MB and this runs inside `npm install`, where a second full copy in the
 * heap is not something to spend on a checksum.
 */
function sha256HexOfFile(file) {
    const fd = fs.openSync(file, 'r');
    const hash = crypto.createHash('sha256');
    const buffer = Buffer.allocUnsafe(4 * 1024 * 1024);
    try {
        for (;;) {
            const read = fs.readSync(fd, buffer, 0, buffer.length, null);
            if (read === 0) break;
            hash.update(buffer.subarray(0, read));
        }
    } finally {
        fs.closeSync(fd);
    }
    return hash.digest('hex');
}

/**
 * The chaos home, matching the Rust `grok_home()`: `$CHAOS_HOME`, else `$GROK_HOME`, else an
 * existing `~/.chaos`, else legacy `~/.grok`, else `~/.chaos`. A symlinked `$HOME` resolves the
 * same way, which matters because macOS `/tmp` and some CI homes are links.
 */
function defaultChaosHome(homedir) {
    const home = homedir ?? os.homedir();
    let real;
    try { real = fs.realpathSync(home); } catch { real = home; }
    const chaos = path.join(real, '.chaos');
    const grok = path.join(real, '.grok');
    try { if (fs.existsSync(chaos)) return chaos; } catch {}
    try { if (fs.existsSync(grok)) return grok; } catch {}
    return chaos;
}

/** Same rule, with the inputs injected, so a fixture can point an install at a temporary home. */
function resolveChaosHome(env, homedir) {
    const explicit = env.CHAOS_HOME || env.GROK_HOME;
    if (explicit) return explicit;
    return defaultChaosHome(homedir);
}

/** `integrity.json` sits beside the binary it describes, inside the platform package's `bin/`. */
function integrityPath(sourceDir) {
    return path.join(sourceDir, 'bin', INTEGRITY_FILE);
}

/**
 * Read and validate one platform package's `bin/integrity.json`.
 *
 * Every check here is a claim the file makes about itself. A record that cannot be read, or that
 * describes a different binary than the one about to be installed, tells us nothing about those
 * bytes, so it is reported as a problem rather than skipped: an unreadable integrity file is the
 * same shape as a tampered one, and treating it as "nothing to check" would make deleting the
 * file a way to disable the check.
 */
function readIntegrity(sourceDir, binName) {
    const file = integrityPath(sourceDir);
    let parsed;
    try {
        parsed = JSON.parse(fs.readFileSync(file, 'utf8'));
    } catch (err) {
        if (err && err.code === 'ENOENT') {
            return { ok: false, file, problem: `${file} does not exist` };
        }
        return { ok: false, file, problem: `${file} is not readable JSON (${err.message})` };
    }
    if (parsed.schema !== INTEGRITY_SCHEMA) {
        return { ok: false, file, problem: `${file} records schema ${JSON.stringify(parsed.schema)}, expected ${JSON.stringify(INTEGRITY_SCHEMA)}` };
    }
    const fields = [];
    const binary = parsed.binary || {};
    const compressed = parsed.compressed || {};
    if (binary.name !== binName) {
        fields.push(`${file} describes binary ${JSON.stringify(binary.name)}, not ${binName}`);
    }
    if (typeof binary.sha256 !== 'string' || !HEX64.test(binary.sha256)) {
        fields.push(`${file} has no 64-hex lowercase binary sha256 (got ${JSON.stringify(binary.sha256)})`);
    }
    if (typeof binary.bytes !== 'number' || !Number.isInteger(binary.bytes) || binary.bytes <= 0) {
        fields.push(`${file} has no positive integer binary.bytes (got ${JSON.stringify(binary.bytes)})`);
    }
    if (!Number.isInteger(compressed.bytes) || compressed.bytes <= 0) {
        fields.push(`${file} has no positive integer compressed.bytes (got ${JSON.stringify(compressed.bytes)})`);
    }
    if (!parsed.version || !parsed.platform) {
        fields.push(`${file} does not name the version and platform it was assembled for`);
    }
    if (fields.length) return { ok: false, file, problem: fields.join('; ') };
    return { ok: true, file, record: parsed };
}

/**
 * Produce the installed bytes, refusing anything that does not match `integrity.json`.
 *
 * The compressed digest is checked first because it is the cheap one and because a truncated or
 * substituted archive is the more likely accident; the decompressed digest is what the installed
 * file is compared against, so it is checked even when there was nothing to decompress.
 */
function readVerifiedBytes(sourceDir, binName) {
    const integrity = readIntegrity(sourceDir, binName);
    if (!integrity.ok) {
        return { ok: false, reason: 'integrity', detail: integrity.problem };
    }
    const brotliPath = path.join(sourceDir, 'bin', `${binName}.br`);
    const binaryPath = path.join(sourceDir, 'bin', binName);
    const record = integrity.record;
    let bytes;
    if (fs.existsSync(brotliPath)) {
        if (typeof record.compressed.sha256 !== 'string' || !HEX64.test(record.compressed.sha256)) {
            return {
                ok: false, reason: 'integrity',
                detail: `${integrity.file} has no 64-hex lowercase compressed sha256 `
                    + `(got ${JSON.stringify(record.compressed.sha256)})`,
            };
        }
        const packed = fs.readFileSync(brotliPath);
        if (sha256Hex(packed) !== record.compressed.sha256) {
            return {
                ok: false, reason: 'digest', path: brotliPath,
                expected: record.compressed.sha256, actual: sha256Hex(packed),
                label: `${path.basename(brotliPath)} (compressed)`,
            };
        }
        try {
            bytes = zlib.brotliDecompressSync(packed);
        } catch (err) {
            return { ok: false, reason: 'decompress', path: brotliPath, detail: err.message };
        }
    } else if (fs.existsSync(binaryPath)) {
        bytes = fs.readFileSync(binaryPath);
    } else {
        return {
            ok: false, reason: 'missing',
            detail: `neither ${brotliPath} nor ${binaryPath} exists`,
        };
    }
    if (sha256Hex(bytes) !== record.binary.sha256) {
        return {
            ok: false, reason: 'digest', path: fs.existsSync(brotliPath) ? brotliPath : binaryPath,
            expected: record.binary.sha256, actual: sha256Hex(bytes),
            label: `${binName} (decompressed)`,
        };
    }
    if (bytes.length !== record.binary.bytes) {
        return {
            ok: false, reason: 'size', path: binaryPath,
            expected: String(record.binary.bytes), actual: String(bytes.length),
            label: binName,
        };
    }
    return { ok: true, bytes, record };
}

/**
 * Write the platform package's binary to `destPath`, atomically, and only if it matches the
 * digests recorded beside it.
 *
 * Returns `{ ok, ... }` rather than throwing or returning a bare boolean: "there is no binary
 * here" and "the binary here is not the one that was hashed" need different messages, and the
 * second one has to reach the user rather than become a silent fallback.
 */
function writeVerifiedBinary(sourceDir, binName, destPath, { isWindows = false } = {}) {
    const verified = readVerifiedBytes(sourceDir, binName);
    if (!verified.ok) return verified;
    const tmp = `${destPath}.tmp.${process.pid}`;
    try {
        fs.writeFileSync(tmp, verified.bytes);
        if (!isWindows) fs.chmodSync(tmp, 0o755);
        fs.renameSync(tmp, destPath);
        return { ok: true, bytes: verified.bytes.length };
    } catch (err) {
        return { ok: false, reason: 'write', path: destPath, detail: err.message };
    } finally {
        try { fs.unlinkSync(tmp); } catch {}
    }
}

/**
 * Point `canonicalPath` at `versionedName`.
 *
 * Unix swaps a relative symlink, which is what lets a process still running the previous binary
 * keep its mmap'd pages. Windows cannot symlink without elevation, so it copies, and a copy over
 * a running exe fails: the in-flight file is renamed aside and restored if the new copy fails, so
 * a locked binary never leaves the install with no `chaos` at all.
 */
function swapCanonical(canonicalPath, versionedName, versionedPath, { isWindows = false } = {}) {
    if (!isWindows) {
        const tmpLink = `${canonicalPath}.link.${process.pid}`;
        try { fs.unlinkSync(tmpLink); } catch {}
        fs.symlinkSync(versionedName, tmpLink);
        fs.renameSync(tmpLink, canonicalPath);
        return;
    }
    const oldPath = `${canonicalPath}.old`;
    try { fs.unlinkSync(oldPath); } catch {}
    try {
        try { fs.unlinkSync(canonicalPath); } catch {}
        fs.copyFileSync(versionedPath, canonicalPath);
    } catch {
        fs.renameSync(canonicalPath, oldPath);
        try {
            fs.copyFileSync(versionedPath, canonicalPath);
        } catch (copyErr) {
            try { fs.renameSync(oldPath, canonicalPath); } catch {}
            throw copyErr;
        }
    }
}

/** Comparator: sort "<prefix>X.Y.Z" filenames by version, newest first. */
function byVersionDescending(prefix) {
    return (a, b) => {
        const pa = a.slice(prefix.length).split('.').map(Number);
        const pb = b.slice(prefix.length).split('.').map(Number);
        for (let i = 0; i < 3; i++) {
            if ((pa[i] || 0) !== (pb[i] || 0)) return (pb[i] || 0) - (pa[i] || 0);
        }
        return 0;
    };
}

/**
 * Install `binName` from a platform package into the chaos home as `binName-<version>`, with the
 * unversioned name pointing at it.
 *
 * `verifyInstalled` decides what happens when a file for this exact version is already there and
 * the package's record cannot be read. The installer passes true, because `npm install` is the
 * moment the digests are at hand and a package that cannot say what its own bytes are should fail
 * loudly while it is still in hand; the launcher passes false, because a missing record must not
 * stop a binary that is already installed from running. When the record *is* readable, an existing
 * file is hashed and rewritten from the package either way -- this function is only reached when
 * the caller intends the canonical name to resolve to verified bytes, so a file that disagrees is
 * repaired rather than trusted.
 *
 * The post-condition check for `dangling` cannot fail on a filesystem that honours the rename or
 * copy it just reported success for; no caller can arrange for it to fail. It is there because a
 * network or FUSE mount can report a write it did not make, and `installed` is a claim worth
 * making only once the unversioned name actually resolves.
 */
function installVersionedBinary({
    sourceDir, binName, version, canonicalDir, isWindows = false, verifyInstalled = false,
}) {
    const exe = isWindows ? '.exe' : '';
    const packagedName = `${binName}${exe}`;
    const versionedName = `${binName}-${version}${exe}`;
    const versionedPath = path.join(canonicalDir, versionedName);
    const canonicalPath = path.join(canonicalDir, packagedName);
    fs.mkdirSync(canonicalDir, { recursive: true });

    const integrity = readIntegrity(sourceDir, packagedName);
    let replacedInstalled = false;
    if (fs.existsSync(versionedPath)) {
        if (!integrity.ok) {
            if (verifyInstalled) {
                return { ok: false, reason: 'integrity', detail: integrity.problem, versionedPath, canonicalPath };
            }
        } else {
            let actual = null;
            try { actual = sha256HexOfFile(versionedPath); } catch {}
            if (actual !== integrity.record.binary.sha256) {
                const rewritten = writeVerifiedBinary(sourceDir, packagedName, versionedPath, { isWindows });
                if (!rewritten.ok) return { ok: false, ...rewritten, versionedPath, canonicalPath };
                replacedInstalled = true;
            }
        }
    } else {
        const written = writeVerifiedBinary(sourceDir, packagedName, versionedPath, { isWindows });
        if (!written.ok) return { ok: false, ...written, versionedPath, canonicalPath };
    }
    try {
        swapCanonical(canonicalPath, versionedName, versionedPath, { isWindows });
    } catch (err) {
        return { ok: false, reason: 'swap', path: canonicalPath, detail: err.message, versionedPath, canonicalPath };
    }
    if (!fs.existsSync(canonicalPath)) {
        return {
            ok: false, reason: 'dangling', path: canonicalPath,
            detail: `${canonicalPath} does not resolve after the swap`, versionedPath, canonicalPath,
        };
    }
    return { ok: true, versionedName, versionedPath, canonicalPath, replacedInstalled };
}

/**
 * Remove this binary name's versioned files except the current one and the newest older one.
 *
 * The previous version stays because a process may still be running it, and on macOS replacing a
 * binary that a running process has mapped kills that process. The `^\d` test on the suffix is
 * what keeps `chaos-pager-*` out of `chaos-*` cleanup.
 */
function cleanupOldVersions({ canonicalDir, binName, version, isWindows = false }) {
    const exe = isWindows ? '.exe' : '';
    const prefix = `${binName}-`;
    const currentVersioned = `${binName}-${version}${exe}`;
    let entries;
    try {
        entries = fs.readdirSync(canonicalDir);
    } catch {
        return { removed: [], kept: [] };
    }
    const versioned = entries
        .filter((name) => {
            if (!name.startsWith(prefix)) return false;
            if (name.includes('.tmp.') || name.includes('.link.')) return false;
            if (name === currentVersioned) return false;
            return /^\d/.test(name.slice(prefix.length));
        })
        .sort(byVersionDescending(prefix));
    const removed = [];
    for (const old of versioned.slice(1)) {
        try {
            fs.unlinkSync(path.join(canonicalDir, old));
            removed.push(old);
        } catch {}
    }
    return { removed, kept: versioned.slice(0, 1).concat([currentVersioned]) };
}

/** The version encoded in a `binName-<version><exe>` filename, or null. */
function versionOfVersionedName(name, binName, isWindows = false) {
    const prefix = `${binName}-`;
    const exe = isWindows ? '.exe' : '';
    if (!name.startsWith(prefix) || !name.endsWith(exe)) return null;
    const suffix = name.slice(prefix.length, name.length - exe.length);
    return /^\d/.test(suffix) ? suffix : null;
}

module.exports = {
    INTEGRITY_FILE,
    INTEGRITY_SCHEMA,
    byVersionDescending,
    cleanupOldVersions,
    defaultChaosHome,
    installVersionedBinary,
    integrityPath,
    readIntegrity,
    readVerifiedBytes,
    resolveChaosHome,
    sha256Hex,
    sha256HexOfFile,
    swapCanonical,
    versionOfVersionedName,
    writeVerifiedBinary,
};
