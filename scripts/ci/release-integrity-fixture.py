#!/usr/bin/env python3
"""Build a complete fake release, signed, plus one mutated copy per scenario.

Shared by the two release-integrity labs: install-integrity-in-docker.sh drives
scripts/install.sh against the output, install-integrity-powershell.sh drives
scripts/install.ps1. One generator means both installers are offered the same
release, so a difference in what they accept is a difference between the
installers and not an artefact of two fixtures that drifted apart.

Nothing here fakes what an installer is supposed to do: it produces exactly the
files the release workflow publishes for one asset, and the scenarios then break
one of them the way a bad mirror or a hostile publisher would.

Usage: release-integrity-fixture.py --root DIR --version 9.9.9 --asset NAME
"""

import argparse
import base64
import hashlib
import os
import sys

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

# Stand-in for the released binary. install.sh chmod +x's it and runs --version on it,
# and its second-run check compares that output against the requested version. The
# PowerShell installer reaches the same line and tolerates not being able to exec it,
# which on Linux is what happens to a file named *.exe.
ARTIFACT = """#!/bin/sh
case "$1" in
  --version|-V) echo "chaos %s" ;;
  --help) echo "usage: chaos" ;;
  *) echo "chaos %s: this fixture does nothing else" ;;
esac
"""

# install.ps1 refuses any artifact smaller than this before it hashes it, because a
# truncated transfer is the failure it is guarding against and a 200 OK with a short
# body is exactly what one looks like. A real artifact is over 150 MB. The fixture has
# to clear that bar to reach the checks under test; without the padding every
# PowerShell scenario would end in "too small" and prove nothing.
MIN_ARTIFACT_BYTES = 1024 * 1024


def keypair(root, tag):
    private = Ed25519PrivateKey.generate()
    # Positional, and no public_bytes_raw(): the distro build of cryptography in
    # the lab image is 38.x and on older Debians 34.x, which is what a user gets.
    public = private.public_key().public_bytes(
        serialization.Encoding.Raw, serialization.PublicFormat.Raw
    )
    with open(os.path.join(root, tag), "w") as handle:
        handle.write(base64.b64encode(public).decode() + "\n")
    return private


def sign(private, payload):
    return (base64.b64encode(private.sign(payload)).decode() + "\n").encode()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--asset", required=True)
    args = parser.parse_args()
    if not args.asset.startswith("chaos-"):
        raise SystemExit("--asset must look like a release asset name")

    out = os.path.join(args.root, "releases")
    ours = keypair(args.root, "pubkey-ours")
    keypair(args.root, "pubkey-other")

    script = (ARTIFACT % (args.version, args.version)).encode()
    # Padded with comment lines, so the file is still an executable shell script of the
    # advertised size rather than a blob that only looks large.
    filler = b"#" * (MIN_ARTIFACT_BYTES + 1 - len(script))
    artifact = script + filler + b"\n"
    digest = hashlib.sha256(artifact).hexdigest()
    sums = "{}  {}\n".format(digest, args.asset).encode()
    signature = sign(ours, artifact)

    def write(case, name, payload):
        directory = os.path.join(out, case)
        os.makedirs(directory, exist_ok=True)
        with open(os.path.join(directory, name), "wb") as handle:
            handle.write(payload)

    write("good", args.asset, artifact)
    write("good", "SHA256SUMS", sums)
    write("good", args.asset + ".sig", signature)

    # A mirror that serves different bytes than it advertises. The digest and the
    # signature still describe the original, so the checksum is what catches it.
    tampered = artifact.replace(b"does nothing else", b"does something else")
    write("tampered", args.asset, tampered)
    write("tampered", "SHA256SUMS", sums)
    write("tampered", args.asset + ".sig", signature)

    # The mirror also recomputes SHA256SUMS. Only the signature stands in the way
    # now, which is the check this scenario exists to isolate.
    forged = "{}  {}\n".format(hashlib.sha256(tampered).hexdigest(), args.asset).encode()
    write("forged-sums", args.asset, tampered)
    write("forged-sums", "SHA256SUMS", forged)
    write("forged-sums", args.asset + ".sig", signature)

    # A release whose sidecar is missing.
    write("no-sig", args.asset, artifact)
    write("no-sig", "SHA256SUMS", sums)

    # The asset is not listed: a well-formed manifest for some other platform.
    write("no-sums-entry", args.asset, artifact)
    write(
        "no-sums-entry",
        "SHA256SUMS",
        "{}  chaos-plan9-mips\n".format(digest).encode(),
    )
    write("no-sums-entry", args.asset + ".sig", signature)

    # A proxy that answers 200 with an HTML error page instead of the checksums.
    write("html-sums", args.asset, artifact)
    write(
        "html-sums",
        "SHA256SUMS",
        (
            b"<!DOCTYPE html><html><head><title>502 Bad Gateway</title></head>"
            b"<body><h1>502 Bad Gateway</h1></body></html>\n"
        ),
    )
    write("html-sums", args.asset + ".sig", signature)

    # A truncated download: 200 OK with no body.
    write("empty-artifact", args.asset, b"")
    write("empty-artifact", "SHA256SUMS", sums)
    write("empty-artifact", args.asset + ".sig", signature)

    print(args.asset)


if __name__ == "__main__":
    sys.exit(main())
