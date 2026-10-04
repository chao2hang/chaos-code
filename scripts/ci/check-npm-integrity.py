#!/usr/bin/env python3
"""Check the digests that the published npm packages claim for their own binaries.

`crates/codegen/xai-grok-pager/npm/chaos/scripts/assemble-platform-packages.js` writes
`bin/integrity.json` into every per-platform package, and the two shipped entry points that install
and run the binary (`bin/postinstall.js`, `bin/chaos-bootstrap.js`, through `bin/install-lib.js`)
refuse bytes that do not hash to it. This guard checks the same claim from the outside, at
publication time, for every package in the tree rather than for the one package a user happens to
install. Six rules:

1. A platform package that carries a binary archive carries `bin/integrity.json`, and that file is
   readable JSON with `schema` equal to `chaos-npm-integrity/1`. A record that cannot be read is
   treated like a mismatch, not like nothing to check: deleting the file would otherwise be a way
   to switch the check off.
2. The record describes the binary that is actually in the package: `binary.name` is the file name
   the launcher looks for on that platform, `compressed.name` is that name plus `.br`, the
   platform/version the record names match the directory and the meta package's version, and the
   directory's own `package.json` carries the name the meta package's `optionalDependencies` pin.
3. `compressed.sha256` and `compressed.bytes` match the archive on disk, and decompressing that
   archive gives bytes whose SHA-256 and length match `binary.sha256` and `binary.bytes`. Both
   halves of the claim are checked against the package's own contents; the decompression is done by
   the same `zlib` the shipped installer uses (`node`), so the record and the check cannot disagree
   about what "the binary" means. `--no-decompress` skips that second half.
4. `binary.sha256` is 64 lowercase hex digits and both byte counts are positive integers. A record
   full of empty strings satisfies nothing, and a guard that only looks at key presence would say
   so.
5. If a plain (uncompressed) binary sits next to the archive, its digest has to equal
   `binary.sha256` too. The launcher prefers the archive, so a plain file that disagrees is either
   a leftover from a different build or something that was dropped in afterwards, and either way it
   is a second, contradicting answer about what this package contains.
6. With `--sha256sums FILE`, the digest the record claims for the binary has to be the digest
   `FILE` records for the release artifact named in `release_artifact` (`.exe` spelling accepted
   for the Windows targets). `release.yml` runs this after assembling: it is what ties a package on
   the npm registry to the artifact whose `.sig` sidecar covers the same bytes, and it is the only
   link in this chain that a mirror cannot rewrite on its way to a user.

What this guard does not claim: that the digests are honest. Whoever can replace a binary can
rewrite the record beside it, so rule 6 matters more than rules 1 to 5 -- it is the only rule whose
answer comes from outside the package. Rule 3 closes the other gap: with only the archive digest
checked, a record could keep a true `compressed.sha256` and claim an all-zero `binary.sha256`, and
the package would read as consistent while the installer's own check on the decompressed bytes was
the only thing standing between it and a user.

Usage:
  scripts/ci/check-npm-integrity.py                              # whole tree, host-side packages
  scripts/ci/check-npm-integrity.py --npm-root DIR               # a scratch npm tree
  scripts/ci/check-npm-integrity.py --package DIR [--package DIR ...]
  scripts/ci/check-npm-integrity.py --sha256sums release-bins/SHA256SUMS
  scripts/ci/check-npm-integrity.py --no-decompress     # skip rule 3's second half (needs no node)

Rule 3's second half needs a brotli decoder, which the standard library does not have, so it asks
`node` for the digest of the decompressed bytes -- the same `zlib.brotliDecompressSync` call the
shipped installer makes. A tree with an archive and no `node` on PATH is reported as unverified
rather than passed; use `--no-decompress` when that is the intent.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_NPM_ROOT = REPO_ROOT / "crates" / "codegen" / "xai-grok-pager" / "npm"
SCHEMA = "chaos-npm-integrity/1"
INTEGRITY_REL = ("bin", "integrity.json")
HEX64 = re.compile(r"\A[0-9a-f]{64}\Z")
# The six targets the assembler knows, and the binary name each one installs. Anything else in the
# tree is not a package this pipeline publishes, and inventing a seventh here would let a target
# that nobody assembles for still look verified.
PLATFORMS = {
    "chaos-darwin-arm64": "chaos",
    "chaos-darwin-x64": "chaos",
    "chaos-linux-arm64": "chaos",
    "chaos-linux-x64": "chaos",
    "chaos-win32-arm64": "chaos.exe",
    "chaos-win32-x64": "chaos.exe",
}


def sha256_of(path: Path) -> tuple[str, int]:
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as handle:
        while True:
            chunk = handle.read(1 << 22)
            if not chunk:
                break
            size += len(chunk)
            digest.update(chunk)
    return digest.hexdigest(), size


# Node is the only brotli decoder guaranteed to be present wherever these packages are built or
# installed, and using the same call the installer makes means this check cannot accept an archive
# that the installer itself would reject.  The digest is computed in node and only the hex string
# crosses the pipe, so a 200 MB binary does not have to be held in memory twice.
_NODE_DECOMPRESS = """
const fs = require('fs');
const zlib = require('zlib');
const crypto = require('crypto');
try {
    const raw = zlib.brotliDecompressSync(fs.readFileSync(process.argv[1]));
    process.stdout.write(JSON.stringify({
        sha256: crypto.createHash('sha256').update(raw).digest('hex'),
        bytes: raw.length,
    }));
} catch (err) {
    process.stderr.write(String(err && err.message ? err.message : err));
    process.exit(2);
}
"""


def decompress_digest(node: str, archive: Path) -> tuple[dict | None, str]:
    """`{sha256, bytes}` of what `archive` actually decompresses to, or `(_, why-not)`."""
    try:
        proc = subprocess.run([node, "-e", _NODE_DECOMPRESS, str(archive)],
                              capture_output=True, text=True, timeout=600)
    except (OSError, subprocess.TimeoutExpired) as err:
        return None, f"cannot run {node} to decompress it ({err})"
    if proc.returncode != 0:
        return None, proc.stderr.strip() or f"decompression failed with exit code {proc.returncode}"
    try:
        payload = json.loads(proc.stdout)
    except json.JSONDecodeError:
        return None, f"the decompression helper printed {proc.stdout!r}, not a digest object"
    if not isinstance(payload, dict) or not isinstance(payload.get("sha256"), str) \
            or not isinstance(payload.get("bytes"), int):
        return None, f"the decompression helper printed {payload!r}, not a sha256/bytes object"
    return payload, ""


def read_meta_manifest(npm_root: Path) -> tuple[dict, list[str]]:
    """The meta package's name and version, which every platform package has to agree with.

    The platform packages are named by appending their target to the meta package's name, so the
    pin the launcher resolves at install time only holds if that spelling is checked here.
    """
    manifest = npm_root / "chaos" / "package.json"
    if not manifest.is_file():
        return {}, [f"{manifest}: no meta package manifest to compare platform versions against"]
    try:
        payload = json.loads(manifest.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as err:
        return {}, [f"{manifest}: unreadable meta package manifest ({err})"]
    info: dict = {}
    problems: list[str] = []
    for key in ("name", "version"):
        value = payload.get(key)
        if not isinstance(value, str) or not value:
            problems.append(f"{manifest}: meta package manifest has no {key} string")
        else:
            info[key] = value
    return info, problems


def read_package_name(package_dir: Path) -> str | None:
    manifest = package_dir / "package.json"
    try:
        payload = json.loads(manifest.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    name = payload.get("name")
    return name if isinstance(name, str) else None


def parse_sha256sums(path: Path) -> tuple[dict[str, str], list[str]]:
    """Parse `sha256sum` output into `{file name: digest}`."""
    entries: dict[str, str] = {}
    problems: list[str] = []
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as err:
        return {}, [f"{path}: cannot be read ({err})"]
    for number, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        fields = line.split()
        if len(fields) != 2 or not HEX64.match(fields[0]):
            problems.append(f"{path}:{number}: not a `sha256sum` line: {line!r}")
            continue
        name = fields[1].lstrip("*")
        if name in entries and entries[name] != fields[0]:
            problems.append(f"{path}: {name} is listed twice with two different digests")
        entries[name] = fields[0]
    return entries, problems


def has_own_bytes(package_dir: Path) -> bool:
    """Whether this package carries a binary someone could install.

    A zero-length placeholder left in a working tree is not one, which is the same test
    `publish-npm.sh` uses to decide a package was never assembled.
    """
    expected = PLATFORMS.get(package_dir.name)
    bin_dir = package_dir / "bin"
    if expected is None or not bin_dir.is_dir():
        return False
    return any(p.is_file() and p.stat().st_size > 0 and (p.suffix == ".br" or p.name == expected)
               for p in bin_dir.iterdir())


def check_package(package_dir: Path, meta: dict, node: str | None = None,
                  decompress: bool = False) -> list[str]:
    """Every way one platform package's claim can fail to hold. An empty list means it holds."""
    problems: list[str] = []
    dir_name = package_dir.name
    expected_binary = PLATFORMS.get(dir_name)
    if expected_binary is None:
        return [f"{package_dir}: not one of the platform packages this pipeline publishes "
                f"({', '.join(sorted(PLATFORMS))})"]

    bin_dir = package_dir / "bin"
    # A zero-length archive is not an assembled binary; `publish-npm.sh` uses the same test, and
    # an empty placeholder left in a working tree must not read as a package with unrecorded bytes.
    archives = sorted(p for p in bin_dir.glob("*.br")) if bin_dir.is_dir() else []
    archives = [p for p in archives if p.stat().st_size > 0]
    plain = [p for p in bin_dir.iterdir()
             if p.name == expected_binary and p.is_file() and p.stat().st_size > 0] if bin_dir.is_dir() else []
    if not archives and not plain:
        return []  # Not assembled. `publish-npm.sh` and the release workflow decide that separately.

    integrity_path = package_dir.joinpath(*INTEGRITY_REL)
    if not integrity_path.is_file():
        return [f"{integrity_path}: missing, so nothing in this package says what its bytes should "
                "hash to and the shipped installer will refuse it"]
    try:
        record = json.loads(integrity_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as err:
        return [f"{integrity_path}: unreadable JSON ({err})"]

    if record.get("schema") != SCHEMA:
        problems.append(f"{integrity_path}: schema is {record.get('schema')!r}, expected {SCHEMA!r}")

    binary = record.get("binary") if isinstance(record.get("binary"), dict) else {}
    compressed = record.get("compressed") if isinstance(record.get("compressed"), dict) else {}

    if binary.get("name") != expected_binary:
        problems.append(f"{integrity_path}: binary.name is {binary.get('name')!r}, but the launcher "
                        f"for {dir_name} looks for {expected_binary!r}")
    if compressed.get("name") != f"{expected_binary}.br":
        problems.append(f"{integrity_path}: compressed.name is {compressed.get('name')!r}, "
                        f"expected {expected_binary + '.br'!r}")

    binary_digest = binary.get("sha256")
    if not isinstance(binary_digest, str) or not HEX64.match(binary_digest):
        problems.append(f"{integrity_path}: binary.sha256 is {binary_digest!r}, not 64 lowercase "
                        "hex digits")
    compressed_digest = compressed.get("sha256")
    if not isinstance(compressed_digest, str) or not HEX64.match(compressed_digest):
        problems.append(f"{integrity_path}: compressed.sha256 is {compressed_digest!r}, not 64 "
                        "lowercase hex digits")
    for label, holder in (("binary", binary), ("compressed", compressed)):
        value = holder.get("bytes")
        if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
            problems.append(f"{integrity_path}: {label}.bytes is {value!r}, not a positive integer")

    record_platform = record.get("platform")
    target = dir_name.removeprefix("chaos-")
    if record_platform != target:
        problems.append(f"{integrity_path}: platform is {record_platform!r}, but the package "
                        f"directory is {dir_name!r}")
    record_version = record.get("version")
    meta_version = meta.get("version")
    if meta_version and record_version != meta_version:
        problems.append(f"{integrity_path}: version is {record_version!r}, but the meta package "
                        f"publishes {meta_version!r}")
    expected_artifact = f"chaos-{target}"
    artifact = record.get("release_artifact")
    if artifact != expected_artifact:
        problems.append(f"{integrity_path}: release_artifact is {artifact!r}, expected "
                        f"{expected_artifact!r}")

    if archives:
        archive = archives[0]
        actual_digest, actual_size = sha256_of(archive)
        if isinstance(compressed_digest, str) and actual_digest != compressed_digest:
            problems.append(f"{archive}: sha256 is {actual_digest}, the record says {compressed_digest}")
        if isinstance(compressed.get("bytes"), int) and actual_size != compressed["bytes"]:
            problems.append(f"{archive}: is {actual_size} bytes, the record says {compressed['bytes']}")
        if len(archives) > 1:
            problems.append(f"{package_dir / 'bin'}: {len(archives)} archives ({', '.join(p.name for p in archives)}); "
                            "the record describes one, so the others are unaccounted for")
        elif decompress:
            if node is None:
                problems.append(f"{archive}: its decompressed bytes are not checked here, because no "
                                "`node` is on PATH to decompress them (say --no-decompress to make "
                                "that the intent rather than an accident)")
            else:
                payload, err = decompress_digest(node, archive)
                if payload is None:
                    problems.append(f"{archive}: does not decompress ({err}), so the shipped "
                                    "installer would refuse it")
                else:
                    if isinstance(binary_digest, str) and payload["sha256"] != binary_digest:
                        problems.append(f"{archive}: decompresses to bytes hashing {payload['sha256']}, "
                                        f"but the record calls the binary {binary_digest}; the "
                                        "installer hashes these same bytes and will stop on this")
                    claimed_size = binary.get("bytes")
                    if isinstance(claimed_size, int) and not isinstance(claimed_size, bool) \
                            and payload["bytes"] != claimed_size:
                        problems.append(f"{archive}: decompresses to {payload['bytes']} bytes, but the "
                                        f"record says the binary is {claimed_size}")

    for stray in plain:
        actual_digest, _ = sha256_of(stray)
        if isinstance(binary_digest, str) and actual_digest != binary_digest:
            problems.append(f"{stray}: sha256 is {actual_digest}, the record says {binary_digest}; "
                            f"the launcher prefers the archive, so this file is a second answer")

    package_name = read_package_name(package_dir)
    meta_name = meta.get("name")
    if meta_name and package_name and package_name != f"{meta_name}-{target}":
        problems.append(f"{package_dir / 'package.json'}: name is {package_name!r}, which is not the "
                        f"name the meta package {meta_name!r} pins for {dir_name}")
    return problems


def check_against_sha256sums(package_dirs: list[Path], sums: dict[str, str], sums_path: Path) -> list[str]:
    """Tie each package's claimed binary digest to the digest the release recorded for its artifact."""
    problems: list[str] = []
    for package_dir in package_dirs:
        integrity_path = package_dir.joinpath(*INTEGRITY_REL)
        if not integrity_path.is_file():
            continue  # already reported
        try:
            record = json.loads(integrity_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            continue
        binary = record.get("binary") if isinstance(record.get("binary"), dict) else {}
        artifact = record.get("release_artifact")
        digest = binary.get("sha256")
        if not isinstance(artifact, str) or not isinstance(digest, str):
            continue  # already reported by check_package
        for spelling in (artifact, f"{artifact}.exe"):
            if spelling in sums:
                if sums[spelling] != digest:
                    problems.append(f"{integrity_path}: binary.sha256 is {digest}, but "
                                    f"{sums_path} records {spelling} as {sums[spelling]}")
                break
        else:
            problems.append(f"{integrity_path}: release_artifact {artifact!r} is not listed in "
                            f"{sums_path}, so nothing outside this package vouches for these bytes")
    return problems


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="check the npm packages' claimed binary digests")
    parser.add_argument("--npm-root", type=Path, default=DEFAULT_NPM_ROOT,
                        help="the npm/ directory holding chaos plus the six platform packages")
    parser.add_argument("--package", type=Path, action="append", default=[],
                        help="check only this platform package directory (repeatable)")
    parser.add_argument("--sha256sums", type=Path,
                        help="cross-check each record against this `sha256sum` file (SHA256SUMS)")
    parser.add_argument("--require-assembled", action="store_true",
                        help="fail if any of the six platform packages has no binary at all")
    parser.add_argument("--no-decompress", action="store_true",
                        help="skip rule 3's second half: do not decompress the archives with node")
    args = parser.parse_args(argv)

    npm_root: Path = args.npm_root
    if not npm_root.is_dir():
        print(f"check-npm-integrity: {npm_root}: no npm package tree here", file=sys.stderr)
        return 1

    package_dirs = [p.resolve() for p in args.package] or [npm_root / name for name in PLATFORMS]
    meta, problems = read_meta_manifest(npm_root)

    if args.require_assembled:
        for package_dir in package_dirs:
            name = package_dir.name
            if name not in PLATFORMS:
                continue
            archive = package_dir / "bin" / f"{PLATFORMS[name]}.br"
            if not archive.is_file() or archive.stat().st_size == 0:
                problems.append(f"{archive}: nothing assembled for {name}")

    checked: list[Path] = []
    node = None if args.no_decompress else shutil.which("node")
    for package_dir in package_dirs:
        if not package_dir.is_dir():
            problems.append(f"{package_dir}: no such platform package")
            continue
        found = check_package(package_dir, meta, node=node, decompress=not args.no_decompress)
        problems.extend(found)
        if not found:
            checked.append(package_dir)

    if args.sha256sums is not None:
        sums, sum_problems = parse_sha256sums(args.sha256sums)
        problems.extend(sum_problems)
        if not sum_problems:
            problems.extend(check_against_sha256sums(package_dirs, sums, args.sha256sums))

    if problems:
        for problem in problems:
            print(f"check-npm-integrity: {problem}", file=sys.stderr)
        print(f"check-npm-integrity: {len(problems)} problem(s) across "
              f"{len(package_dirs)} package(s)", file=sys.stderr)
        return 1

    claim = "archive and decompressed bytes both match the record" \
        if not args.no_decompress else "archive matches the record, decompression not checked"
    verified = sum(1 for package_dir in checked if has_own_bytes(package_dir))
    suffix = f", cross-checked against {args.sha256sums.name}" if args.sha256sums else ""
    print(f"check-npm-integrity: OK ({verified} of {len(package_dirs)} package(s) where the {claim}"
          f"{suffix})")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
