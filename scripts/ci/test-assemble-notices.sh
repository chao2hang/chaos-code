#!/usr/bin/env bash
# Regression tests for the third-party notices document that travels inside every npm
# package published from this repository.
#
# What is under test is the shipped builder in
# `crates/codegen/xai-grok-pager/npm/chaos/scripts/assemble-platform-packages.js`. The script
# is copied into a scratch repository shape so the module's own path resolution is what
# decides where files land, and the assertions are made against the files it actually wrote.
# No binary is assembled, nothing is published, and the working tree is never written to.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
NPM_SRC="$ROOT/crates/codegen/xai-grok-pager/npm"
ASSEMBLER="$NPM_SRC/chaos/scripts/assemble-platform-packages.js"
DEPENDENCY_NOTICES="$ROOT/THIRD-PARTY-NOTICES"
PORTED_NOTICES="$ROOT/crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

REPO="$TMP/repo"
NPM_DIR="$REPO/crates/codegen/xai-grok-pager/npm"
mkdir -p "$REPO/crates/codegen/xai-grok-tools" "$NPM_DIR/chaos/scripts"
cp "$ASSEMBLER" "$NPM_DIR/chaos/scripts/assemble-platform-packages.js"
cat > "$NPM_DIR/chaos/package.json" <<'JSON'
{"name":"chaos-code","version":"9.9.9"}
JSON
# The CLI path needs a per-platform manifest for the one target it is asked to assemble.
mkdir -p "$NPM_DIR/chaos-linux-x64"
cat > "$NPM_DIR/chaos-linux-x64/package.json" <<'JSON'
{"name":"chaos-code-linux-x64","version":"0.0.0","files":["bin/","THIRD_PARTY_NOTICES.md"]}
JSON
cat > "$REPO/THIRD-PARTY-NOTICES" <<'DOC'
================================================================================
PART I — PER-PACKAGE ENTRIES

--------------------------------------------------------------------------------
synthetic-crate 1.2.3
--------------------------------------------------------------------------------
  Source:  https://example.invalid/synthetic-crate
  License: MIT
DOC
cat > "$REPO/crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md" <<'DOC'
# Ported source code notices

Ported from example/upstream and modified.
DOC
for t in darwin-arm64 darwin-x64 linux-x64 linux-arm64 win32-x64 win32-arm64; do
  mkdir -p "$NPM_DIR/chaos-$t"
  printf '{"name":"chaos-%s","version":"0.0.0","files":["bin/","THIRD_PARTY_NOTICES.md"]}\n' \
    "$t" > "$NPM_DIR/chaos-$t/package.json"
done

# --- 1. the bundle the shipped builder writes -------------------------------------------
cat > "$TMP/driver.js" <<'JS'
const fs = require('fs');
const path = require('path');

const [modulePath, npmDir] = process.argv.slice(2);
const asm = require(modulePath);
const fail = (msg) => { throw new Error(`assemble-notices: ${msg}`); };
const eq = (actual, expected, what) => {
    if (actual !== expected) { fail(`${what}: got ${JSON.stringify(actual)}, want ${JSON.stringify(expected)}`); }
};

const targets = [
    { platform: 'darwin', arch: 'arm64' }, { platform: 'darwin', arch: 'x64' },
    { platform: 'linux', arch: 'x64' }, { platform: 'linux', arch: 'arm64' },
    { platform: 'win32', arch: 'x64' }, { platform: 'win32', arch: 'arm64' },
];
eq(asm.NOTICES_NAME, 'THIRD_PARTY_NOTICES.md', 'the file name inside the package');

// npmDir is <repo>/crates/codegen/xai-grok-pager/npm, so the repository root is four up.
const dependencyText = fs.readFileSync(
    path.resolve(npmDir, '..', '..', '..', '..', 'THIRD-PARTY-NOTICES'), 'utf8');
const portedText = fs.readFileSync(
    path.resolve(npmDir, '..', '..', 'xai-grok-tools', 'THIRD_PARTY_NOTICES.md'), 'utf8');

const bundle = asm.writeNoticesBundles(targets);

// Every publish target gets a copy: the meta package, the six platform packages, and the
// stable path the release workflow attaches to the GitHub Release.
const expected = [
    path.join(npmDir, asm.NOTICES_NAME),
    path.join(npmDir, 'chaos', asm.NOTICES_NAME),
    ...targets.map((t) => path.join(npmDir, `chaos-${t.platform}-${t.arch}`, asm.NOTICES_NAME)),
];
eq(expected.length, 8, 'number of destinations');
for (const file of expected) {
    if (!fs.existsSync(file)) { fail(`no file written at ${path.relative(npmDir, file)}`); }
    eq(fs.readFileSync(file, 'utf8'), bundle, `contents of ${path.relative(npmDir, file)}`);
}

// Both inputs survive the trip verbatim. A notice that was summarised on the way to the
// user is not the notice the author asked for.
if (!bundle.includes(dependencyText.trimEnd())) { fail('dependency notices are not reproduced verbatim'); }
if (!bundle.includes(portedText.trimEnd())) { fail('ported-code notices are not reproduced verbatim'); }
if (!bundle.includes('synthetic-crate 1.2.3')) { fail('a package entry did not survive into the bundle'); }
if (!bundle.includes('chaos-code 9.9.9')) { fail('the bundle does not name the package it ships inside'); }
if (bundle.indexOf('synthetic-crate 1.2.3') > bundle.indexOf('## Ported source code')) {
    fail('the dependency notices must precede the ported-code section');
}
console.log('assemble-notices: the shipped builder wrote the bundle to all 8 destinations');

// Refusing is the point: an assembled package without notices is installable and silent.
const refusals = [
    ['empty dependency notices', () => asm.buildNoticesBundle('', portedText)],
    ['whitespace-only dependency notices', () => asm.buildNoticesBundle('   \n', portedText)],
    ['empty ported notices', () => asm.buildNoticesBundle(dependencyText, '')],
    ['a file that is not the dependency document', () => asm.buildNoticesBundle('just prose\n', portedText)],
];
for (const [label, fn] of refusals) {
    let threw = false;
    try { fn(); } catch (err) {
        threw = true;
        if (!/^(\[assemble\] )/.test(err.message)) { fail(`${label}: unexpected message ${err.message}`); }
    }
    if (!threw) { fail(`${label} must be refused`); }
}
console.log(`assemble-notices: ${refusals.length} malformed inputs refused`);
JS
node "$TMP/driver.js" "$NPM_DIR/chaos/scripts/assemble-platform-packages.js" "$NPM_DIR"

# Requiring the module and calling the builder must not assemble anything: a platform
# directory that never received a binary stays free of a bin/ entry.
if compgen -G "$NPM_DIR/*/bin" > /dev/null; then
  echo "the notices builder must not create bin directories" >&2
  exit 1
fi

# --- 2. the assembled package really carries it -------------------------------------------
# `main()` is what CI runs, so the wiring is proved through the CLI, not by calling the
# exported function: delete the documents, run the assembler for one target with a fake
# binary, and check that they came back, for that target and for the untouched five.
find "$NPM_DIR" -name THIRD_PARTY_NOTICES.md -delete
printf 'not the real binary\n' > "$TMP/fake-chaos"
CHAOS_LINUX_X64="$TMP/fake-chaos" ONLY_PLATFORMS=linux-x64 \
  node "$NPM_DIR/chaos/scripts/assemble-platform-packages.js" > "$TMP/cli.out" 2>&1 \
  || { echo "assembler CLI run failed" >&2; cat "$TMP/cli.out" >&2; exit 1; }
grep -q 'assembled at version 9.9.9' "$TMP/cli.out" \
  || { echo "assembler CLI did not report an assembly" >&2; cat "$TMP/cli.out" >&2; exit 1; }
for dir in chaos chaos-linux-x64 chaos-darwin-arm64 chaos-darwin-x64 chaos-linux-arm64 \
  chaos-win32-x64 chaos-win32-arm64; do
  [[ -s "$NPM_DIR/$dir/THIRD_PARTY_NOTICES.md" ]] \
    || { echo "$dir got no notices from the assembler CLI" >&2; exit 1; }
done
[[ -s "$NPM_DIR/THIRD_PARTY_NOTICES.md" ]] \
  || { echo "the release-asset copy at npm/ was not written" >&2; exit 1; }
[[ -s "$NPM_DIR/chaos-linux-x64/bin/chaos.br" ]] \
  || { echo "the selected target was not assembled" >&2; exit 1; }
if [[ -e "$NPM_DIR/chaos-darwin-arm64/bin" ]]; then
  echo "unselected targets must not gain a bin directory" >&2
  exit 1
fi
grep -q 'synthetic-crate 1.2.3' "$NPM_DIR/chaos-linux-x64/THIRD_PARTY_NOTICES.md" \
  || { echo "the assembled notices lost the dependency entries" >&2; exit 1; }
grep -q 'Ported from example/upstream and modified.' "$NPM_DIR/chaos-linux-x64/THIRD_PARTY_NOTICES.md" \
  || { echo "the assembled notices dropped the ported-code section" >&2; exit 1; }
echo "assemble-notices: the assembler CLI wrote the bundle into every package directory"

# --- 2. the real inputs in this repository ------------------------------------------------
# The fixture above proves the shape; this proves the document the shipped release actually
# carries. Read-only: buildNoticesBundle takes the texts, so nothing here writes to the tree.
cat > "$TMP/real.js" <<'JS'
const fs = require('fs');
const asm = require(process.argv[2]);
const dependencyText = fs.readFileSync(process.argv[3], 'utf8');
const portedText = fs.readFileSync(process.argv[4], 'utf8');
const bundle = asm.buildNoticesBundle(dependencyText, portedText);
const fail = (msg) => { throw new Error(`assemble-notices: ${msg}`); };
if (!bundle.includes(dependencyText.trimEnd())) { fail('repository dependency notices not verbatim'); }
if (!bundle.includes(portedText.trimEnd())) { fail('repository ported notices not verbatim'); }
if (!/-{60,}\n[^\n]+ \d[^\s]*\n-{60,}/.test(bundle)) { fail('no per-package entry found in the real bundle'); }
process.stdout.write(`assemble-notices: real bundle is ${(bundle.length / 1024).toFixed(0)} KB\n`);
JS
node "$TMP/real.js" "$ASSEMBLER" "$DEPENDENCY_NOTICES" "$PORTED_NOTICES"

# --- 3. npm has to be told to include the file --------------------------------------------
# `files` is an allowlist: a notices document that no manifest lists never reaches the
# tarball no matter how correctly it is assembled.
cat > "$TMP/manifests.js" <<'JS'
const fs = require('fs');
const path = require('path');
const asm = require(process.argv[2]);
const npmRoot = process.argv[3];
const fail = (msg) => { throw new Error(`assemble-notices: ${msg}`); };
const dirs = ['chaos'].concat(fs.readdirSync(npmRoot)
    .filter((name) => /^chaos-[a-z0-9]+-[a-z0-9]+$/.test(name))
    .sort());
if (dirs.length !== 7) { fail(`expected the meta package plus six platforms, saw ${dirs.join(', ')}`); }
for (const dir of dirs) {
    const manifest = JSON.parse(fs.readFileSync(path.join(npmRoot, dir, 'package.json'), 'utf8'));
    if (!Array.isArray(manifest.files) || !manifest.files.includes(asm.NOTICES_NAME)) {
        fail(`${dir}/package.json does not list ${asm.NOTICES_NAME} in "files"`);
    }
}
console.log(`assemble-notices: all ${dirs.length} manifests include ${asm.NOTICES_NAME}`);
JS
node "$TMP/manifests.js" "$ASSEMBLER" "$NPM_SRC"

# --- 4. npm's own view of the tarball -----------------------------------------------------
# Structure above proves intent; where the real npm is installed, ask it what it would ship.
if command -v npm >/dev/null 2>&1; then
  PKG="$TMP/pack/chaos-linux-x64"
  mkdir -p "$PKG/bin"
  cp "$NPM_SRC/chaos-linux-x64/package.json" "$PKG/package.json"
  printf 'not the real binary\n' > "$PKG/bin/chaos.br"
  cp "$NPM_DIR/chaos-linux-x64/THIRD_PARTY_NOTICES.md" "$PKG/THIRD_PARTY_NOTICES.md"
  if ! (cd "$PKG" && npm pack --dry-run --json 2>/dev/null) \
      | node -e '
const chunks = [];
process.stdin.on("data", (c) => chunks.push(c));
process.stdin.on("end", () => {
    const [pkg] = JSON.parse(Buffer.concat(chunks).toString());
    const names = pkg.files.map((f) => f.path);
    if (!names.includes("THIRD_PARTY_NOTICES.md")) {
        throw new Error(`assemble-notices: npm would ship ${names.join(", ")} without the notices`);
    }
    console.log(`assemble-notices: npm pack would ship ${names.sort().join(", ")}`);
});
'
  then
    echo "npm pack --dry-run did not include the notices document" >&2
    exit 1
  fi
else
  echo "assemble-notices: npm not installed here; manifest allowlist above is the only proof" >&2
fi

echo "assemble-notices guards: OK (bundle shape, refusals, real inputs, manifest allowlist)"
