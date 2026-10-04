#!/usr/bin/env node
// Runs once after npm install/update. Reads the chaos binary from the
// matching per-platform optional dependency (chaos-code-<platform>) and
// installs it to the chaos home's bin/ using versioned filenames:
//
//   Unix:    chaos-<version>  +  chaos  (symlink)
//   Windows: chaos-<version>.exe  +  chaos.exe  (copy)
//
// Versioned files ensure running processes are never disrupted on macOS
// (replacing a binary that a running process has mmap'd causes SIGKILL
// because the kernel can no longer verify the code signature).
//
// The install itself lives in `./install-lib.js`, which `./chaos-bootstrap.js`
// also uses. That shared module is where the digests in the platform package's
// `bin/integrity.json` are checked: nothing is written under the chaos home
// unless the bytes hash to what the release build recorded for them.
'use strict';

const path = require('path');
const fs = require('fs');
const { execSync } = require('child_process');
const TOML = require('@iarna/toml');
const lib = require('./install-lib.js');

const GROK_HOME = lib.resolveChaosHome(process.env);
const CANONICAL_DIR = path.join(GROK_HOME, 'bin');

const key = `${process.platform}-${process.arch}`;
const SUPPORTED = new Set([
    'darwin-arm64',
    'darwin-x64',
    'linux-x64',
    'linux-arm64',
    'win32-x64',
    'win32-arm64',
]);
if (!SUPPORTED.has(key)) {
    console.error(`chaos-code: unsupported platform ${key}`);
    process.exit(0);
}

// Resolve the per-platform sibling package's directory. The matching
// optionalDependency is installed by npm based on `os`/`cpu` filters; the
// other five are silently skipped. If the matching one is missing, npm was
// likely invoked with --no-optional or the platform is unsupported.
function resolvePlatformPackageDir() {
    const platformPkg = `chaos-code-${key}`;
    try {
        return path.dirname(require.resolve(`${platformPkg}/package.json`));
    } catch {
        return null;
    }
}

let version;
try { version = require('../package.json').version; } catch {}
if (!version) {
    console.error('chaos-code: unable to determine version');
    process.exit(0);
}

const IS_WINDOWS = process.platform === 'win32';
const EXE = IS_WINDOWS ? '.exe' : '';

fs.mkdirSync(CANONICAL_DIR, { recursive: true });

/**
 * One message per way an install can fail.
 *
 * A digest refusal is the one that must not read like a bug in this package: the user's options
 * are to fetch the package again or to report it, never to install around it. `integrity` is the
 * adjacent case -- the file that vouches for the bytes is missing or malformed, which is the same
 * position as a digest mismatch as far as the install is concerned.
 */
function reportInstallFailure(pkgName, result) {
    if (result.reason === 'digest') {
        console.error(`chaos-code: refusing to install ${pkgName}: ${result.label} does not match bin/${lib.INTEGRITY_FILE}`);
        console.error(`  file:     ${result.path}`);
        console.error(`  expected: ${result.expected}`);
        console.error(`  actual:   ${result.actual}`);
        console.error('  These are not the bytes the release build hashed. Re-run the install after');
        console.error('  `npm cache clean --force`; if it repeats, the package you received was');
        console.error('  altered, so please report it instead of installing around it.');
        return;
    }
    if (result.reason === 'integrity') {
        console.error(`chaos-code: refusing to install ${pkgName}: ${result.detail}`);
        console.error('  The package does not say which bytes it is meant to contain, so there is');
        console.error('  nothing to check them against. Re-install from the registry; a package');
        console.error('  assembled without bin/integrity.json must not be published.');
        return;
    }
    if (result.reason === 'missing') {
        console.error(`chaos-code: missing binary in ${pkgName}: ${result.detail}`);
        return;
    }
    if (result.reason === 'swap') {
        console.error(`chaos-code: failed to update ${result.path}: ${result.detail}`);
        console.error('Close all running chaos processes and try again.');
        return;
    }
    console.error(`chaos-code: could not install ${pkgName} (${result.reason}): ${result.detail || result.path}`);
}

function installBinary(binName, sourceDir) {
    const result = lib.installVersionedBinary({
        sourceDir,
        binName,
        version,
        canonicalDir: CANONICAL_DIR,
        isWindows: IS_WINDOWS,
        verifyInstalled: true,
    });
    if (result.ok) {
        console.log(`${binName} ${version} installed to ${result.canonicalPath} -> ${result.versionedName}`);
        return true;
    }
    reportInstallFailure(path.basename(sourceDir), result);
    return false;
}

// Best-effort cleanup of old versioned binaries for a given binary name.
// Keeps the current version and the previous one (in case a process is still
// running the old binary and hasn't fully loaded all pages yet).
function cleanupOldVersions(binName) {
    lib.cleanupOldVersions({ canonicalDir: CANONICAL_DIR, binName, version, isWindows: IS_WINDOWS });
}

const platformDir = resolvePlatformPackageDir();
if (!platformDir) {
    console.error(`chaos-code: platform package chaos-code-${key} not installed.`);
    console.error('  This usually means npm was invoked with --no-optional, or the install failed.');
    console.error('  Try: npm install -g chaos-code');
    process.exit(0);
}

// Point the bin entry at a binary extracted beside it: launches become one
// process, and the link can only dangle if the package itself is broken.
// Windows keeps the node launcher; npm generates its command shims from it.
function installBinLink(binSourceDir) {
    if (IS_WINDOWS) return;
    // Other package managers wrap the entry's `#!` line in their own launchers.
    if (!(process.env.npm_config_user_agent ?? '').startsWith('npm/')) return;
    const nativePath = path.join(__dirname, 'chaos-native');
    const entryPath = path.join(__dirname, 'chaos');
    const tmp = entryPath + `.link.${process.pid}`;
    const written = lib.writeVerifiedBinary(binSourceDir, `chaos${EXE}`, nativePath, { isWindows: IS_WINDOWS });
    if (!written.ok) {
        // The versioned install above reports the same failure with the full message; this link
        // is only a latency optimisation, so it stays quiet here.
        return;
    }
    try {
        try { fs.unlinkSync(entryPath); } catch {}
        fs.symlinkSync('./chaos-native', tmp);
        fs.renameSync(tmp, entryPath);
    } catch (e) {
        // Losing the link only costs latency; the node launcher still works.
        console.error(`chaos-code: bin link not installed: ${e.message}`);
        try { fs.unlinkSync(tmp); } catch {}
    }
}

if (!installBinary('chaos', platformDir)) {
    process.exit(1);
}
installBinLink(platformDir);
cleanupOldVersions('chaos');
// Legacy upstream installs may still carry these names.
cleanupOldVersions('grok');
cleanupOldVersions('chaos-pager');

// Write installer config
const configDir = GROK_HOME;
const configPath = path.join(configDir, 'config.toml');
let obj = {};
try { obj = TOML.parse(fs.readFileSync(configPath, 'utf8')); } catch { }
obj.cli ??= {};
obj.cli.installer = 'npm';

// Persist the npm registry so `grok update` and the launcher use the same one.
const npmRegistry = process.env.GROK_NPM_REGISTRY
    || (() => {
        try {
            const resolved = execSync(
                'npm config get chaos-code:registry',
                { encoding: 'utf8', timeout: 5000 }
            ).trim();
            if (resolved && resolved !== 'undefined') return resolved;
        } catch {}
        return null;
    })();

if (npmRegistry) {
    obj.cli.npm_registry = npmRegistry;
}

fs.writeFileSync(configPath, TOML.stringify(obj), 'utf8');

// Shell completions: print setup hints (no silent shell config mutation).
// Set GROK_INSTALL_COMPLETIONS=1 to auto-generate completions.
const GROK_PATH = path.join(CANONICAL_DIR, `chaos${EXE}`);
if (process.env.GROK_INSTALL_COMPLETIONS === '1' && !IS_WINDOWS) {
    try {
        const { spawnSync } = require('child_process');
        const completionsDir = path.join(GROK_HOME, 'completions');
        const bashPath = path.join(completionsDir, 'bash', 'chaos.bash');
        const zshPath = path.join(completionsDir, 'zsh', '_chaos');
        fs.mkdirSync(path.dirname(bashPath), { recursive: true });
        fs.mkdirSync(path.dirname(zshPath), { recursive: true });
        const bashRes = spawnSync(GROK_PATH, ['completions', 'bash'], { encoding: 'utf8' });
        if (bashRes.status === 0) fs.writeFileSync(bashPath, bashRes.stdout);
        const zshRes = spawnSync(GROK_PATH, ['completions', 'zsh'], { encoding: 'utf8' });
        if (zshRes.status === 0) fs.writeFileSync(zshPath, zshRes.stdout);
        console.log(`Completions generated to ${GROK_HOME}/completions (bash/zsh)`);
    } catch {}
} else if (!IS_WINDOWS) {
    console.log('Tip: chaos completions bash > ~/.local/share/bash-completion/completions/chaos');
    console.log('     chaos completions zsh  > ~/.zsh/completions/_chaos');
}
