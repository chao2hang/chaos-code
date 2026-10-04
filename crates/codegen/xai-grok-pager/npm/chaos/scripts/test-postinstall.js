#!/usr/bin/env node
// Tests for the installation logic in `bin/install-lib.js`.
//
// Run with:  node scripts/test-postinstall.js
//
// This file used to carry its own copy of the install logic and test that copy, which is how a
// suite stays green while the two shipped entry points do something else: the mirror drifted from
// `postinstall.js` in three places (the chaos-home rule, the cleanup suffix guard, and the Windows
// chmod) and not one test noticed. Everything below requires `../bin/install-lib.js`, so a test
// here can only pass for the code that ships. The end-to-end path -- a real `postinstall.js` run
// and a real `bin/chaos` launch against a package the real assembler built -- lives in
// `scripts/ci/test-assemble-integrity.sh`, and the split is deliberate: this file covers the
// decision each function makes, that file covers the two programs users run.
//
// Packages are built with `buildIntegrityRecord` from the assembler, which is the code that
// produces the claim in a real release, and every refusal below is checked against bytes that were
// changed *after* the record was written rather than against a hand-written record.

'use strict';

const fs = require('fs');
const os = require('os');
const path = require('path');
const zlib = require('zlib');
const assert = require('assert');

const lib = require('../bin/install-lib.js');
const { buildIntegrityRecord, INTEGRITY_FILE } = require('./assemble-platform-packages.js');

let passed = 0;
let failed = 0;

function test(name, fn) {
    try {
        fn();
        console.log(`  ✓ ${name}`);
        passed++;
    } catch (e) {
        console.error(`  ✗ ${name}`);
        console.error(`    ${e.message}`);
        failed++;
    }
}

function makeTmpDir() {
    return fs.mkdtempSync(path.join(os.tmpdir(), 'chaos-install-test-'));
}

function cleanup(dir) {
    fs.rmSync(dir, { recursive: true, force: true });
}

/**
 * A platform package in the shape the assembler leaves it: `bin/<name>.br` plus the record that
 * describes it, so the only way a test can make it disagree with itself is to change one of the
 * two afterwards.
 */
function makePackage(dir, { binName = 'chaos', version = '1.0.0', platform = 'linux-x64', raw, record } = {}) {
    const bytes = raw ?? Buffer.from(`#!/bin/sh\nprintf 'fixture ${version} %s\\n' "$*"\n`);
    const compressed = zlib.brotliCompressSync(bytes);
    const [plat, ...rest] = platform.split('-');
    const entry = record ?? buildIntegrityRecord({
        platform: plat,
        arch: rest.join('-'),
        binName,
        version,
        raw: bytes,
        compressed,
    });
    fs.mkdirSync(path.join(dir, 'bin'), { recursive: true });
    fs.writeFileSync(path.join(dir, 'bin', `${binName}.br`), compressed);
    fs.writeFileSync(path.join(dir, 'bin', INTEGRITY_FILE), `${JSON.stringify(entry, null, 2)}\n`);
    return { dir, bytes, compressed, record: entry };
}

/** Reads a record back off disk, mutates it, and writes it back. */
function editRecord(dir, binName, mutate) {
    const file = path.join(dir, 'bin', INTEGRITY_FILE);
    const record = JSON.parse(fs.readFileSync(file, 'utf8'));
    mutate(record);
    fs.writeFileSync(file, `${JSON.stringify(record, null, 2)}\n`);
    return record;
}

/** Replaces the archive with one that no longer matches the record beside it. */
function tamperArchive(dir, binName) {
    const file = path.join(dir, 'bin', `${binName}.br`);
    const bytes = Buffer.from(fs.readFileSync(file));
    bytes[Math.floor(bytes.length / 2)] ^= 0x01;
    fs.writeFileSync(file, bytes);
    return file;
}

function installFrom(pkg, opts) {
    return lib.installVersionedBinary({ sourceDir: pkg.dir, binName: 'chaos', version: '1.0.0', ...opts });
}

console.log('installVersionedBinary: the install a user gets');

test('a fresh install writes the versioned file byte-for-byte and links the plain name at it', () => {
    const root = makeTmpDir();
    try {
        const pkg = makePackage(path.join(root, 'pkg'));
        const bin = path.join(root, 'bin');
        const result = lib.installVersionedBinary({
            sourceDir: pkg.dir, binName: 'chaos', version: '1.0.0', canonicalDir: bin,
        });
        assert.ok(result.ok, `install failed: ${JSON.stringify(result)}`);
        assert.deepStrictEqual(fs.readFileSync(result.versionedPath), pkg.bytes);
        assert.strictEqual(fs.readlinkSync(result.canonicalPath), 'chaos-1.0.0');
        assert.strictEqual(fs.readFileSync(result.canonicalPath, 'utf8'), pkg.bytes.toString());
    } finally { cleanup(root); }
});

test('the installed binary is executable without being asked to be', () => {
    const root = makeTmpDir();
    try {
        const pkg = makePackage(path.join(root, 'pkg'));
        const result = lib.installVersionedBinary({
            sourceDir: pkg.dir, binName: 'chaos', version: '1.0.0', canonicalDir: path.join(root, 'bin'),
        });
        assert.ok(result.ok, JSON.stringify(result));
        // eslint-disable-next-line no-bitwise
        assert.strictEqual(fs.statSync(result.versionedPath).mode & 0o777, 0o755);
    } finally { cleanup(root); }
});

test('a rewrite lands as a new inode instead of writing into the one a process may hold', () => {
    const root = makeTmpDir();
    try {
        const pkg = makePackage(path.join(root, 'pkg'));
        const bin = path.join(root, 'bin');
        fs.mkdirSync(bin, { recursive: true });
        const dest = path.join(bin, 'chaos-1.0.0');
        // This is what the self-heal path does with a file that disagrees: it rewrites it. Those
        // bytes may be mapped by a live process, and on macOS writing into a mapped inode kills it,
        // so the new bytes have to arrive as a different inode under the same name.
        fs.writeFileSync(dest, 'the bytes a running process is still executing');
        const before = fs.statSync(dest);
        const result = lib.writeVerifiedBinary(pkg.dir, 'chaos', dest);
        assert.ok(result.ok, JSON.stringify(result));
        const after = fs.statSync(dest);
        assert.notStrictEqual(after.ino, before.ino, 'the rewrite reused the inode it was replacing');
        assert.deepStrictEqual(fs.readFileSync(dest), pkg.bytes);
    } finally { cleanup(root); }
});

test('an upgrade leaves the previous version in place and moves the link', () => {
    const root = makeTmpDir();
    try {
        const bin = path.join(root, 'bin');
        const v1 = makePackage(path.join(root, 'p1'), { version: '1.0.0' });
        const v2 = makePackage(path.join(root, 'p2'), { version: '1.1.0' });
        lib.installVersionedBinary({ sourceDir: v1.dir, binName: 'chaos', version: '1.0.0', canonicalDir: bin });
        const second = lib.installVersionedBinary({
            sourceDir: v2.dir, binName: 'chaos', version: '1.1.0', canonicalDir: bin,
        });
        assert.ok(second.ok, JSON.stringify(second));
        assert.strictEqual(fs.readlinkSync(path.join(bin, 'chaos')), 'chaos-1.1.0');
        // Both must still exist: a process running 1.0.0 has those pages mapped, and on macOS
        // replacing a mapped binary kills it rather than deferring the unlink.
        assert.ok(fs.existsSync(path.join(bin, 'chaos-1.0.0')));
        assert.deepStrictEqual(fs.readFileSync(path.join(bin, 'chaos-1.1.0')), v2.bytes);
    } finally { cleanup(root); }
});

test('re-installing the same version does not rewrite the file', () => {
    const root = makeTmpDir();
    try {
        const bin = path.join(root, 'bin');
        const pkg = makePackage(path.join(root, 'pkg'));
        const first = lib.installVersionedBinary({
            sourceDir: pkg.dir, binName: 'chaos', version: '1.0.0', canonicalDir: bin, verifyInstalled: true,
        });
        const mtime = fs.statSync(first.versionedPath).mtimeMs;
        const again = lib.installVersionedBinary({
            sourceDir: pkg.dir, binName: 'chaos', version: '1.0.0', canonicalDir: bin, verifyInstalled: true,
        });
        assert.ok(again.ok, JSON.stringify(again));
        assert.strictEqual(again.replacedInstalled, false);
        assert.strictEqual(fs.statSync(first.versionedPath).mtimeMs, mtime);
    } finally { cleanup(root); }
});

test('verifyInstalled rewrites an installed file that was changed under us', () => {
    const root = makeTmpDir();
    try {
        const bin = path.join(root, 'bin');
        const pkg = makePackage(path.join(root, 'pkg'));
        const first = lib.installVersionedBinary({
            sourceDir: pkg.dir, binName: 'chaos', version: '1.0.0', canonicalDir: bin,
        });
        fs.writeFileSync(first.versionedPath, Buffer.from('somebody edited the installed binary'));
        const again = lib.installVersionedBinary({
            sourceDir: pkg.dir, binName: 'chaos', version: '1.0.0', canonicalDir: bin, verifyInstalled: true,
        });
        assert.ok(again.ok, JSON.stringify(again));
        assert.strictEqual(again.replacedInstalled, true);
        assert.deepStrictEqual(fs.readFileSync(first.versionedPath), pkg.bytes);
    } finally { cleanup(root); }
});

test('an existing file that disagrees is rewritten even by the caller that does not verify', () => {
    // Both entry points self-heal: this function is only reached when the canonical name is
    // supposed to resolve to verified bytes, so a versioned file that disagrees is repaired
    // whoever is asking. What the flag actually changes is the unreadable-record case below.
    const root = makeTmpDir();
    try {
        const bin = path.join(root, 'bin');
        const pkg = makePackage(path.join(root, 'pkg'));
        const first = lib.installVersionedBinary({
            sourceDir: pkg.dir, binName: 'chaos', version: '1.0.0', canonicalDir: bin,
        });
        fs.writeFileSync(first.versionedPath, Buffer.from('edited'));
        const again = lib.installVersionedBinary({
            sourceDir: pkg.dir, binName: 'chaos', version: '1.0.0', canonicalDir: bin,
        });
        assert.ok(again.ok, JSON.stringify(again));
        assert.strictEqual(again.replacedInstalled, true);
        assert.deepStrictEqual(fs.readFileSync(first.versionedPath), pkg.bytes);
    } finally { cleanup(root); }
});

test('an unreadable record stops the installer but not a binary that is already installed', () => {
    const root = makeTmpDir();
    try {
        const bin = path.join(root, 'bin');
        const pkg = makePackage(path.join(root, 'pkg'));
        const first = lib.installVersionedBinary({
            sourceDir: pkg.dir, binName: 'chaos', version: '1.0.0', canonicalDir: bin,
        });
        assert.ok(first.ok, JSON.stringify(first));
        fs.rmSync(path.join(pkg.dir, 'bin', INTEGRITY_FILE));

        const installer = lib.installVersionedBinary({
            sourceDir: pkg.dir, binName: 'chaos', version: '1.0.0', canonicalDir: bin, verifyInstalled: true,
        });
        assert.strictEqual(installer.ok, false);
        assert.strictEqual(installer.reason, 'integrity');

        const launcher = lib.installVersionedBinary({
            sourceDir: pkg.dir, binName: 'chaos', version: '1.0.0', canonicalDir: bin,
        });
        assert.ok(launcher.ok, `a lost record must not break a working install: ${JSON.stringify(launcher)}`);
        assert.deepStrictEqual(fs.readFileSync(first.versionedPath), pkg.bytes);
    } finally { cleanup(root); }
});

test('an old-style plain canonical file is replaced by the versioned link', () => {
    const root = makeTmpDir();
    try {
        const bin = path.join(root, 'bin');
        fs.mkdirSync(bin, { recursive: true });
        fs.writeFileSync(path.join(bin, 'chaos'), 'installed by install.sh once upon a time');
        const pkg = makePackage(path.join(root, 'pkg'));
        const result = lib.installVersionedBinary({
            sourceDir: pkg.dir, binName: 'chaos', version: '1.0.0', canonicalDir: bin,
        });
        assert.ok(result.ok, JSON.stringify(result));
        assert.strictEqual(fs.readlinkSync(path.join(bin, 'chaos')), 'chaos-1.0.0');
    } finally { cleanup(root); }
});

test('a dangling link left by a deleted version is replaced, not followed', () => {
    const root = makeTmpDir();
    try {
        const bin = path.join(root, 'bin');
        fs.mkdirSync(bin, { recursive: true });
        fs.symlinkSync('chaos-0.9.0', path.join(bin, 'chaos'));
        const pkg = makePackage(path.join(root, 'pkg'));
        const result = lib.installVersionedBinary({
            sourceDir: pkg.dir, binName: 'chaos', version: '1.0.0', canonicalDir: bin,
        });
        assert.ok(result.ok, JSON.stringify(result));
        assert.strictEqual(fs.readlinkSync(path.join(bin, 'chaos')), 'chaos-1.0.0');
        assert.ok(fs.existsSync(path.join(bin, 'chaos')));
    } finally { cleanup(root); }
});

test('the swap leaves no temporary link behind', () => {
    const root = makeTmpDir();
    try {
        const bin = path.join(root, 'bin');
        const pkg = makePackage(path.join(root, 'pkg'));
        const result = lib.installVersionedBinary({
            sourceDir: pkg.dir, binName: 'chaos', version: '1.0.0', canonicalDir: bin,
        });
        assert.ok(result.ok, JSON.stringify(result));
        const strays = fs.readdirSync(bin).filter((n) => n.includes('.link.') || n.includes('.tmp.'));
        assert.deepStrictEqual(strays, []);
    } finally { cleanup(root); }
});

console.log('\nintegrity: every way the bytes can stop being the bytes');

test('a package with no record installs nothing', () => {
    const root = makeTmpDir();
    try {
        const pkg = makePackage(path.join(root, 'pkg'));
        fs.rmSync(path.join(pkg.dir, 'bin', INTEGRITY_FILE));
        const bin = path.join(root, 'bin');
        const result = lib.installVersionedBinary({
            sourceDir: pkg.dir, binName: 'chaos', version: '1.0.0', canonicalDir: bin,
        });
        assert.strictEqual(result.ok, false);
        assert.strictEqual(result.reason, 'integrity');
        assert.match(result.detail, /does not exist/);
        assert.ok(!fs.existsSync(path.join(bin, 'chaos')), 'a refused install must not leave a link');
        assert.ok(!fs.existsSync(path.join(bin, 'chaos-1.0.0')), 'a refused install must not leave a binary');
    } finally { cleanup(root); }
});

test('a record for another schema is not this package\'s record', () => {
    const root = makeTmpDir();
    try {
        const pkg = makePackage(path.join(root, 'pkg'));
        editRecord(pkg.dir, 'chaos', (r) => { r.schema = 'chaos-npm-integrity/0'; });
        const result = installFrom(pkg, { canonicalDir: path.join(root, 'bin') });
        assert.strictEqual(result.ok, false);
        assert.strictEqual(result.reason, 'integrity');
        assert.match(result.detail, /chaos-npm-integrity\/0/);
    } finally { cleanup(root); }
});

test('a tampered archive is refused with both digests in the message', () => {
    const root = makeTmpDir();
    try {
        const pkg = makePackage(path.join(root, 'pkg'));
        tamperArchive(pkg.dir, 'chaos');
        const result = installFrom(pkg, { canonicalDir: path.join(root, 'bin') });
        assert.strictEqual(result.ok, false);
        assert.strictEqual(result.reason, 'digest');
        assert.match(result.label, /compressed/);
        assert.strictEqual(result.expected, pkg.record.compressed.sha256);
        assert.match(result.actual, /^[0-9a-f]{64}$/);
        assert.notStrictEqual(result.actual, result.expected);
    } finally { cleanup(root); }
});

test('a record whose binary digest no bytes hash to is refused on the decompressed half', () => {
    // The archive still matches here, so a check that stopped at the archive would wave this
    // through -- and this is the exact record a package could carry while its installer rejects it.
    const root = makeTmpDir();
    try {
        const pkg = makePackage(path.join(root, 'pkg'));
        editRecord(pkg.dir, 'chaos', (r) => { r.binary.sha256 = '0'.repeat(64); });
        const result = installFrom(pkg, { canonicalDir: path.join(root, 'bin') });
        assert.strictEqual(result.ok, false);
        assert.strictEqual(result.reason, 'digest');
        assert.match(result.label, /decompressed/);
        assert.strictEqual(result.actual, lib.sha256Hex(pkg.bytes));
    } finally { cleanup(root); }
});

test('a record that understates the binary length is refused on size', () => {
    const root = makeTmpDir();
    try {
        const pkg = makePackage(path.join(root, 'pkg'));
        editRecord(pkg.dir, 'chaos', (r) => { r.binary.bytes = r.binary.bytes + 1; });
        const result = installFrom(pkg, { canonicalDir: path.join(root, 'bin') });
        assert.strictEqual(result.ok, false);
        assert.strictEqual(result.reason, 'size');
        assert.strictEqual(result.actual, String(pkg.bytes.length));
    } finally { cleanup(root); }
});

test('a record describing a different binary name cannot be used to install this one', () => {
    const root = makeTmpDir();
    try {
        const pkg = makePackage(path.join(root, 'pkg'));
        editRecord(pkg.dir, 'chaos', (r) => { r.binary.name = 'grok'; });
        const result = installFrom(pkg, { canonicalDir: path.join(root, 'bin') });
        assert.strictEqual(result.ok, false);
        assert.strictEqual(result.reason, 'integrity');
        assert.match(result.detail, /"grok"/);
    } finally { cleanup(root); }
});

test('a digest that is not 64 lowercase hex is not a digest', () => {
    const root = makeTmpDir();
    try {
        const pkg = makePackage(path.join(root, 'pkg'));
        editRecord(pkg.dir, 'chaos', (r) => { r.binary.sha256 = '0'.repeat(63); });
        const result = installFrom(pkg, { canonicalDir: path.join(root, 'bin') });
        assert.strictEqual(result.ok, false);
        assert.strictEqual(result.reason, 'integrity');
    } finally { cleanup(root); }
});

test('a record with no usable binary length is a bad record, not a size mismatch', () => {
    const root = makeTmpDir();
    try {
        const pkg = makePackage(path.join(root, 'pkg'));
        editRecord(pkg.dir, 'chaos', (r) => { r.binary.bytes = 0; });
        const result = installFrom(pkg, { canonicalDir: path.join(root, 'bin') });
        assert.strictEqual(result.ok, false);
        // 'size' would mean the bytes disagreed with a usable claim; here the claim itself is the
        // broken thing, and reading those two differently is why the record is validated first.
        assert.strictEqual(result.reason, 'integrity', JSON.stringify(result));
        assert.match(result.detail, /positive integer binary\.bytes/);
    } finally { cleanup(root); }
});

test('a record with no usable compressed length is refused the same way', () => {
    const root = makeTmpDir();
    try {
        const pkg = makePackage(path.join(root, 'pkg'));
        editRecord(pkg.dir, 'chaos', (r) => { r.compressed.bytes = '36'; });
        const result = installFrom(pkg, { canonicalDir: path.join(root, 'bin') });
        assert.strictEqual(result.ok, false);
        assert.strictEqual(result.reason, 'integrity', JSON.stringify(result));
        assert.match(result.detail, /positive integer compressed\.bytes/);
    } finally { cleanup(root); }
});

test('a record that does not name its version and platform is not this build', () => {
    const root = makeTmpDir();
    try {
        const pkg = makePackage(path.join(root, 'pkg'));
        editRecord(pkg.dir, 'chaos', (r) => { delete r.platform; });
        const result = installFrom(pkg, { canonicalDir: path.join(root, 'bin') });
        assert.strictEqual(result.ok, false);
        assert.strictEqual(result.reason, 'integrity', JSON.stringify(result));
        assert.match(result.detail, /version and platform/);
    } finally { cleanup(root); }
});

test('a compressed digest the record cannot state is a bad record, not a mismatch', () => {
    const root = makeTmpDir();
    try {
        const pkg = makePackage(path.join(root, 'pkg'));
        editRecord(pkg.dir, 'chaos', (r) => { r.compressed.sha256 = 'abc'; });
        const result = installFrom(pkg, { canonicalDir: path.join(root, 'bin') });
        assert.strictEqual(result.ok, false);
        // Without the shape check the comparison further down still refuses, but reports the
        // package as altered -- 'digest' names an expected value that was never a digest --
        // instead of saying the record itself is unusable.
        assert.strictEqual(result.reason, 'integrity', JSON.stringify(result));
        assert.match(result.detail, /compressed sha256/);
    } finally { cleanup(root); }
});

test('a truncated archive is refused rather than half-installed', () => {
    const root = makeTmpDir();
    try {
        const pkg = makePackage(path.join(root, 'pkg'));
        const file = path.join(pkg.dir, 'bin', 'chaos.br');
        const bytes = fs.readFileSync(file);
        // Keep the compressed digest true and break the payload: that is the case where only the
        // decompression can tell, and it must not produce a usable-looking file.
        editRecord(pkg.dir, 'chaos', (r) => {
            r.compressed.sha256 = lib.sha256Hex(bytes.subarray(0, bytes.length - 4));
            r.compressed.bytes = bytes.length - 4;
        });
        fs.writeFileSync(file, bytes.subarray(0, bytes.length - 4));
        const result = installFrom(pkg, { canonicalDir: path.join(root, 'bin') });
        assert.strictEqual(result.ok, false);
        assert.ok(['digest', 'decompress'].includes(result.reason), JSON.stringify(result));
        assert.ok(!fs.existsSync(path.join(root, 'bin', 'chaos-1.0.0')));
    } finally { cleanup(root); }
});

test('an uncompressed binary beside the record is held to the same digest', () => {
    const root = makeTmpDir();
    try {
        const pkg = makePackage(path.join(root, 'pkg'));
        const rawFile = path.join(pkg.dir, 'bin', 'chaos');
        fs.writeFileSync(rawFile, pkg.bytes);
        fs.rmSync(path.join(pkg.dir, 'bin', 'chaos.br'));
        const bin = path.join(root, 'bin');
        const result = lib.installVersionedBinary({
            sourceDir: pkg.dir, binName: 'chaos', version: '1.0.0', canonicalDir: bin,
        });
        assert.ok(result.ok, JSON.stringify(result));
        assert.deepStrictEqual(fs.readFileSync(result.versionedPath), pkg.bytes);

        fs.writeFileSync(rawFile, Buffer.from('substituted after the record was written'));
        const root2 = makeTmpDir();
        try {
            const refused = lib.installVersionedBinary({
                sourceDir: pkg.dir, binName: 'chaos', version: '2.0.0', canonicalDir: path.join(root2, 'bin'),
            });
            assert.strictEqual(refused.ok, false);
            assert.strictEqual(refused.reason, 'digest');
        } finally { cleanup(root2); }
    } finally { cleanup(root); }
});

test('a package with neither an archive nor a binary says so', () => {
    const root = makeTmpDir();
    try {
        const pkg = makePackage(path.join(root, 'pkg'));
        fs.rmSync(path.join(pkg.dir, 'bin', 'chaos.br'));
        const result = installFrom(pkg, { canonicalDir: path.join(root, 'bin') });
        assert.strictEqual(result.ok, false);
        assert.strictEqual(result.reason, 'missing');
    } finally { cleanup(root); }
});

test('readIntegrity refuses a record that is not JSON instead of treating it as absent', () => {
    const root = makeTmpDir();
    try {
        const pkg = makePackage(path.join(root, 'pkg'));
        fs.writeFileSync(path.join(pkg.dir, 'bin', INTEGRITY_FILE), '{ "schema": ');
        const read = lib.readIntegrity(pkg.dir, 'chaos');
        assert.strictEqual(read.ok, false);
        assert.match(read.problem, /not readable JSON/);
    } finally { cleanup(root); }
});

console.log('\nWindows: the same install through a copy');

test('the versioned name carries .exe and the canonical name is a copy, not a link', () => {
    const root = makeTmpDir();
    try {
        const pkg = makePackage(path.join(root, 'pkg'), { binName: 'chaos.exe' });
        const bin = path.join(root, 'bin');
        const result = lib.installVersionedBinary({
            sourceDir: pkg.dir, binName: 'chaos', version: '1.0.0', canonicalDir: bin, isWindows: true,
        });
        assert.ok(result.ok, JSON.stringify(result));
        assert.strictEqual(result.versionedName, 'chaos-1.0.0.exe');
        assert.strictEqual(path.basename(result.canonicalPath), 'chaos.exe');
        assert.deepStrictEqual(fs.readFileSync(result.canonicalPath), pkg.bytes);
        assert.strictEqual(fs.lstatSync(result.canonicalPath).isSymbolicLink(), false);
    } finally { cleanup(root); }
});

test('a Windows swap removes the .old sidecar it made on the way out', () => {
    const root = makeTmpDir();
    try {
        const bin = path.join(root, 'bin');
        const v1 = makePackage(path.join(root, 'p1'), { binName: 'chaos.exe', version: '1.0.0' });
        const v2 = makePackage(path.join(root, 'p2'), { binName: 'chaos.exe', version: '1.1.0' });
        lib.installVersionedBinary({
            sourceDir: v1.dir, binName: 'chaos', version: '1.0.0', canonicalDir: bin, isWindows: true,
        });
        const second = lib.installVersionedBinary({
            sourceDir: v2.dir, binName: 'chaos', version: '1.1.0', canonicalDir: bin, isWindows: true,
        });
        assert.ok(second.ok, JSON.stringify(second));
        assert.deepStrictEqual(fs.readFileSync(path.join(bin, 'chaos.exe')), v2.bytes);
        assert.ok(!fs.existsSync(path.join(bin, 'chaos.exe.old')));
    } finally { cleanup(root); }
});

test('a destination that cannot be written is reported, not swallowed', () => {
    // npm can run under a home that is read-only (a container with a mounted store), and the
    // launcher's fallback path depends on this returning a reason instead of throwing.
    if (typeof process.getuid === 'function' && process.getuid() === 0) {
        return; // root ignores the mode bits below, so the case cannot be made here
    }
    const root = makeTmpDir();
    try {
        const pkg = makePackage(path.join(root, 'pkg'));
        const locked = path.join(root, 'locked');
        fs.mkdirSync(locked);
        fs.chmodSync(locked, 0o500);
        try {
            const result = lib.writeVerifiedBinary(pkg.dir, 'chaos', path.join(locked, 'chaos'));
            assert.strictEqual(result.ok, false);
            assert.strictEqual(result.reason, 'write');
            assert.ok(result.detail, 'the refusal has to carry the OS reason');
        } finally {
            fs.chmodSync(locked, 0o700);
        }
    } finally { cleanup(root); }
});

console.log('\nresolveChaosHome: the same rule the Rust side uses');

test('$CHAOS_HOME wins over $GROK_HOME', () => {
    assert.strictEqual(
        lib.resolveChaosHome({ CHAOS_HOME: '/tmp/a', GROK_HOME: '/tmp/b' }, '/home/u'), '/tmp/a');
});

test('$GROK_HOME is still honoured on its own', () => {
    assert.strictEqual(lib.resolveChaosHome({ GROK_HOME: '/tmp/b' }, '/home/u'), '/tmp/b');
});

test('an existing ~/.chaos is preferred over a legacy ~/.grok', () => {
    const root = makeTmpDir();
    try {
        fs.mkdirSync(path.join(root, '.chaos'));
        fs.mkdirSync(path.join(root, '.grok'));
        assert.strictEqual(lib.defaultChaosHome(root), path.join(root, '.chaos'));
    } finally { cleanup(root); }
});

test('a legacy ~/.grok is used when there is no ~/.chaos', () => {
    const root = makeTmpDir();
    try {
        fs.mkdirSync(path.join(root, '.grok'));
        assert.strictEqual(lib.defaultChaosHome(root), path.join(root, '.grok'));
    } finally { cleanup(root); }
});

test('with neither, the default is ~/.chaos', () => {
    const root = makeTmpDir();
    try {
        assert.strictEqual(lib.defaultChaosHome(root), path.join(root, '.chaos'));
    } finally { cleanup(root); }
});

test('a symlinked home resolves to the directory the link points at', () => {
    const root = makeTmpDir();
    try {
        const real = path.join(root, 'real');
        const link = path.join(root, 'link');
        fs.mkdirSync(real);
        fs.symlinkSync('real', link);
        assert.strictEqual(lib.defaultChaosHome(link), path.join(fs.realpathSync(real), '.chaos'));
    } finally { cleanup(root); }
});

console.log('\ncleanupOldVersions: keeping what a running process may still need');

function seedBin(root, names) {
    const bin = path.join(root, 'bin');
    fs.mkdirSync(bin, { recursive: true });
    for (const name of names) fs.writeFileSync(path.join(bin, name), name);
    return bin;
}

test('the current version and the newest older one survive, the rest go', () => {
    const root = makeTmpDir();
    try {
        const bin = seedBin(root, ['chaos-1.0.0', 'chaos-0.9.0', 'chaos-0.8.0', 'chaos-0.7.0']);
        const { removed, kept } = lib.cleanupOldVersions({ canonicalDir: bin, binName: 'chaos', version: '1.0.0' });
        assert.deepStrictEqual(removed, ['chaos-0.8.0', 'chaos-0.7.0']);
        assert.deepStrictEqual(kept, ['chaos-0.9.0', 'chaos-1.0.0']);
        assert.ok(fs.existsSync(path.join(bin, 'chaos-0.9.0')));
    } finally { cleanup(root); }
});

test('a single older version is kept, because it may be mapped by a running process', () => {
    const root = makeTmpDir();
    try {
        const bin = seedBin(root, ['chaos-1.0.0', 'chaos-0.9.0']);
        const { removed } = lib.cleanupOldVersions({ canonicalDir: bin, binName: 'chaos', version: '1.0.0' });
        assert.deepStrictEqual(removed, []);
    } finally { cleanup(root); }
});

test('cleanup with no older versions is a no-op, not an error', () => {
    const root = makeTmpDir();
    try {
        const bin = seedBin(root, ['chaos-1.0.0']);
        const { removed, kept } = lib.cleanupOldVersions({ canonicalDir: bin, binName: 'chaos', version: '1.0.0' });
        assert.deepStrictEqual(removed, []);
        assert.deepStrictEqual(kept, ['chaos-1.0.0']);
    } finally { cleanup(root); }
});

test('a missing bin directory is nothing to clean', () => {
    const root = makeTmpDir();
    try {
        const { removed } = lib.cleanupOldVersions({
            canonicalDir: path.join(root, 'nope'), binName: 'chaos', version: '1.0.0',
        });
        assert.deepStrictEqual(removed, []);
    } finally { cleanup(root); }
});

test('in-flight .tmp. and .link. files are not cleanup targets', () => {
    const root = makeTmpDir();
    try {
        const bin = seedBin(root, [
            'chaos-1.0.0', 'chaos-0.9.0', 'chaos-0.8.0',
            'chaos-0.8.0.tmp.4242', 'chaos.link.4242',
        ]);
        const { removed } = lib.cleanupOldVersions({ canonicalDir: bin, binName: 'chaos', version: '1.0.0' });
        assert.deepStrictEqual(removed, ['chaos-0.8.0']);
        assert.ok(fs.existsSync(path.join(bin, 'chaos-0.8.0.tmp.4242')));
    } finally { cleanup(root); }
});

test('cleaning chaos-* leaves chaos-pager-* alone, and the other way round', () => {
    const root = makeTmpDir();
    try {
        const bin = seedBin(root, [
            'chaos-2.0.0', 'chaos-1.0.0', 'chaos-0.9.0', 'chaos-0.8.0',
            'chaos-pager-2.0.0', 'chaos-pager-1.0.0', 'chaos-pager-0.9.0', 'chaos-pager-0.8.0',
        ]);
        const chaos = lib.cleanupOldVersions({ canonicalDir: bin, binName: 'chaos', version: '2.0.0' });
        assert.deepStrictEqual(chaos.removed, ['chaos-0.9.0', 'chaos-0.8.0']);
        for (const name of ['chaos-pager-2.0.0', 'chaos-pager-1.0.0', 'chaos-pager-0.9.0', 'chaos-pager-0.8.0']) {
            assert.ok(fs.existsSync(path.join(bin, name)), `${name} must survive a chaos cleanup`);
        }
        const pager = lib.cleanupOldVersions({ canonicalDir: bin, binName: 'chaos-pager', version: '2.0.0' });
        assert.deepStrictEqual(pager.removed, ['chaos-pager-0.9.0', 'chaos-pager-0.8.0']);
        assert.ok(fs.existsSync(path.join(bin, 'chaos-2.0.0')), 'chaos-2.0.0 must survive a pager cleanup');
    } finally { cleanup(root); }
});

test('files that are not versioned binaries are never touched', () => {
    const root = makeTmpDir();
    try {
        const bin = seedBin(root, ['chaos-2.0.0', 'chaos-1.0.0', 'config.toml', 'chaos-pager']);
        const { removed } = lib.cleanupOldVersions({ canonicalDir: bin, binName: 'chaos', version: '2.0.0' });
        assert.deepStrictEqual(removed, []);
        assert.ok(fs.existsSync(path.join(bin, 'chaos-pager')));
        assert.ok(fs.existsSync(path.join(bin, 'config.toml')));
    } finally { cleanup(root); }
});

console.log('\nnames and ordering');

test('version sorting crosses the digit boundary and the major boundary', () => {
    const names = ['chaos-0.1.9', 'chaos-0.1.10', 'chaos-1.0.0', 'chaos-0.2.0'];
    assert.deepStrictEqual(
        [...names].sort(lib.byVersionDescending('chaos-')),
        ['chaos-1.0.0', 'chaos-0.2.0', 'chaos-0.1.10', 'chaos-0.1.9']);
});

test('cleanup sorts by version, not by name', () => {
    const root = makeTmpDir();
    try {
        const bin = seedBin(root, ['chaos-0.10.0', 'chaos-0.9.0', 'chaos-0.2.0', 'chaos-1.0.0']);
        const { kept } = lib.cleanupOldVersions({ canonicalDir: bin, binName: 'chaos', version: '1.0.0' });
        assert.deepStrictEqual(kept, ['chaos-0.10.0', 'chaos-1.0.0']);
    } finally { cleanup(root); }
});

test('versionOfVersionedName reads the version out of the name the link points at', () => {
    assert.strictEqual(lib.versionOfVersionedName('chaos-0.4.2', 'chaos'), '0.4.2');
    assert.strictEqual(lib.versionOfVersionedName('chaos-0.4.2.exe', 'chaos', true), '0.4.2');
    assert.strictEqual(lib.versionOfVersionedName('chaos-pager-1.0.0', 'chaos'), null);
    assert.strictEqual(lib.versionOfVersionedName('chaos-latest', 'chaos'), null);
    assert.strictEqual(lib.versionOfVersionedName('chaos-0.4.2', 'chaos', true), null);
});

test('sha256HexOfFile matches sha256Hex across the chunk boundary', () => {
    const root = makeTmpDir();
    try {
        const file = path.join(root, 'big');
        const bytes = Buffer.alloc((4 << 20) + 12345, 7);
        for (let i = 0; i < bytes.length; i += 4093) bytes[i] = (bytes[i] + i) & 0xff;
        fs.writeFileSync(file, bytes);
        assert.strictEqual(lib.sha256HexOfFile(file), lib.sha256Hex(bytes));
    } finally { cleanup(root); }
});

console.log('\nthe shipped entry points');

test('bin/chaos is still the node launcher that hands off to the bootstrap', () => {
    const entry = fs.readFileSync(path.join(__dirname, '..', 'bin', 'chaos'), 'utf8');
    assert.match(entry, /require\('\.\/chaos-bootstrap\.js'\)/);
});

test('both entry points go through install-lib rather than carrying their own copy', () => {
    for (const file of ['postinstall.js', 'chaos-bootstrap.js']) {
        const text = fs.readFileSync(path.join(__dirname, '..', 'bin', file), 'utf8');
        assert.match(text, /require\('\.\/install-lib\.js'\)/, `${file} must use the shared module`);
        assert.ok(
            !/brotliDecompressSync/.test(text),
            `${file} must not decompress on its own; that is where the digest check lives`,
        );
    }
});

test('the installer refuses by exiting non-zero, so a broken package cannot look installed', () => {
    const text = fs.readFileSync(path.join(__dirname, '..', 'bin', 'postinstall.js'), 'utf8');
    assert.match(text, /if \(!installBinary\('chaos', platformDir\)\) \{\n    process\.exit\(1\);/);
});

console.log(`\n${passed} passed, ${failed} failed`);
process.exit(failed ? 1 : 0);
