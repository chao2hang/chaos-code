#!/usr/bin/env bash
# Regression tests for npm publication guards. Uses fake npm and temporary bin files;
# never contacts the registry or publishes packages.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
NPM_SCRIPT="$ROOT/scripts/ci/publish-npm.sh"
TMP="$(mktemp -d)"
NPM_ROOT="$TMP/npm"
mkdir -p "$NPM_ROOT/chaos/bin"
cat > "$NPM_ROOT/chaos/package.json" <<'JSON'
{"name":"chaos-code","version":"9.9.9"}
JSON
printf 'placeholder notices for the meta package\n' > "$NPM_ROOT/chaos/THIRD_PARTY_NOTICES.md"
for p in chaos-darwin-arm64 chaos-darwin-x64 chaos-linux-arm64 chaos-linux-x64 chaos-win32-arm64 chaos-win32-x64; do
  mkdir -p "$NPM_ROOT/$p/bin"
  # The name the meta package pins, not the directory name: a mismatch here is a package nobody
  # can install, and the integrity guard is what notices before the registry does.
  cat > "$NPM_ROOT/$p/package.json" <<JSON
{"name":"chaos-code-${p#chaos-}","version":"9.9.9"}
JSON
  # The notices document is an input to publication, not just to the assembler: the
  # placeholder is written here so the assertions below fail for the reason they claim.
  printf 'placeholder notices for %s\n' "$p" > "$NPM_ROOT/$p/THIRD_PARTY_NOTICES.md"
done
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/bin"

# A package is only publishable if its bytes hash to what `bin/integrity.json` claims, so the
# fixtures below cannot be a `printf` of dummy text any more. The archive and its record are
# written here by the shipped assembler's own record builder, which keeps the producer of the
# claim and the checker of the claim in different pieces of code where they belong.
write_fake_binary() { # write_fake_binary <pkg-dir> <bin-name> <marker>
  ASM_NPM="$NPM_ROOT/chaos" \
  ASM_MOD="$ROOT/crates/codegen/xai-grok-pager/npm/chaos/scripts/assemble-platform-packages.js" \
  node -e '
const fs = require("fs"), path = require("path"), zlib = require("zlib");
const { buildIntegrityRecord, INTEGRITY_FILE } = require(process.env.ASM_MOD);
const [dir, binName, marker] = process.argv.slice(1);
const raw = Buffer.from(`#!/bin/sh\nprintf "${marker}\\n"\n`);
const compressed = zlib.brotliCompressSync(raw);
const target = path.basename(dir).replace(/^chaos-/, "");
const cut = target.lastIndexOf("-");
const version = JSON.parse(fs.readFileSync(path.join(process.env.ASM_NPM, "package.json"), "utf8")).version;
const record = buildIntegrityRecord({
    platform: target.slice(0, cut), arch: target.slice(cut + 1), binName, version, raw, compressed,
});
fs.mkdirSync(path.join(dir, "bin"), { recursive: true });
fs.writeFileSync(path.join(dir, "bin", binName + ".br"), compressed);
fs.writeFileSync(path.join(dir, "bin", INTEGRITY_FILE), JSON.stringify(record, null, 2) + "\n");
' "$1" "$2" "$3"
}

cat > "$TMP/bin/npm" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*|$PWD" >> "$NPM_CALLS"
case " $* " in
  *" pack "*) exit 0 ;;
  *" publish "*) exit 0 ;;
  *" view "*) printf '"%s"\n' "$(node -p 'require(process.argv[1]).version' "$PWD/package.json")" ;;
esac
exit 0
SH
chmod +x "$TMP/bin/npm"
export PATH="$TMP/bin:$PATH" NPM_CALLS="$TMP/npm-calls.log" DRY_RUN=1 NPM_ROOT
: > "$NPM_CALLS"

# No assembled packages must fail before contacting npm.
if PUBLISH_EXISTING_ONLY=1 bash "$NPM_SCRIPT" >"$TMP/no-bins.out" 2>&1; then
  echo "expected the script to reject missing bins" >&2
  exit 1
fi
if [[ -s "$NPM_CALLS" ]]; then
  echo "npm must not be invoked when no platform bins exist" >&2
  exit 1
fi

# A dry run without the explicit EXISTING_ONLY/partial mode must refuse to pretend missing platforms are okay.
if bash "$NPM_SCRIPT" >"$TMP/default-dry-run.out" 2>&1; then
  echo "expected the default dry run to reject missing platform packages" >&2
  exit 1
fi
# A dry run that tolerates nothing missing is a run that means "all six or nothing", and the guard
# is run with `--require-assembled` exactly in that mode. Asserting the guard's own message is what
# notices the flag stopping being passed: without it the run still fails two lines later for the
# per-package reason, so a mode that grew the same tolerance in some future edit would reach the
# registry with packages whose bytes nobody checked.
if PUBLISH_NPM_ALLOW_PARTIAL=1 bash "$NPM_SCRIPT" >"$TMP/require-assembled.out" 2>&1; then
  echo "expected the assembled-required run to reject a tree with no binaries" >&2
  exit 1
fi
if ! grep -q 'nothing assembled for chaos-darwin-arm64' "$TMP/require-assembled.out"; then
  echo "expected the assembled-required run to refuse through the digest guard" >&2
  cat "$TMP/require-assembled.out" >&2
  exit 1
fi
if [[ -s "$NPM_CALLS" ]]; then
  echo "npm must not be asked to pack anything when nothing is assembled" >&2
  cat "$NPM_CALLS" >&2
  exit 1
fi

# Tracked .gitkeep placeholders do not count as assembled package content.
if PUBLISH_EXISTING_ONLY=1 bash "$NPM_SCRIPT" >"$TMP/placeholders.out" 2>&1; then
  echo "expected .gitkeep placeholders to be rejected" >&2
  exit 1
fi

# Five assembled platforms are insufficient: the meta package pins all six. The Windows package
# carries `chaos.exe`, which is the name the launcher looks for there and the name the record has
# to name.
for p in chaos-darwin-arm64 chaos-darwin-x64 chaos-linux-arm64 chaos-linux-x64; do
  write_fake_binary "$NPM_ROOT/$p" chaos "fake-$p"
done
write_fake_binary "$NPM_ROOT/chaos-win32-arm64" chaos.exe "fake-chaos-win32-arm64"
if PUBLISH_EXISTING_ONLY=1 bash "$NPM_SCRIPT" >"$TMP/partial.out" 2>&1; then
  echo "expected the script to reject a partial platform set" >&2
  exit 1
fi
if grep -Fx "$NPM_ROOT/chaos" "$NPM_CALLS" >/dev/null 2>&1; then
  echo "meta package must not be packed when a platform is missing" >&2
  cat "$NPM_CALLS" >&2
  exit 1
fi

# Explicit partial mode may publish present platforms, but must not publish the meta package.
: > "$NPM_CALLS"
PUBLISH_NPM_ALLOW_PARTIAL=1 PUBLISH_EXISTING_ONLY=1 bash "$NPM_SCRIPT" >"$TMP/partial-opt-in.out" 2>&1
if [[ "$(grep -c '^pack --dry-run|' "$NPM_CALLS")" -ne 5 ]]; then
  echo "expected only the five available platform packages in partial mode" >&2
  cat "$NPM_CALLS" >&2
  exit 1
fi
if grep -Fx "pack --dry-run|$NPM_ROOT/chaos" "$NPM_CALLS" >/dev/null 2>&1; then
  echo "partial mode must never pack the meta package" >&2
  exit 1
fi

# Six platforms allow the dry-run path to pack every platform and the meta package.
write_fake_binary "$NPM_ROOT/chaos-win32-x64" chaos.exe "fake-chaos-win32-x64"
: > "$NPM_CALLS"
PUBLISH_EXISTING_ONLY=1 bash "$NPM_SCRIPT" >"$TMP/complete.out" 2>&1
if [[ "$(grep -c '^pack --dry-run|' "$NPM_CALLS")" -ne 7 ]]; then
  echo "expected six platform packs plus the meta package" >&2
  cat "$NPM_CALLS" >&2
  exit 1
fi

# A platform package whose assembled notices are missing (or empty) must be refused, and
# npm must not be asked to pack it: the notices in the tarball are the attribution itself.
rm -f "$NPM_ROOT/chaos-win32-x64/THIRD_PARTY_NOTICES.md"
: > "$NPM_CALLS"
if PUBLISH_EXISTING_ONLY=1 bash "$NPM_SCRIPT" >"$TMP/no-notices.out" 2>&1; then
  echo "expected the script to refuse a platform package without notices" >&2
  exit 1
fi
if ! grep -q 'chaos-win32-x64 has no non-empty THIRD_PARTY_NOTICES.md' "$TMP/no-notices.out"; then
  echo "expected the refusal to name the package missing its notices" >&2
  cat "$TMP/no-notices.out" >&2
  exit 1
fi
if grep -Fx "pack --dry-run|$NPM_ROOT/chaos-win32-x64" "$NPM_CALLS" >/dev/null 2>&1; then
  echo "npm must not pack a package that has no notices" >&2
  cat "$NPM_CALLS" >&2
  exit 1
fi
if [[ "$(grep -c '^pack --dry-run|' "$NPM_CALLS")" -ne 5 ]]; then
  echo "expected the five packages that do have notices to be packed before the refusal" >&2
  cat "$NPM_CALLS" >&2
  exit 1
fi
printf 'placeholder notices for chaos-win32-x64\n' > "$NPM_ROOT/chaos-win32-x64/THIRD_PARTY_NOTICES.md"

# An empty notices file is as silent as an absent one, and the meta package is checked too.
: > "$NPM_ROOT/chaos-linux-x64/THIRD_PARTY_NOTICES.md"
: > "$NPM_ROOT/chaos/THIRD_PARTY_NOTICES.md"
: > "$NPM_CALLS"
if PUBLISH_EXISTING_ONLY=1 bash "$NPM_SCRIPT" >"$TMP/empty-notices.out" 2>&1; then
  echo "expected the script to refuse a zero-length notices file" >&2
  exit 1
fi
if ! grep -q 'chaos-linux-x64 has no non-empty THIRD_PARTY_NOTICES.md' "$TMP/empty-notices.out"; then
  echo "expected a zero-length notices file to be refused" >&2
  cat "$TMP/empty-notices.out" >&2
  exit 1
fi
printf 'placeholder notices for chaos-linux-x64\n' > "$NPM_ROOT/chaos-linux-x64/THIRD_PARTY_NOTICES.md"
: > "$NPM_CALLS"
if PUBLISH_EXISTING_ONLY=1 bash "$NPM_SCRIPT" >"$TMP/meta-no-notices.out" 2>&1; then
  echo "expected the script to refuse the meta package without notices" >&2
  exit 1
fi
if ! grep -q '^error: chaos has no non-empty THIRD_PARTY_NOTICES.md' "$TMP/meta-no-notices.out"; then
  echo "expected the meta package refusal to name the meta package" >&2
  cat "$TMP/meta-no-notices.out" >&2
  exit 1
fi
if grep -Fx "pack --dry-run|$NPM_ROOT/chaos" "$NPM_CALLS" >/dev/null 2>&1; then
  echo "the meta package must not be packed without its notices" >&2
  cat "$NPM_CALLS" >&2
  exit 1
fi
if [[ "$(grep -c '^pack --dry-run|' "$NPM_CALLS")" -ne 6 ]]; then
  echo "expected all six platform packs before the meta package was refused" >&2
  cat "$NPM_CALLS" >&2
  exit 1
fi
printf 'placeholder notices for the meta package\n' > "$NPM_ROOT/chaos/THIRD_PARTY_NOTICES.md"

# The complete set publishes again once every notices file is back, which is what separates
# the refusals above from a guard that simply always fails.
: > "$NPM_CALLS"
PUBLISH_EXISTING_ONLY=1 bash "$NPM_SCRIPT" >"$TMP/restored.out" 2>&1
if [[ "$(grep -c '^pack --dry-run|' "$NPM_CALLS")" -ne 7 ]]; then
  echo "expected the restored set to pack six platforms plus the meta package" >&2
  cat "$NPM_CALLS" >&2
  exit 1
fi

# The digest record is checked before anything reaches npm, in both of the ways it can go wrong:
# the claim is gone, or the claim and the bytes disagree. Either way the package that installs
# "fine" and then refuses to start is exactly what the check exists to keep off the registry.
rm "$NPM_ROOT/chaos-linux-x64/bin/integrity.json"
: > "$NPM_CALLS"
if PUBLISH_EXISTING_ONLY=1 bash "$NPM_SCRIPT" >"$TMP/no-integrity.out" 2>&1; then
  echo "expected the script to refuse a platform package with no integrity record" >&2
  exit 1
fi
if ! grep -q 'integrity.json: missing' "$TMP/no-integrity.out"; then
  echo "expected the refusal to name the missing record" >&2
  cat "$TMP/no-integrity.out" >&2
  exit 1
fi
if [[ -s "$NPM_CALLS" ]]; then
  echo "npm must not be asked to pack anything when a record is missing" >&2
  cat "$NPM_CALLS" >&2
  exit 1
fi
write_fake_binary "$NPM_ROOT/chaos-linux-x64" chaos "fake-chaos-linux-x64"

python3 - "$NPM_ROOT/chaos-darwin-x64/bin/chaos.br" <<'PY'
import sys
path = sys.argv[1]
data = bytearray(open(path, "rb").read())
data[len(data) // 2] ^= 0x01
open(path, "wb").write(bytes(data))
PY
: > "$NPM_CALLS"
if PUBLISH_EXISTING_ONLY=1 bash "$NPM_SCRIPT" >"$TMP/tampered.out" 2>&1; then
  echo "expected the script to refuse an archive that no longer matches its record" >&2
  exit 1
fi
if ! grep -q 'chaos.br: sha256 is' "$TMP/tampered.out"; then
  echo "expected the refusal to report the digest that changed" >&2
  cat "$TMP/no-integrity.out" >&2
  exit 1
fi
if [[ -s "$NPM_CALLS" ]]; then
  echo "npm must not be asked to pack a tampered package" >&2
  cat "$NPM_CALLS" >&2
  exit 1
fi
write_fake_binary "$NPM_ROOT/chaos-darwin-x64" chaos "fake-chaos-darwin-x64"

# With CHAOS_SHA256SUMS set, the release's own digest list is the answer, not the package's.
printf '%s  chaos-linux-x64\n' "$(printf '0%.0s' {1..64})" > "$TMP/SHA256SUMS"
: > "$NPM_CALLS"
if PUBLISH_EXISTING_ONLY=1 CHAOS_SHA256SUMS="$TMP/SHA256SUMS" bash "$NPM_SCRIPT" >"$TMP/sums-drift.out" 2>&1; then
  echo "expected the script to refuse a package whose digest SHA256SUMS disagrees with" >&2
  exit 1
fi
if ! grep -q 'records chaos-linux-x64 as' "$TMP/sums-drift.out"; then
  echo "expected the refusal to quote SHA256SUMS" >&2
  cat "$TMP/sums-drift.out" >&2
  exit 1
fi
if [[ -s "$NPM_CALLS" ]]; then
  echo "npm must not be asked to pack a package SHA256SUMS disagrees with" >&2
  cat "$NPM_CALLS" >&2
  exit 1
fi
rm -f "$TMP/SHA256SUMS"

# A package directory whose manifest carries a name the meta package does not pin would publish a
# tarball that no install can ever resolve.
python3 - "$NPM_ROOT/chaos-linux-arm64/package.json" <<'PY'
import json, sys
path = sys.argv[1]
manifest = json.load(open(path))
manifest["name"] = "chaos-linux-arm64"
json.dump(manifest, open(path, "w"))
PY
: > "$NPM_CALLS"
if PUBLISH_EXISTING_ONLY=1 bash "$NPM_SCRIPT" >"$TMP/wrong-name.out" 2>&1; then
  echo "expected the script to refuse a package named differently from the meta pin" >&2
  exit 1
fi
if ! grep -q "is not the name the meta package 'chaos-code' pins" "$TMP/wrong-name.out"; then
  echo "expected the refusal to name the pin that does not match" >&2
  cat "$TMP/wrong-name.out" >&2
  exit 1
fi
cat > "$NPM_ROOT/chaos-linux-arm64/package.json" <<'JSON'
{"name":"chaos-code-linux-arm64","version":"9.9.9"}
JSON

# With every record back the same tree publishes, which is what separates the two refusals above
# from a check that simply always fails.
: > "$NPM_CALLS"
PUBLISH_EXISTING_ONLY=1 bash "$NPM_SCRIPT" >"$TMP/integrity-restored.out" 2>&1
if [[ "$(grep -c '^pack --dry-run|' "$NPM_CALLS")" -ne 7 ]]; then
  echo "expected the tree with its records restored to pack six platforms plus the meta package" >&2
  cat "$NPM_CALLS" >&2
  exit 1
fi

# The local host publisher refuses to publish from one host without explicit partial mode.
if PUBLISH_NPM_ALLOW_PARTIAL=0 env -u CHAOS_DARWIN_ARM64 -u CHAOS_DARWIN_X64 -u CHAOS_LINUX_ARM64 -u CHAOS_LINUX_X64 -u CHAOS_WIN32_ARM64 -u CHAOS_WIN32_X64 bash "$ROOT/scripts/ci/local-publish-host.sh" --publish >"$TMP/host-missing.out" 2>&1; then
  echo "expected local publisher to require all six binary inputs by default" >&2
  exit 1
fi

# A registry rejection after npm publish must propagate as failure (no fake green).
cat > "$TMP/bin/npm" <<'SH'
#!/usr/bin/env bash
case " $* " in
  *" publish "*) exit 0 ;;
  *" view "*) echo "npm error E404" >&2; exit 1 ;;
esac
exit 0
SH
chmod +x "$TMP/bin/npm"
if DRY_RUN=0 NPM_TOKEN=test-token PUBLISH_NPM_ALLOW_PARTIAL=1 PUBLISH_EXISTING_ONLY=1 NPM_VERIFY_RETRIES=1 bash "$NPM_SCRIPT" >"$TMP/registry-failure.out" 2>&1; then
  echo "expected registry verification to fail after a false-positive publish" >&2
  exit 1
fi
if ! grep -q 'registry did not confirm' "$TMP/registry-failure.out"; then
  echo "expected failure to come from read-after-write registry verification" >&2
  cat "$TMP/registry-failure.out" >&2
  exit 1
fi

echo "publish-npm guards: OK (empty, partial and complete platform sets, and the digest refusals)"
