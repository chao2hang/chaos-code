#!/usr/bin/env bash
# Regression tests for the digests that ride inside each published npm platform package.
#
# What is under test is shipped code, end to end: the assembler that writes
# `bin/integrity.json`, the guard that reads it from outside the install
# (`scripts/ci/check-npm-integrity.py`), the installer that refuses bytes it cannot account for
# (`bin/postinstall.js` through `bin/install-lib.js`), the launcher that runs them
# (`bin/chaos` -> `bin/chaos-bootstrap.js`), and `publish-npm.sh`, the funnel every publish goes
# through. The assembler and the installer are copied into a scratch repository shape so their own
# path resolution decides where files land; the working tree is never written to, nothing is
# published, and no registry is contacted.
#
# The fake binaries are POSIX shell scripts that print their argv, so "the launcher ran the bytes
# the package claimed" is settled by what the child prints rather than by a digest the fixture
# computed for itself. `@iarna/toml`, the one third-party module `postinstall.js` loads, is stubbed
# by a recorder: what is under test here is which bytes reach the chaos home, not the TOML writer.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
NPM_SRC="$ROOT/crates/codegen/xai-grok-pager/npm"
GUARD="$ROOT/scripts/ci/check-npm-integrity.py"
PUBLISH="$ROOT/scripts/ci/publish-npm.sh"
RELEASE_WF="$ROOT/.github/workflows/release.yml"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

failures=0
failure() { printf 'not ok: %s\n' "$*" >&2; failures=$((failures + 1)); }
ok() { printf 'ok: %s\n' "$*"; }

REPO="$TMP/repo"
NPM_DIR="$REPO/crates/codegen/xai-grok-pager/npm"
CHAOS_DIR="$NPM_DIR/chaos"
mkdir -p "$REPO/crates/codegen/xai-grok-tools" "$CHAOS_DIR/scripts" "$CHAOS_DIR/bin"

cp "$NPM_SRC/chaos/scripts/assemble-platform-packages.js" "$CHAOS_DIR/scripts/"
cp "$NPM_SRC/chaos/bin/postinstall.js" "$NPM_SRC/chaos/bin/chaos-bootstrap.js" \
   "$NPM_SRC/chaos/bin/install-lib.js" "$CHAOS_DIR/bin/"
cp "$NPM_SRC/chaos/bin/chaos" "$CHAOS_DIR/bin/chaos"
chmod +x "$CHAOS_DIR/bin/chaos"

cat > "$CHAOS_DIR/package.json" <<'JSON'
{"name":"chaos-code","version":"9.9.9",
 "optionalDependencies":{"chaos-code-linux-x64":"9.9.9","chaos-code-linux-arm64":"9.9.9",
 "chaos-code-darwin-arm64":"9.9.9","chaos-code-darwin-x64":"9.9.9",
 "chaos-code-win32-x64":"9.9.9","chaos-code-win32-arm64":"9.9.9"}}
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

TARGETS=(darwin-arm64 darwin-x64 linux-x64 linux-arm64 win32-x64 win32-arm64)
for target in "${TARGETS[@]}"; do
  mkdir -p "$NPM_DIR/chaos-$target"
  printf '{"name":"chaos-code-%s","version":"0.0.0","files":["bin/","THIRD_PARTY_NOTICES.md"]}\n' \
    "$target" > "$NPM_DIR/chaos-$target/package.json"
done

# --- 1. the assembler records what it packed --------------------------------------------
BIN_SRC="$TMP/src"
mkdir -p "$BIN_SRC"
for target in "${TARGETS[@]}"; do
  # Each fake binary is a real executable printing its argv and a target marker, and each carries
  # distinct bytes so a digest cannot be accidentally shared between two targets.
  printf '#!/bin/sh\nprintf "chaos-fixture %s %%s\\n" "$*"\n' "$target" > "$BIN_SRC/$target"
  chmod +x "$BIN_SRC/$target"
done
CHAOS_DARWIN_ARM64="$BIN_SRC/darwin-arm64" CHAOS_DARWIN_X64="$BIN_SRC/darwin-x64" \
CHAOS_LINUX_X64="$BIN_SRC/linux-x64" CHAOS_LINUX_ARM64="$BIN_SRC/linux-arm64" \
CHAOS_WIN32_X64="$BIN_SRC/win32-x64" CHAOS_WIN32_ARM64="$BIN_SRC/win32-arm64" \
CHAOS_ROOT="$REPO" node "$CHAOS_DIR/scripts/assemble-platform-packages.js" > "$TMP/assemble.log" 2>&1 \
  || { cat "$TMP/assemble.log" >&2; failure "the assembler failed"; }

if [[ $failures -gt 0 ]]; then exit 1; fi

# The record has to describe the file that is actually in the package. Both digests are recomputed
# here with `sha256sum` and with node's brotli decoder, neither of which is the assembler's code.
python3 - "$NPM_DIR" "$BIN_SRC" <<'PY' || exit 1
import hashlib, json, subprocess, sys
from pathlib import Path

npm_dir, bin_src = Path(sys.argv[1]), Path(sys.argv[2])
targets = ["darwin-arm64", "darwin-x64", "linux-x64", "linux-arm64", "win32-x64", "win32-arm64"]
problems = []
for target in targets:
    pkg = npm_dir / f"chaos-{target}"
    bin_name = "chaos.exe" if target.startswith("win32-") else "chaos"
    path = pkg / "bin" / "integrity.json"
    if not path.is_file():
        problems.append(f"{path}: the assembler wrote no record")
        continue
    record = json.loads(path.read_text())
    raw = (bin_src / target).read_bytes()
    archive = (pkg / "bin" / f"{bin_name}.br").read_bytes()
    decompressed = subprocess.run(
        ["node", "-e", "const z=require('zlib');let b=[];process.stdin.on('data',d=>b.push(d));"
         "process.stdin.on('end',()=>process.stdout.write(z.brotliDecompressSync(Buffer.concat(b))))"],
        input=archive, capture_output=True, check=True).stdout
    binary, compressed = record["binary"], record["compressed"]
    if binary["sha256"] != hashlib.sha256(raw).hexdigest():
        problems.append(f"{path}: binary.sha256 does not describe {bin_src / target}")
    if binary["bytes"] != len(raw):
        problems.append(f"{path}: binary.bytes is {binary['bytes']}, the file is {len(raw)}")
    if compressed["sha256"] != hashlib.sha256(archive).hexdigest():
        problems.append(f"{path}: compressed.sha256 does not describe the archive on disk")
    if compressed["bytes"] != len(archive):
        problems.append(f"{path}: compressed.bytes is {compressed['bytes']}, the file is {len(archive)}")
    if decompressed != raw:
        problems.append(f"{path}: decompressing the archive does not reproduce the source binary")
    if record["schema"] != "chaos-npm-integrity/1":
        problems.append(f"{path}: unexpected schema {record['schema']!r}")
    if record["version"] != "9.9.9":
        problems.append(f"{path}: version is {record['version']!r}, the meta package is 9.9.9")
    if record["platform"] != target:
        problems.append(f"{path}: platform is {record['platform']!r}, assembled for {target}")
    if binary["name"] != bin_name or compressed["name"] != f"{bin_name}.br":
        problems.append(f"{path}: names {binary['name']!r}/{compressed['name']!r}, expected {bin_name}")
    if record["release_artifact"] != f"chaos-{target}":
        problems.append(f"{path}: release_artifact is {record['release_artifact']!r}")
for problem in problems:
    print(f"not ok: {problem}", file=sys.stderr)
sys.exit(1 if problems else 0)
PY
ok "the assembler records both digests for all six targets, and they describe the bytes on disk"

# --- 2. `release_artifact` names an artifact the release workflow really builds -----------
# The tie to the signed release rests on this name. If the matrix renames an artifact and the
# assembler keeps producing the old spelling, rule 6 of the guard stops matching and the release
# would go out with the cross-check quietly pointing at nothing.
wf_artifacts=$(grep -oE '^ +artifact: chaos-[a-z0-9-]+' "$RELEASE_WF" | awk '{print $2}' | sort -u)
record_artifacts=$(for target in "${TARGETS[@]}"; do
  python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["release_artifact"])' \
    "$NPM_DIR/chaos-$target/bin/integrity.json"
done | sort -u)
if [[ "$wf_artifacts" != "$record_artifacts" ]]; then
  failure "release.yml builds {$(echo "$wf_artifacts" | tr '\n' ' ')} but the assembler claims {$(echo "$record_artifacts" | tr '\n' ' ')}"
else
  ok "release_artifact matches the $(echo "$wf_artifacts" | wc -l) artifacts release.yml builds"
fi

# --- 3. the guard accepts the scratch tree, and refuses each way it can fail --------------
if python3 "$GUARD" --npm-root "$NPM_DIR" --require-assembled > "$TMP/guard.ok" 2>&1; then
  ok "guard: $(cat "$TMP/guard.ok")"
else
  cat "$TMP/guard.ok" >&2
  failure "the guard refused a tree the assembler just assembled correctly"
fi

sha256sums="$TMP/SHA256SUMS"
: > "$sha256sums"
for target in "${TARGETS[@]}"; do
  bin_name="chaos"; [[ "$target" == win32-* ]] && bin_name="chaos.exe"
  (cd "$BIN_SRC" && sha256sum "$target" | sed "s| $target\$| chaos-$target|") >> "$sha256sums"
done
if python3 "$GUARD" --npm-root "$NPM_DIR" --sha256sums "$sha256sums" > "$TMP/guard.sums" 2>&1; then
  ok "guard: $(cat "$TMP/guard.sums")"
else
  cat "$TMP/guard.sums" >&2
  failure "the guard refused a tree whose records match SHA256SUMS"
fi

tamper() { # tamper <file> -- flip one byte in the middle of a small file
  python3 -c 'import sys
p = sys.argv[1]
b = bytearray(open(p, "rb").read())
b[len(b) // 2] ^= 0x01
open(p, "wb").write(bytes(b))' "$1"
}

check_refused() { # check_refused <label> <expect> [args...]
  local label="$1" expect="$2"; shift 2
  if out=$(python3 "$GUARD" "$@" 2>&1); then
    failure "$label: the guard accepted it"
    printf '%s\n' "$out" >&2
  elif ! grep -qF -- "$expect" <<<"$out"; then
    failure "$label: refused without saying why (looking for: $expect)"
    printf '%s\n' "$out" >&2
  else
    ok "$label refused: $(head -1 <<<"$out")"
  fi
}

cp -a "$NPM_DIR" "$TMP/tamper"
tamper "$TMP/tamper/chaos-linux-x64/bin/chaos.br"
check_refused "guard, byte flipped inside an archive" "chaos.br: sha256 is" \
  --npm-root "$TMP/tamper"

cp -a "$NPM_DIR" "$TMP/norecord" && rm "$TMP/norecord/chaos-linux-x64/bin/integrity.json"
check_refused "guard, record deleted" "missing, so nothing in this package says" \
  --npm-root "$TMP/norecord"

cp -a "$NPM_DIR" "$TMP/othername"
python3 -c 'import json,sys
p = sys.argv[1]; r = json.load(open(p)); r["binary"]["name"] = "chaos"; json.dump(r, open(p, "w"))' \
  "$TMP/othername/chaos-win32-x64/bin/integrity.json"
check_refused "guard, record describes another binary" "binary.name is" --npm-root "$TMP/othername"

cp -a "$NPM_DIR" "$TMP/wrongversion"
python3 -c 'import json,sys
p = sys.argv[1]; r = json.load(open(p)); r["version"] = "9.9.8"; json.dump(r, open(p, "w"))' \
  "$TMP/wrongversion/chaos-linux-x64/bin/integrity.json"
check_refused "guard, record from another version" "the meta package publishes" \
  --npm-root "$TMP/wrongversion"

cp -a "$NPM_DIR" "$TMP/zerodigest"
python3 -c 'import json,sys
p = sys.argv[1]; r = json.load(open(p)); r["binary"]["sha256"] = "0"*64; json.dump(r, open(p, "w"))' \
  "$TMP/zerodigest/chaos-linux-x64/bin/integrity.json"
check_refused "guard, digest that no bytes hash to" "the record calls the binary 00000000" \
  --npm-root "$TMP/zerodigest"

cp -a "$NPM_DIR" "$TMP/staleplain"
printf 'a stale binary nobody re-recorded\n' > "$TMP/staleplain/chaos-linux-x64/bin/chaos"
check_refused "guard, stale plain binary beside the archive" "the launcher prefers the archive" \
  --npm-root "$TMP/staleplain"

cp -a "$NPM_DIR" "$TMP/nosum"
sha256sums_drift="$TMP/SHA256SUMS-drift"
sed '3s/^[0-9a-f]\{64\}/'"$(printf '0%.0s' {1..64})"'/' "$sha256sums" > "$sha256sums_drift"
check_refused "guard, SHA256SUMS disagrees with the record" "records chaos-linux-x64 as" \
  --npm-root "$NPM_DIR" --sha256sums "$sha256sums_drift"

grep -v ' chaos-linux-x64$' "$sha256sums" > "$TMP/SHA256SUMS-partial"
check_refused "guard, artifact missing from SHA256SUMS" "is not listed in" \
  --npm-root "$NPM_DIR" --sha256sums "$TMP/SHA256SUMS-partial"

if [[ $failures -gt 0 ]]; then exit 1; fi

# --- 4. install and launch, through the shipped entry points --------------------------------
# Everything above checks claims on disk. This drives the two programs a user actually runs:
# `bin/postinstall.js`, which npm invokes after extracting the tarball, and `bin/chaos`, which is
# what `chaos` on a PATH means. They are the files from the working tree, copied unchanged into a
# node_modules layout so `require.resolve('chaos-code-<target>/package.json')` finds the sibling
# package the way it does after an install.
HOST_KEY="$(node -p 'process.platform + "-" + process.arch')"
E2E="$TMP/e2e"
NM="$E2E/node_modules"
META_INST="$NM/chaos-code"
HOST_PKG="chaos-code-$HOST_KEY"
mkdir -p "$META_INST/bin" "$NM/@iarna/toml"
cp "$CHAOS_DIR/bin/"*.js "$CHAOS_DIR/bin/chaos" "$META_INST/bin/"
cp "$CHAOS_DIR/package.json" "$META_INST/"
cp -a "$NPM_DIR/chaos-$HOST_KEY" "$NM/$HOST_PKG"

# The one third-party module `postinstall.js` loads. What is under test is which bytes reach the
# chaos home, so the stub only records the object it was asked to serialise -- but it records it,
# because a refactor that lost the `cli.installer = "npm"` write would break `chaos update`.
cat > "$NM/@iarna/toml/package.json" <<'JSON'
{"name":"@iarna/toml","version":"0.0.0-fixture","main":"index.js"}
JSON
cat > "$NM/@iarna/toml/index.js" <<'JS'
const fs = require('fs');
function parse(text) {
    const cli = {};
    for (const line of String(text).split('\n')) {
        const kv = /^\s*([A-Za-z0-9_.-]+)\s*=\s*"?([^"]*)"?\s*$/.exec(line);
        if (kv) cli[kv[1]] = kv[2];
    }
    return Object.keys(cli).length ? { cli } : {};
}
function stringify(obj) {
    fs.appendFileSync(process.env.TOML_STUB_LOG, JSON.stringify(obj) + '\n');
    const cli = (obj && obj.cli) || {};
    return Object.entries(cli).map(([k, v]) => `${k} = "${v}"`).join('\n') + '\n';
}
module.exports = { parse, stringify };
JS

run_postinstall() { # run_postinstall <home> ; echoes stdout+stderr
  CHAOS_HOME="$1" TOML_STUB_LOG="$1/toml-stub.log" \
  GROK_NPM_REGISTRY="https://registry.invalid" \
  npm_config_user_agent="npm/10.9.0 node/npm@10.9.0" \
  node "$META_INST/bin/postinstall.js" 2>&1
}

HOME1="$E2E/home1"
mkdir -p "$HOME1"
if ! out="$(run_postinstall "$HOME1")"; then
  printf '%s\n' "$out" >&2
  failure "postinstall.js exited non-zero on a package it just passed the guard with"
else
  version="$(node -p 'require(process.argv[1]).version' "$META_INST/package.json")"
  installed="$HOME1/bin/chaos-$version"
  if cmp -s "$BIN_SRC/$HOST_KEY" "$installed"; then
    ok "postinstall wrote $HOME1/bin/chaos-$version byte-for-byte identical to the release binary"
  else
    failure "the installed binary is not the bytes the package's record describes"
  fi
  if [[ "$(readlink "$HOME1/bin/chaos")" == "chaos-$version" ]]; then
    ok "the unversioned name is a relative symlink at chaos-$version"
  else
    failure "bin/chaos is $(readlink "$HOME1/bin/chaos" 2>/dev/null || echo 'not a symlink'), not chaos-$version"
  fi
  if [[ -x "$META_INST/bin/chaos-native" ]] && cmp -s "$BIN_SRC/$HOST_KEY" "$META_INST/bin/chaos-native"; then
    ok "the bin entry points at a verified copy beside the package"
  else
    failure "bin/chaos-native was not written from the verified bytes"
  fi
  if grep -q '"installer":"npm"' "$HOME1/toml-stub.log" 2>/dev/null; then
    ok "postinstall still records cli.installer = npm for the updater"
  else
    failure "postinstall no longer writes the installer config: $(cat "$HOME1/toml-stub.log" 2>/dev/null)"
  fi
fi

# The bin entry a user actually runs. postinstall replaces `bin/chaos` with a link to the verified
# copy beside the package, so this is the hot path, and it is exercised as a program -- the shebang
# picks the interpreter, exactly as npm's generated shim does.
if [[ -L "$META_INST/bin/chaos" && "$(readlink "$META_INST/bin/chaos")" == "./chaos-native" ]]; then
  ok "postinstall leaves bin/chaos pointing at the verified chaos-native beside the package"
else
  failure "bin/chaos is $(readlink "$META_INST/bin/chaos" 2>/dev/null || echo 'not a symlink')"
fi
if entry_out="$("$META_INST/bin/chaos" entry-check 2>&1)" \
   && [[ "$entry_out" == "chaos-fixture $HOST_KEY entry-check" ]]; then
  ok "the package's own bin entry runs the digested bytes: $entry_out"
else
  failure "the bin entry did not run the installed binary (got: ${entry_out:-no output})"
fi

# Restore the node launcher in place. It is the fallback for every install that did not go through
# npm's shim (a global install without the postinstall link, another package manager that wraps the
# entry, a home mounted read-only), and it is the code that has to refuse bytes it cannot account
# for, so the assertions below drive it rather than the native link.
cp "$CHAOS_DIR/bin/chaos" "$META_INST/bin/chaos"
chmod +x "$META_INST/bin/chaos"
if launch_out="$(CHAOS_HOME="$HOME1" node "$META_INST/bin/chaos" launch-check 2>&1)" \
   && [[ "$launch_out" == "chaos-fixture $HOST_KEY launch-check" ]]; then
  ok "the launcher runs the installed binary with its arguments: $launch_out"
else
  failure "the launcher did not run the installed binary (got: ${launch_out:-no output})"
fi

# --- 5. the ways the bytes can stop being the bytes ------------------------------------------
# A truncated install is the accident that actually happens, and the launcher is the last place
# that can still see the package's own digests. The installed file is cut, the launcher is asked
# to run it, and the assertion is that the user gets the real binary anyway.
truncated="$HOME1/bin/chaos-$version"
if [[ -f "$truncated" ]]; then
  head -c 8 "$truncated" > "$truncated.cut" && mv "$truncated.cut" "$truncated"
  chmod +x "$truncated"
  if launch_out="$(CHAOS_HOME="$HOME1" node "$META_INST/bin/chaos" after-truncate 2>&1)" \
     && [[ "$launch_out" == *"re-installing from the package"* ]] \
     && [[ "$launch_out" == *"chaos-fixture $HOST_KEY after-truncate"* ]]; then
    ok "a truncated install is reported and repaired from the package before it runs"
  else
    failure "a truncated installed binary was launched anyway (got: ${launch_out:-no output})"
  fi
  if cmp -s "$BIN_SRC/$HOST_KEY" "$HOME1/bin/chaos-$version"; then
    ok "the repaired file is byte-for-byte the release binary again"
  else
    failure "the launcher's repair wrote something other than the package's bytes"
  fi
fi

# A self-update or an install.sh replace moves the canonical link to a version this package never
# shipped. The launcher has to run it anyway: the digests in the package describe this package's
# own binary, and refusing here would mean `chaos update` stops working until someone npm-installs.
skew_home="$E2E/home-skew"
mkdir -p "$skew_home/bin"
printf '#!/bin/sh\nprintf "chaos-selfupdate %%s\\n" "$*"\n' > "$skew_home/bin/chaos-8.8.8"
chmod +x "$skew_home/bin/chaos-8.8.8"
ln -s chaos-8.8.8 "$skew_home/bin/chaos"
if skew_out="$(CHAOS_HOME="$skew_home" node "$META_INST/bin/chaos" selfupdate 2>&1)" \
   && [[ "$skew_out" == "chaos-selfupdate selfupdate" ]]; then
  ok "a canonical link naming another version still runs (the self-update contract)"
else
  failure "the launcher refused a self-updated binary (got: ${skew_out:-no output})"
fi

# The package itself is the root of trust for the install. Tamper with it and neither entry point
# may produce a runnable binary: the installer has to fail the install, and the launcher has to
# refuse rather than decompress-and-hope.
cp -a "$E2E" "$E2E-bad"
META_BAD="$E2E-bad/node_modules/chaos-code"
python3 -c 'import sys
p = sys.argv[1]
b = bytearray(open(p, "rb").read())
b[len(b) // 2] ^= 0x01
open(p, "wb").write(bytes(b))' "$E2E-bad/node_modules/$HOST_PKG/bin/chaos$([[ "$HOST_KEY" == win32-* ]] && echo .exe).br"

HOME2="$E2E-bad/home2"
mkdir -p "$HOME2"
if out="$(CHAOS_HOME="$HOME2" TOML_STUB_LOG="$HOME2/toml-stub.log" GROK_NPM_REGISTRY=https://registry.invalid \
    node "$META_BAD/bin/postinstall.js" 2>&1)"; then
  failure "postinstall accepted a package whose archive no longer matches its record"
  printf '%s\n' "$out" >&2
elif ! grep -q '(compressed) does not match bin/integrity.json' <<<"$out"; then
  failure "postinstall refused without naming the digest: $(head -1 <<<"$out")"
elif [[ -e "$HOME2/bin/chaos" ]]; then
  failure "postinstall left a binary in the chaos home after refusing the bytes"
else
  ok "postinstall exits non-zero and writes nothing when the archive and the record disagree"
fi

HOME3="$E2E-bad/home3"
mkdir -p "$HOME3"
if out="$(CHAOS_HOME="$HOME3" node "$META_BAD/bin/chaos" should-not-run 2>&1)"; then
  failure "the launcher ran bytes its own package cannot account for"
  printf '%s\n' "$out" >&2
elif ! grep -q 'refusing to run' <<<"$out"; then
  failure "the launcher refused without saying why: $(head -1 <<<"$out")"
else
  ok "the launcher refuses to run when the package's archive does not match its record"
fi

# The archive being broken is only half of it. A failed install makes the launcher fall back to the
# plain file beside the archive, and the only check it then makes on that file is its length, so a
# wrong file of exactly the recorded length has to be stopped by the digest that failed rather than
# survive because the size happened to be right.
cp -a "$E2E" "$E2E-stale"
STALE_PKG="$E2E-stale/node_modules/$HOST_PKG"
STALE_META="$E2E-stale/node_modules/chaos-code"
python3 -c 'import json,sys
rec = json.load(open(sys.argv[1]))
n = rec["binary"]["bytes"]
body = b"#!/bin/sh\nprintf \"chaos-stale %s\\n\" \"$*\"\n"
data = (body + b"#" * (n - len(body)))[:n] if n >= len(body) else body[:n]
open(sys.argv[2], "wb").write(data)' \
    "$STALE_PKG/bin/integrity.json" "$STALE_PKG/bin/chaos"
chmod +x "$STALE_PKG/bin/chaos"
python3 -c 'import sys
p = sys.argv[1]
b = bytearray(open(p, "rb").read())
b[len(b) // 2] ^= 0x01
open(p, "wb").write(bytes(b))' "$STALE_PKG/bin/chaos$([[ "$HOST_KEY" == win32-* ]] && echo .exe).br"
HOME6="$E2E-stale/home6"
mkdir -p "$HOME6"
if stale_out="$(CHAOS_HOME="$HOME6" node "$STALE_META/bin/chaos" must-not-run 2>&1)"; then
  printf '%s\n' "$stale_out" >&2
  failure "the launcher ran the plain file beside a broken archive because its length matched"
elif grep -q 'chaos-stale' <<<"$stale_out"; then
  printf '%s\n' "$stale_out" >&2
  failure "the same-length stale file ran anyway"
elif ! grep -q 'refusing to run' <<<"$stale_out"; then
  printf '%s\n' "$stale_out" >&2
  failure "the launcher bailed here without naming the digest: $(head -1 <<<"$stale_out")"
else
  ok "a same-length file beside a broken archive is refused, not run on the strength of its size"
fi

# A record that has gone missing is the same position as a digest mismatch: nothing in the package
# says what the bytes should be, so the installer stops instead of shipping the user an install
# that can never be checked.
cp -a "$E2E" "$E2E-norec"
rm "$E2E-norec/node_modules/$HOST_PKG/bin/integrity.json"
HOME4="$E2E-norec/home4"
mkdir -p "$HOME4"
if out="$(CHAOS_HOME="$HOME4" TOML_STUB_LOG="$HOME4/toml-stub.log" GROK_NPM_REGISTRY=https://registry.invalid \
    node "$E2E-norec/node_modules/chaos-code/bin/postinstall.js" 2>&1)"; then
  failure "postinstall accepted a package with no integrity record"
  printf '%s\n' "$out" >&2
elif ! grep -q 'does not exist' <<<"$out"; then
  failure "postinstall refused a missing record without saying so: $(head -1 <<<"$out")"
else
  ok "postinstall refuses a package whose record was deleted"
fi

# The same missing record against a home that already holds the binary is the reinstall case, and
# it has to fail too: `verifyInstalled` is what makes an existing install count as checked, so
# without the record there is no evidence the file in place is the file that was released. The
# mirror image is the launcher's job -- it cannot check those bytes either, and refusing there
# would mean a packaging accident stops an already-working shell, so it hands off instead.
cp -a "$E2E" "$E2E-norec2"
rm "$E2E-norec2/node_modules/$HOST_PKG/bin/integrity.json"
NOREC2_PKG="$E2E-norec2/node_modules/chaos-code"
HOME5="$E2E-norec2/home5"
mkdir -p "$HOME5"
if ! out="$(CHAOS_HOME="$HOME5" TOML_STUB_LOG="$HOME5/toml-stub.log" GROK_NPM_REGISTRY=https://registry.invalid \
    npm_config_user_agent="npm/10.9.0 node/npm@10.9.0" \
    node "$META_INST/bin/postinstall.js" 2>&1)"; then
  printf '%s\n' "$out" >&2
  failure "postinstall refused a package that still had its record, for none of the reasons above"
fi
if out="$(CHAOS_HOME="$HOME5" TOML_STUB_LOG="$HOME5/toml-stub-re.log" GROK_NPM_REGISTRY=https://registry.invalid \
    npm_config_user_agent="npm/10.9.0 node/npm@10.9.0" \
    node "$NOREC2_PKG/bin/postinstall.js" 2>&1)"; then
  failure "postinstall accepted a reinstall it could not verify, reusing the installed binary"
  printf '%s\n' "$out" >&2
elif ! grep -q 'does not exist' <<<"$out"; then
  failure "postinstall refused the reinstall without naming the record: $(head -1 <<<"$out")"
elif ! cmp -s "$BIN_SRC/$HOST_KEY" "$HOME5/bin/chaos-$version"; then
  failure "the refused reinstall damaged the install that was already in place"
else
  ok "a reinstall over an existing install is refused once the record is gone"
fi
if norec2_out="$(CHAOS_HOME="$HOME5" node "$NOREC2_PKG/bin/chaos" missing-record 2>&1)" \
   && [[ "$norec2_out" == "chaos-fixture $HOST_KEY missing-record" ]]; then
  ok "an installed binary still runs when its package's record is gone (nothing left to check)"
else
  failure "the launcher would not run an installed binary whose record was deleted (got: ${norec2_out:-no output})"
fi

# --- 6. the Windows branch, on a machine that is not Windows ---------------------------------
# `swapCanonical` copies instead of symlinking there, and a copy over a running exe is the failure
# that branch exists to survive. The platform is overridden for a child that then loads the real
# `postinstall.js`, so the code under test is the shipped copy path, not a re-imagining of it.
E2W="$TMP/e2w"
mkdir -p "$E2W/home"
WIN_PKG="chaos-code-win32-x64"
if [[ "$HOST_KEY" == "win32-x64" ]]; then
  ok "Windows swap path: skipped, this host is win32-x64 and installs that way natively"
else
  cp -a "$NPM_DIR/chaos-win32-x64" "$NM/$WIN_PKG"
  wver="$(node -p 'require(process.argv[1]).version' "$META_INST/package.json")"
  if ! out="$(CHAOS_HOME="$E2W/home" TOML_STUB_LOG="$E2W/home/toml-stub.log" \
      GROK_NPM_REGISTRY=https://registry.invalid \
      node -e 'Object.defineProperty(process, "platform", { value: "win32" });
               Object.defineProperty(process, "arch", { value: "x64" });
               require(process.argv[1]);' "$META_INST/bin/postinstall.js" 2>&1)"; then
    printf '%s\n' "$out" >&2
    failure "the Windows install path failed on a package the guard accepted"
  else
    if cmp -s "$BIN_SRC/win32-x64" "$E2W/home/bin/chaos-$wver.exe"; then
      ok "the Windows install writes chaos-$wver.exe from the verified bytes"
    else
      failure "chaos-$wver.exe is not the bytes the win32-x64 record describes"
    fi
    if [[ -f "$E2W/home/bin/chaos.exe" && ! -L "$E2W/home/bin/chaos.exe" ]]; then
      ok "the canonical Windows name is a copy, not a symlink a non-admin install cannot create"
    else
      failure "bin/chaos.exe is $(ls -l "$E2W/home/bin/chaos.exe" 2>/dev/null || echo missing)"
    fi
  fi
fi

if [[ $failures -gt 0 ]]; then exit 1; fi
echo "assemble-integrity guards: OK (assembler, records, the guard's eight refusals, install, launch, repair)"
