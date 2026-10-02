#!/usr/bin/env bash
# Read-only upstream reconnaissance: records how far this fork's ported
# `SOURCE_REV` has fallen behind upstream, into `sync/recon/`.
#
# It never fetches into the local repository, never mutates a working tree and
# never writes outside `sync/recon/`. Network failure exits non-zero instead of
# writing a record that would look like a completed review.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_rev_file="$repo_root/SOURCE_REV"
upstream="${UPSTREAM_REMOTE:-https://github.com/xai-org/grok-build}"
api_root="${UPSTREAM_API:-https://api.github.com/repos/xai-org/grok-build}"

test -f "$source_rev_file" || {
  echo "upstream-recon: missing $source_rev_file" >&2
  exit 1
}
source_rev="$(tr -d '[:space:]' <"$source_rev_file")"
test -n "$source_rev" || {
  echo "upstream-recon: SOURCE_REV is empty" >&2
  exit 1
}

tip="$(git ls-remote "$upstream" HEAD | cut -f1)"
test -n "$tip" || {
  echo "upstream-recon: could not read upstream HEAD from $upstream" >&2
  exit 1
}

# The compare endpoint reports the gap without cloning upstream history.
compare_json="$(mktemp)"
trap 'rm -f "$compare_json"' EXIT
http_code="$(curl -sS -o "$compare_json" -w '%{http_code}' \
  "$api_root/compare/$source_rev...HEAD" || echo 000)"
case "$http_code" in
200) ;;
*)
  echo "upstream-recon: compare API returned HTTP $http_code" >&2
  exit 1
  ;;
esac

read -r status ahead behind total <<<"$(python3 - "$compare_json" <<'PY'
import json, sys
data = json.load(open(sys.argv[1]))
print(data.get("status", "unknown"), data.get("ahead_by", -1),
      data.get("behind_by", -1), data.get("total_commits", -1))
PY
)"

out_dir="$repo_root/sync/recon"
mkdir -p "$out_dir"
out_file="$out_dir/$(date -u +%Y-%m-%d)-${tip:0:9}.md"

{
  echo "# Upstream recon $(date -u +%Y-%m-%d)"
  echo
  echo "Read-only inventory produced by \`scripts/upstream-recon.sh\`. Recording a"
  echo "gap is not a port: every upstream change still needs the curated-port"
  echo "review before it reaches this fork."
  echo
  echo "| Field | Value |"
  echo "|---|---|"
  echo "| Fork \`SOURCE_REV\` | \`$source_rev\` |"
  echo "| Upstream HEAD | \`$tip\` |"
  echo "| Upstream remote | $upstream |"
  echo "| Compare status | $status |"
  echo "| Commits upstream is ahead of \`SOURCE_REV\` | $ahead |"
  echo "| Commits \`SOURCE_REV\` is ahead of upstream | $behind |"
  echo "| Commits in compare window | $total |"
  echo "| Upstream comparison page | $api_root/compare/${source_rev:0:9}...HEAD |"
  echo
  echo "## Next step"
  echo
  echo "- Triage the commits in the compare window into: port, port-with-adaptation,"
  echo "  or skip (fork-layer architecture, removed feature, security-sensitive)."
  echo "- Record the decision in \`sync/\` before changing any ported source file."
} >"$out_file"

echo "upstream-recon: SOURCE_REV=${source_rev:0:9} upstream=${tip:0:9} status=$status ahead=$ahead behind=$behind"
echo "upstream-recon: wrote ${out_file#"$repo_root"/}"
