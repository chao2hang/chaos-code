#!/usr/bin/env bash
# Read-only upstream reconnaissance: records how far this fork's ported
# `SOURCE_REV` has fallen behind upstream, into `sync/recon/`.
#
# It never fetches into the local repository, never mutates a working tree and
# never writes outside `sync/recon/`. It also never overwrites a record: a run
# whose output would differ from an existing same-day record writes beside it.
# Network failure exits non-zero instead of writing a record that would look like
# a completed review.
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
stamp="$(date -u +%Y-%m-%d)"
out_file="$out_dir/${stamp}-${tip:0:9}.md"

record="$(mktemp)"
trap 'rm -f "$compare_json" "$record"' EXIT
{
  echo "# 上游侦察 $stamp"
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
} >"$record"

# A recon record is a record, not a scratch file. The name is stable within a UTC
# day, so a second run used to overwrite the first: on 2026-10-03 the documented
# command replaced a hand-enriched record (ancestor check, fork scale, the count
# of changed files inside the l10n-protected paths) with the generated table, and
# `git status` was the only thing that said so. Identical content is a no-op;
# anything else lands beside it as `-2`, `-3`, so both readings survive.
action="written"
target="$out_file"
if [ -f "$out_file" ] && cmp -s "$record" "$out_file"; then
  action="unchanged"
else
  seq=2
  while [ -f "$target" ]; do
    target="$out_dir/${stamp}-${tip:0:9}-${seq}.md"
    seq=$((seq + 1))
  done
  cp "$record" "$target"
  # mktemp made the record 0600 and cp carries that mode to a new file. A recon
  # record is a document other maintainers read, not a secret.
  chmod 644 "$target"
fi

echo "upstream-recon: SOURCE_REV=${source_rev:0:9} upstream=${tip:0:9} status=$status ahead=$ahead behind=$behind"
if [ "$action" = unchanged ]; then
  echo "upstream-recon: ${out_file#"$repo_root"/} already holds this exact record"
else
  echo "upstream-recon: wrote ${target#"$repo_root"/}"
  if [ "$target" != "$out_file" ]; then
    echo "upstream-recon: ${out_file#"$repo_root"/} already existed with different content; a recon record is never overwritten, so this one went to ${target#"$repo_root"/}" >&2
  fi
fi
