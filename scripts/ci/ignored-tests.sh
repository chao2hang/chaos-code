#!/usr/bin/env bash
# 列出全工作区被 `#[ignore]` 的测试，含 crate / 文件 / 行号 / reason。
#
# 用法：
#   scripts/ci/ignored-tests.sh              # 所有 ignored 测试，按 crate 分组
#   scripts/ci/ignored-tests.sh --csv        # 机器可读 CSV
#   scripts/ci/ignored-tests.sh --stale      # 只列 review date 已过或未设的
#
# 单测和集成测试都算。理由从 `#[ignore = "..."]` 中提取；
# 没有理由的裸 `#[ignore]` 归为 "NO_REASON" —— CI 应当拒绝这类。
#
# 输出始终发到 stdout；退出码：
#   0 = 有至少一个 ignored 测试（正常）
#   1 = 解析错误或空
set -uo pipefail

MODE="human"
FILTER="all"

for arg in "$@"; do
    case "$arg" in
        --csv)   MODE="csv" ;;
        --stale) FILTER="stale" ;;
        -h|--help)
            sed -n '2,/^$/p' "$0"
            exit 0
            ;;
        *)
            echo "unknown arg: $arg" >&2
            exit 1
            ;;
    esac
done

# --- 找到所有 *.rs 文件 ---------------------------------------------------
RS_FILES=$(find crates -name '*.rs' -type f 2>/dev/null | sort)

# --- awk 逐文件提取 ignore 属性 --------------------------------------------
# 我们找的是 `#[ignore]` 或 `#[ignore = "..."]`，它们紧跟在一个 `#[test]` 或
# `fn <name>() {` 之前/之后不远处。简化策略：凡是 `#[ignore]` 行的 *下一个*
# 函数名/测试名，就是被忽略的测试。
#
# 为了避免漏报，我们直接输出 *所有* `#[ignore(..)]` 出现处以及离它最近的
# `fn` 名。人工审 csv 时可以筛掉非测试用的 ignore。

CSV_HEADER="crate,file,line,reason,nearest_fn"

collect() {
    while IFS= read -r f; do
        # 算出 crate 名：从 Cargo.toml 向上找
        crate=""
        dir=$(dirname "$f")
        while [ "$dir" != "." ] && [ "$dir" != "/" ] && [ -z "$crate" ]; do
            if [ -f "$dir/Cargo.toml" ]; then
                crate=$(grep '^name = ' "$dir/Cargo.toml" | head -1 | sed 's/name = "\(.*\)"/\1/')
                break
            fi
            dir=$(dirname "$dir")
        done
        [ -z "$crate" ] && crate="unknown"

        # 逐行扫：记录最近一个 fn 名；遇到 #[ignore] 就输出一行
        # 关键：#[ignore = "..."] 可能跨行 —— 任何包含 `#[ignore` 的行都
        # 进入"开始收 reason"状态，状态机直到遇到 `"` 配对完成或换行 +
        # 不是 continuation 行才结束。简化：把所有 #[ignore 起始行的
        # 下面最多 5 行累计成一个 reason 字符串。
        awk -v crate="$crate" -v file="$f" '
        BEGIN { nearest_fn = "" }
        /^[[:space:]]*(pub[[:space:]]+)?(async[[:space:]]+)?fn[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[[:space:]]*\(/ {
            match($0, /fn[[:space:]]+([A-Za-z_][A-Za-z0-9_]*)/, arr)
            if (arr[1] != "") nearest_fn = arr[1]
        }
        /#\[ignore[[:space:]]*(=|\])/ {
            reason = "NO_REASON"
            # 收 reason：从这一行的 `=` 后开始,到下一行 `]"` 收尾
            in_reason = 0
            collected = ""
            for (i = 1; i <= 5; i++) {
                line = (i == 1 ? $0 : (getline nextline) > 0 ? nextline : "")
                if (line == "") break
                if (!in_reason && index(line, "#[ignore") > 0) {
                    # 找第一个 "
                    p = index(line, "\"")
                    if (p > 0) {
                        in_reason = 1
                        rest = substr(line, p + 1)
                        # 找下一个 ", 但忽略 \""
                        for (j = 1; j <= length(rest); j++) {
                            c = substr(rest, j, 1)
                            if (c == "\\" && j < length(rest)) { collected = collected substr(rest, j, 2); j++; continue }
                            if (c == "\"") { in_reason = 0; break }
                            collected = collected c
                        }
                        if (in_reason) {
                            # 多行：line 不含收尾的 `"`，继续下一行
                            next
                        }
                        break
                    }
                } else if (in_reason) {
                    # 在多行 reason 内部
                    for (j = 1; j <= length(line); j++) {
                        c = substr(line, j, 1)
                        if (c == "\\" && j < length(line)) { collected = collected substr(line, j, 2); j++; continue }
                        if (c == "\"") { in_reason = 0; break }
                        collected = collected c
                    }
                    if (!in_reason) break
                }
            }
            if (collected != "") reason = collected
            gsub(/"/, "\"\"", reason)
            gsub(/"/, "\"\"", nearest_fn)
            printf "%s,%s,%d,\"%s\",\"%s\"\n", crate, file, NR, reason, nearest_fn
            nearest_fn = ""
        }
        ' "$f"
    done <<< "$RS_FILES"
}

if [ "$MODE" = "csv" ]; then
    echo "$CSV_HEADER"
    collect
    exit 0
fi

# --- human 模式 -----------------------------------------------------------
rows=$(collect)
total=$(echo "$rows" | wc -l)
echo "Total ignored test attributes: $total"
echo

# 按 crate 分组统计
echo "--- by crate ---"
echo "$rows" | awk -F, '{print $1}' | sort | uniq -c | sort -rn | head -40
echo

# 裸 ignore（无理由）的数目
bare=$(echo "$rows" | grep -c ',NO_REASON,' || true)
echo "--- bare #[ignore] (no reason): $bare ---"
if [ "$bare" -gt 0 ]; then
    echo "$rows" | grep ',NO_REASON,'
fi
echo

# 裸 ignore（无理由）的数目
bare=$(echo "$rows" | grep -c ',NO_REASON,' || true)
echo "--- bare #[ignore] (no reason): $bare ---"
if [ "$bare" -gt 0 ]; then
    echo "$rows" | grep ',NO_REASON,'
    echo
    echo "裸 #[ignore] 是永久债务的温床：没有理由就没人知道该不该恢复。"
    echo "请补 #[ignore = \"<原因>; review YYYY-MM\"]。"
fi
echo

# review date 覆盖率：reason 里带 20XX-XX 的算已排期
dated=$(echo "$rows" | grep -cE '"[^"]*20[0-9]{2}-[0-9]{2}[^"]*"' || true)
undated=$((total - dated))
echo "--- review date coverage ---"
echo "with review date (20XX-XX in reason): $dated"
echo "without review date:                  $undated"
if [ "$undated" -gt 0 ]; then
    echo
    echo "没有 review date 的 ignore 不会被任何人重新评估。"
    echo "季度审计流程见 docs/ci-test-debt.md。"
fi

# --stale 模式：只输出没有 review date 的行
if [ "$FILTER" = "stale" ]; then
    echo
    echo "--- entries without a review date ---"
    echo "$rows" | grep -vE '"[^"]*20[0-9]{2}-[0-9]{2}[^"]*"'
fi
