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
  cat > "$NPM_ROOT/$p/package.json" <<JSON
{"name":"$p","version":"9.9.9"}
JSON
  # The notices document is an input to publication, not just to the assembler: the
  # placeholder is written here so the assertions below fail for the reason they claim.
  printf 'placeholder notices for %s\n' "$p" > "$NPM_ROOT/$p/THIRD_PARTY_NOTICES.md"
done
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/bin"
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

# Tracked .gitkeep placeholders do not count as assembled package content.
if PUBLISH_EXISTING_ONLY=1 bash "$NPM_SCRIPT" >"$TMP/placeholders.out" 2>&1; then
  echo "expected .gitkeep placeholders to be rejected" >&2
  exit 1
fi

# Five assembled platforms are insufficient: the meta package pins all six.
for p in chaos-darwin-arm64 chaos-darwin-x64 chaos-linux-arm64 chaos-linux-x64 chaos-win32-arm64; do
  mkdir -p "$NPM_ROOT/$p/bin"
  printf 'test archive\n' > "$NPM_ROOT/$p/bin/chaos.br"
done
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
mkdir -p "$NPM_ROOT/chaos-win32-x64/bin"
printf 'test archive\n' > "$NPM_ROOT/chaos-win32-x64/bin/chaos.exe.br"
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

echo "publish-npm guards: OK (empty, partial and complete platform sets)"
