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
  echo "# 上游侦察 $(date -u +%Y-%m-%d)"
  echo
  echo "由 \`scripts/upstream-recon.sh\` 生成的只读清点。**记录差距不等于移植**："
  echo "上游的每一项改动在进入本分叉之前，仍然要走 curated-port 评审。"
  echo
  echo "| 字段 | 值 |"
  echo "|---|---|"
  echo "| 分叉 \`SOURCE_REV\` | \`$source_rev\` |"
  echo "| 上游 HEAD | \`$tip\` |"
  echo "| 上游远端 | $upstream |"
  echo "| 比对状态 | $status |"
  echo "| 上游领先 \`SOURCE_REV\` 的提交数 | $ahead |"
  echo "| \`SOURCE_REV\` 领先上游的提交数 | $behind |"
  echo "| 比对窗口内的提交数 | $total |"
  echo "| 上游比对页面 | $api_root/compare/${source_rev:0:9}...HEAD |"
  echo
  echo "## 下一步"
  echo
  echo "- 把比对窗口内的提交分成三类：直接移植、需改造后移植、跳过"
  echo "  （fork 层架构、已被移除的特性、涉及安全的改动）。"
  echo "- 在改动任何被移植的源文件之前，先把结论写进 \`sync/\`。"
} >"$out_file"

echo "upstream-recon: SOURCE_REV=${source_rev:0:9} upstream=${tip:0:9} status=$status ahead=$ahead behind=$behind"
echo "upstream-recon: wrote ${out_file#"$repo_root"/}"
