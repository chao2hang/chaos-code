#!/usr/bin/env node
// Stamp chaos-code meta + platform package.json versions (and optionalDeps pins).
//
// Usage:
//   node scripts/ci/stamp-npm-version.mjs 0.2.110
//   VERSION=0.2.110 node scripts/ci/stamp-npm-version.mjs
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, '..', '..');
const npmRoot = path.join(repoRoot, 'crates/codegen/xai-grok-pager/npm');

const version = (process.argv[2] || process.env.VERSION || '').replace(/^v/, '');
if (!version || !/^\d+\.\d+\.\d+([.-].+)?$/.test(version)) {
    console.error('usage: stamp-npm-version.mjs <semver>');
    process.exit(1);
}

const platforms = [
    'darwin-arm64',
    'darwin-x64',
    'linux-arm64',
    'linux-x64',
    'win32-arm64',
    'win32-x64',
];

const metaPath = path.join(npmRoot, 'chaos', 'package.json');
const meta = JSON.parse(fs.readFileSync(metaPath, 'utf8'));
meta.version = version;
meta.optionalDependencies = Object.fromEntries(
    platforms.map((p) => [`chaos-code-${p}`, version]),
);
fs.writeFileSync(metaPath, JSON.stringify(meta, null, 4) + '\n');
console.log(`stamped ${path.relative(repoRoot, metaPath)} -> ${version}`);

for (const p of platforms) {
    const pkgPath = path.join(npmRoot, `chaos-${p}`, 'package.json');
    if (!fs.existsSync(pkgPath)) {
        console.error(`missing ${pkgPath}`);
        process.exit(1);
    }
    const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
    pkg.version = version;
    pkg.name = `chaos-code-${p}`;
    fs.writeFileSync(pkgPath, JSON.stringify(pkg, null, 4) + '\n');
    console.log(`stamped ${path.relative(repoRoot, pkgPath)} -> ${version}`);
}
