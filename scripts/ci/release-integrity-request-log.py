#!/usr/bin/env python3
"""Assert the fixture was asked for nothing but the good release's own three files.

A refusal that was never asked for is worth nothing, and an install that quietly
fetched something else would not be an install of the fixture. This reads the request
log the shared mirror wrote.

Usage: release-integrity-request-log.py LOG_PATH ASSET
"""

import sys

log_path, asset = sys.argv[1], sys.argv[2]
allowed = {asset, "SHA256SUMS", asset + ".sig"}
bad = []
with open(log_path) as handle:
    for line in handle:
        parts = line.split()
        if len(parts) != 2 or parts[0] != "good" or parts[1] not in allowed:
            bad.append(line.strip())
if bad:
    print("unexpected: " + " | ".join(bad))
    sys.exit(1)
print("only {} , SHA256SUMS and the sidecar".format(asset))
