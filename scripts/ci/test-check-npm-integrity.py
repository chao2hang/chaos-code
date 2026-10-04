#!/usr/bin/env python3
"""Fixtures for `scripts/ci/check-npm-integrity.py`.

Run with:  python3 scripts/ci/test-check-npm-integrity.py

The tree the guard reads is built once by the shipped assembler -- six packages, six real brotli
archives, six real records -- and every case below copies that tree (a millisecond) and breaks one
thing in it. Nothing here hand-writes a record, so a case cannot pass by disagreeing with the
producer about the format, and a refusal can only come from the one thing the case changed.

The two modes matter: the default decompresses each archive with node and holds the result to
`binary.sha256`, while `--no-decompress` skips that half. A case that only the second half can see
is asserted to be caught by the default and *missed* by the skip, so what the flag costs is
recorded rather than implied.

Nothing is published, installed or fetched.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
GUARD = REPO_ROOT / "scripts" / "ci" / "check-npm-integrity.py"
ASSEMBLER = (REPO_ROOT / "crates" / "codegen" / "xai-grok-pager" / "npm" / "chaos"
             / "scripts" / "assemble-platform-packages.js")
TARGETS = ["darwin-arm64", "darwin-x64", "linux-arm64", "linux-x64", "win32-arm64", "win32-x64"]

failures: list[str] = []
checks = 0


def ok(label: str) -> None:
    global checks
    checks += 1
    print(f"ok: {label}")


def expect_refused(label: str, proc: subprocess.CompletedProcess, needle: str) -> None:
    output = proc.stdout + proc.stderr
    if proc.returncode == 0:
        fail(f"{label}: the guard accepted it", output)
    elif needle not in output:
        fail(f"{label}: refused without saying {needle!r}", output)
    else:
        ok(label)


def expect_accepted(label: str, proc: subprocess.CompletedProcess, needle: str | None = None) -> None:
    if proc.returncode != 0:
        fail(f"{label}: the guard refused a valid tree", proc.stdout + proc.stderr)
    elif needle and needle not in proc.stdout:
        fail(f"{label}: accepted without reporting {needle!r}", proc.stdout + proc.stderr)
    else:
        ok(label)


def fail(label: str, detail: str = "") -> None:
    failures.append(label)
    print(f"not ok: {label}", file=sys.stderr)
    for line in detail.splitlines():
        print(f"    {line}", file=sys.stderr)


def run(argv: list[str], env_extra: dict[str, str] | None = None) -> subprocess.CompletedProcess:
    env = dict(os.environ)
    env.update(env_extra or {})
    return subprocess.run([sys.executable, *argv], capture_output=True, text=True, env=env)


def guard(npm: Path, *args: str) -> subprocess.CompletedProcess:
    return run([str(GUARD), "--npm-root", str(npm), *args])


def package(npm: Path, target: str) -> Path:
    return npm / f"chaos-{target}"


def archive_of(npm: Path, target: str) -> Path:
    return package(npm, target) / "bin" / ("chaos.exe.br" if target.startswith("win32-") else "chaos.br")


def record_path(npm: Path, target: str) -> Path:
    return package(npm, target) / "bin" / "integrity.json"


def read_record(npm: Path, target: str) -> dict:
    return json.loads(record_path(npm, target).read_text())


def edit_record(npm: Path, target: str, mutate) -> None:
    path = record_path(npm, target)
    payload = json.loads(path.read_text())
    mutate(payload)
    path.write_text(json.dumps(payload, indent=2) + "\n")


# --- the one valid tree ----------------------------------------------------------------------
def build_golden(root: Path) -> Path:
    """Assemble a six-package tree with the shipped assembler, in a repository-shaped scratch dir.

    The assembler resolves the notices bundle and the version from paths relative to itself, so the
    copy has to sit in the same shape it ships in -- which is also the shape a release job has.
    """
    repo = root / "crates" / "codegen" / "xai-grok-pager"
    npm = repo / "npm"
    chaos = npm / "chaos"
    (chaos / "scripts").mkdir(parents=True)
    (chaos / "bin").mkdir(parents=True)
    shutil.copy(ASSEMBLER, chaos / "scripts" / ASSEMBLER.name)
    (chaos / "package.json").write_text(json.dumps(
        {"name": "chaos-code", "version": "9.9.9",
         "optionalDependencies": {f"chaos-code-{t}": "9.9.9" for t in TARGETS}}) + "\n")
    (root / "THIRD-PARTY-NOTICES").write_text(
        "PART I — PER-PACKAGE ENTRIES\n\nsynthetic-crate 1.2.3\n  License: MIT\n")
    # The assembler resolves the ported-code notices as `npmRoot/../..`, so the scratch layout has
    # to be the repository's own depth -- which is also the shape a release job has.
    ported = root / "crates" / "codegen" / "xai-grok-tools"
    ported.mkdir(parents=True)
    (ported / "THIRD_PARTY_NOTICES.md").write_text("# Ported source code notices\n\nPorted.\n")
    for target in TARGETS:
        (npm / f"chaos-{target}").mkdir(parents=True)
        (npm / f"chaos-{target}" / "package.json").write_text(json.dumps(
            {"name": f"chaos-code-{target}", "version": "0.0.0", "files": ["bin/"]}) + "\n")

    bins = root / "bins"
    bins.mkdir()
    env_bins = {}
    for target in TARGETS:
        # Distinct bytes per target, so a digest cannot be shared between two packages unnoticed.
        (bins / target).write_text(f"#!/bin/sh\nprintf 'fixture {target} %s\\n' \"$*\"\n")
        env_bins[f"CHAOS_{target.upper().replace('-', '_')}"] = str(bins / target)
    proc = subprocess.run(["node", str(chaos / "scripts" / ASSEMBLER.name)],
                          capture_output=True, text=True,
                          env={**os.environ, "CHAOS_ROOT": str(root), **env_bins})
    if proc.returncode != 0:
        raise RuntimeError(f"the assembler failed: {proc.stdout}{proc.stderr}")
    return npm


def write_sums(root: Path, npm: Path, *, drop: str | None = None, lie: str | None = None,
               name: str = "SHA256SUMS") -> Path:
    """A SHA256SUMS in the release's own spelling, derived from the records unless told to lie."""
    root.mkdir(parents=True, exist_ok=True)
    lines = []
    for target in TARGETS:
        if target == drop:
            continue
        digest = lie or read_record(npm, target)["binary"]["sha256"]
        suffix = ".exe" if target.startswith("win32-") else ""
        lines.append(f"{digest}  chaos-{target}{suffix}")
    path = root / name
    path.write_text("\n".join(lines) + "\n")
    return path


def case(trees: Path, golden: Path, label: str, needle: str, mutate, *args: str) -> Path:
    """One mutation in its own copy of the tree, asserted to be refused for one named reason."""
    copy = trees / f"case-{len(failures + [label])}-{abs(hash(label)) % 10**8}"
    shutil.copytree(golden, copy)
    mutate(copy)
    expect_refused(label, guard(copy, *args), needle)
    return copy


def main() -> int:
    tmp = Path(tempfile.mkdtemp(prefix="check-npm-integrity-"))
    try:
        golden = build_golden(tmp / "golden")
        trees = tmp / "trees"
        trees.mkdir()

        expect_accepted("a tree the assembler just wrote is accepted", guard(golden), "6 of 6")
        expect_accepted("--no-decompress accepts it too, and says which half it skipped",
                        guard(golden, "--no-decompress"), "decompression not checked")
        expect_accepted("the release's own SHA256SUMS vouches for all six",
                        guard(golden, "--sha256sums", str(write_sums(tmp / "sums", golden))),
                        "cross-checked against SHA256SUMS")
        expect_refused("an artifact missing from SHA256SUMS leaves that package unvouched",
                       guard(golden, "--sha256sums",
                             str(write_sums(tmp / "sums-drop", golden, drop="linux-x64"))),
                       "is not listed in")
        expect_refused("a SHA256SUMS that disagrees with the record is a contradiction",
                       guard(golden, "--sha256sums",
                             str(write_sums(tmp / "sums-lie", golden, lie="0" * 64))),
                       "records chaos-linux-x64 as")

        def write_raw_sums(sub: str, text: str) -> Path:
            path = tmp / sub
            path.mkdir(parents=True, exist_ok=True)
            (path / "SHA256SUMS").write_text(text)
            return path / "SHA256SUMS"

        expect_refused("a malformed SHA256SUMS line is refused rather than skipped",
                       guard(golden, "--sha256sums", str(write_raw_sums("d", "not a checksum line\n"))),
                       "not a `sha256sum` line")
        # Two well-separated fields is the shape `sha256sum` writes, so a line in that shape whose
        # first field is not a digest is the forgery the field-count test alone cannot see.
        expect_refused("a SHA256SUMS digest field that is not a digest is refused, not compared",
                       guard(golden, "--sha256sums", str(write_raw_sums("f", "deadbeef  chaos-linux-x64\n"))),
                       "not a `sha256sum` line")
        expect_refused("one artifact listed with two digests is refused",
                       guard(golden, "--sha256sums", str(write_raw_sums(
                           "e", f"{'1' * 64}  chaos-linux-x64\n{'2' * 64}  chaos-linux-x64\n"))),
                       "listed twice")
        expect_refused("a SHA256SUMS that cannot be read is refused",
                       guard(golden, "--sha256sums", str(tmp / "nowhere" / "SHA256SUMS")),
                       "cannot be read")

        case(trees, golden, "an archive whose bytes changed", "chaos.br: sha256 is",
             lambda p: flip_byte(archive_of(p, "linux-x64")))
        case(trees, golden, "a record that has been deleted is not the absence of a problem",
             "missing, so nothing in this package says",
             lambda p: record_path(p, "linux-x64").unlink())
        case(trees, golden, "a record that is not JSON", "unreadable JSON",
             lambda p: record_path(p, "linux-x64").write_text("{ \"schema\": "))
        case(trees, golden, "another schema", "schema is 'chaos-npm-integrity/0'",
             lambda p: edit_record(p, "linux-x64", lambda r: r.update(schema="chaos-npm-integrity/0")))
        # Refusing one spelling while accepting the next one is the failure mode here: a reader that
        # keeps a list of schemas it will parse leniently stops being a format check.
        case(trees, golden, "the next schema version is refused as firmly as the last one",
             "schema is 'chaos-npm-integrity/2'",
             lambda p: edit_record(p, "linux-x64", lambda r: r.update(schema="chaos-npm-integrity/2")))
        case(trees, golden, "a record describing another binary", "binary.name is 'grok'",
             lambda p: edit_record(p, "linux-x64", lambda r: r["binary"].update(name="grok")))
        case(trees, golden, "an archive name the launcher will not look for",
             "compressed.name is 'grok.br'",
             lambda p: edit_record(p, "linux-x64", lambda r: r["compressed"].update(name="grok.br")))
        case(trees, golden, "a digest that is not 64 hex", "not 64 lowercase hex",
             lambda p: edit_record(p, "linux-x64", lambda r: r["binary"].update(sha256="0" * 63)))
        case(trees, golden, "an uppercase digest is not the digest format either",
             "not 64 lowercase hex",
             lambda p: edit_record(p, "linux-x64", lambda r: r["binary"].update(sha256="A" * 64)))
        case(trees, golden, "a zero-length binary claim", "binary.bytes is 0",
             lambda p: edit_record(p, "linux-x64", lambda r: r["binary"].update(bytes=0)))
        case(trees, golden, "a byte count that is not a number", "binary.bytes is '1024'",
             lambda p: edit_record(p, "linux-x64", lambda r: r["binary"].update(bytes="1024")))
        case(trees, golden, "true is not a byte count", "binary.bytes is True",
             lambda p: edit_record(p, "linux-x64", lambda r: r["binary"].update(bytes=True)))
        case(trees, golden, "an archive length the file on disk disagrees with", "bytes, the record says",
             lambda p: edit_record(p, "linux-x64", lambda r: r["compressed"].update(
                 bytes=r["compressed"]["bytes"] + 1)))
        case(trees, golden, "a record for the wrong platform", "platform is 'darwin-x64'",
             lambda p: edit_record(p, "linux-x64", lambda r: r.update(platform="darwin-x64")))
        case(trees, golden, "a record from another version", "the meta package publishes '9.9.9'",
             lambda p: edit_record(p, "linux-x64", lambda r: r.update(version="9.9.8")))
        case(trees, golden, "an artifact name the release does not build",
             "release_artifact is 'chaos-linux64'",
             lambda p: edit_record(p, "linux-x64", lambda r: r.update(release_artifact="chaos-linux64")))
        case(trees, golden, "a package named differently from the meta pin",
             "is not the name the meta package 'chaos-code' pins",
             lambda p: (package(p, "linux-x64") / "package.json").write_text(
                 json.dumps({"name": "chaos-linux-x64", "version": "9.9.9"})))
        case(trees, golden, "a second archive nobody accounted for", "2 archives",
             lambda p: shutil.copy2(archive_of(p, "linux-x64"), package(p, "linux-x64") / "bin" / "other.br"))
        case(trees, golden, "a stale plain binary beside the archive is a second answer",
             "the launcher prefers the archive",
             lambda p: (package(p, "linux-x64") / "bin" / "chaos").write_text("stale\n"))
        case(trees, golden, "a zero-length archive is not bytes to check", "nothing assembled",
             lambda p: archive_of(p, "linux-x64").write_bytes(b""), "--require-assembled")
        # The same tree without `--require-assembled`. An empty archive is nobody's binary, so it is
        # neither a problem nor a verified package, and the OK line has to leave it out of the count
        # rather than announce six packages whose bytes it never hashed.
        zerobyte = trees / "zerobyte"
        shutil.copytree(golden, zerobyte)
        archive_of(zerobyte, "linux-x64").write_bytes(b"")
        expect_accepted("a zero-length archive leaves that package out of the verified count",
                        guard(zerobyte), "5 of 6")
        case(trees, golden, "the Windows package under the Unix name", "binary.name is 'chaos'",
             lambda p: edit_record(p, "win32-x64", lambda r: (
                 r["binary"].update(name="chaos"), r["compressed"].update(name="chaos.br"))))

        # The half only decompression can see: caught by default, missed by the skip.
        def zero_binary_digest(p: Path) -> None:
            edit_record(p, "linux-x64", lambda r: r["binary"].update(sha256="0" * 64))

        case(trees, golden, "a binary digest no bytes hash to", "the record calls the binary 0000",
             zero_binary_digest)
        case(trees, golden, "a length the decompressed bytes disagree with", "decompresses to",
             lambda p: edit_record(p, "linux-x64", lambda r: r["binary"].update(
                 bytes=r["binary"]["bytes"] + 1)))
        shutil.copytree(golden, trees / "skip")
        zero_binary_digest(trees / "skip")
        expect_accepted("--no-decompress does not see that forgery, which is what it costs",
                        guard(trees / "skip", "--no-decompress"), "decompression not checked")
        expect_refused("the same tree is still caught once SHA256SUMS is consulted",
                       guard(trees / "skip", "--sha256sums", str(write_sums(tmp / "sums-skip", golden))),
                       "binary.sha256 is 0000")

        # Decompression is the half that needs a decoder. Where there is none, the archives are
        # unchecked, which has to be a refusal: a build host without node on PATH would otherwise
        # report six verified packages having hashed nothing but the compressed bytes.
        no_node = {"PATH": "/nonexistent"}
        expect_refused("with no node on PATH the decompressed bytes are reported as unchecked",
                       run([str(GUARD), "--npm-root", str(golden)], env_extra=no_node),
                       "no `node` is on PATH")
        expect_accepted("the same tree passes once decompression is said to be out of scope",
                        run([str(GUARD), "--npm-root", str(golden), "--no-decompress"],
                            env_extra=no_node),
                        "decompression not checked")


        # Trees that are not assembled, and arguments that point at nothing.
        empty = trees / "empty"
        (empty / "chaos").mkdir(parents=True)
        (empty / "chaos" / "package.json").write_text(json.dumps({"name": "chaos-code", "version": "1.0.0"}))
        for target in TARGETS:
            (empty / f"chaos-{target}").mkdir(parents=True)
            (empty / f"chaos-{target}" / "package.json").write_text(json.dumps(
                {"name": f"chaos-code-{target}", "version": "1.0.0"}))
            (empty / f"chaos-{target}" / "bin").mkdir()
            (empty / f"chaos-{target}" / "bin" / ".gitkeep").write_text("")
        expect_accepted("an un-assembled tree has nothing to claim and nothing to check",
                        guard(empty), "0 of 6")
        expect_refused("--require-assembled refuses that same tree",
                       guard(empty, "--require-assembled"), "nothing assembled for chaos-linux-x64")
        expect_refused("a --package that is not one of the six is refused, not skipped",
                       run([str(GUARD), "--npm-root", str(golden), "--package", str(empty)]),
                       "not one of the platform packages")
        expect_refused("a --package that does not exist is refused",
                       guard(golden, "--package", str(golden / "chaos-nowhere")),
                       "no such platform package")
        expect_refused("--npm-root pointing at nothing is refused",
                       run([str(GUARD), "--npm-root", str(tmp / "nowhere")]),
                       "no npm package tree here")

        no_meta = trees / "no-meta"
        shutil.copytree(golden, no_meta)
        (no_meta / "chaos" / "package.json").unlink()
        expect_refused("with no meta manifest there is nothing to compare versions against",
                       guard(no_meta), "no meta package manifest")
        (no_meta / "chaos").mkdir(parents=True, exist_ok=True)
        (no_meta / "chaos" / "package.json").write_text("{ not json")
        expect_refused("an unreadable meta manifest is refused", guard(no_meta), "unreadable meta")
        (no_meta / "chaos" / "package.json").write_text(json.dumps({"version": "9.9.9"}))
        expect_refused("a meta manifest with no name cannot vouch for the platform names",
                       guard(no_meta), "no name string")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    if failures:
        print(f"test-check-npm-integrity: {len(failures)} of {checks} cases failed", file=sys.stderr)
        return 1
    print(f"test-check-npm-integrity: OK ({checks} cases against the guard)")
    return 0


def flip_byte(path: Path) -> None:
    data = bytearray(path.read_bytes())
    data[len(data) // 2] ^= 0x01
    path.write_bytes(bytes(data))


if __name__ == "__main__":
    sys.exit(main())
