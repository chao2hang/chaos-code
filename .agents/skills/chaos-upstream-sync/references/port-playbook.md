# 移植操作手册

## 0. 前置

```bash
cd /path/to/chaos-code
git status -sb
command -v gh && command -v cargo && command -v git
```

添加上游 remote（一次性）：

```bash
git remote add upstream https://github.com/xai-org/grok-build.git
# 或
git remote set-url upstream https://github.com/xai-org/grok-build.git
git fetch upstream --tags
```

## 1. 看「有什么新的」

```bash
# 上游 tip
git log -1 --oneline upstream/main

# 本地基线
cat SOURCE_REV
git log -1 --oneline

# 若 SOURCE_REV 恰好是某 git 对象：
git merge-base --is-ancestor "$(cat SOURCE_REV)" upstream/main && echo "ancestor OK" || echo "not a simple ancestor"

# 目录级热度
git diff --stat "$(cat SOURCE_REV)"..upstream/main 2>/dev/null | tail -50
# 失败则改用：
git diff --stat upstream/main~30..upstream/main
```

API 备选（无 fetch 时）：

```bash
gh api repos/xai-org/grok-build/commits/main --jq '{sha:.sha,msg:.commit.message|split("\n")[0]}'
gh api "repos/xai-org/grok-build/commits?per_page=20" --jq '.[]|{sha:.sha[0:10],msg:.commit.message|split("\n")[0]}'
```

## 2. 小范围：cherry-pick

```bash
git switch -c sync/pick-<topic>
git cherry-pick -x <sha1> <sha2>
# 冲突 → 按 chaos-fork-map 解决 → git add → git cherry-pick --continue
```

## 3. 只要某些路径

```bash
git switch -c sync/paths-<topic>
# 从上游某点取出文件到工作区（会覆盖工作区同名文件，先确认）
git checkout upstream/main -- crates/codegen/xai-grok-tools/src/some_module.rs
# 或
git restore --source=upstream/main -- crates/codegen/xai-grok-pager/src/app/queue_edit.rs
git add -A
git commit -m "port: <topic> from grok-build $(git rev-parse --short upstream/main)"
```

**禁止**对整棵 `crates/codegen/xai-grok-pager/src/views` 或整个 `auth` 做无差别 checkout。

## 4. 大范围 merge

```bash
git switch -c sync/merge-$(date +%Y%m%d)
git merge upstream/main --no-ff -m "merge: grok-build upstream"
# 大量冲突时：优先中止并改 cherry-pick
# git merge --abort
```

合并后立刻：

```bash
# 确认 Chaos 标记还在
rg -n "Chaos|自带模型|xai-grok-pager-bin" CHAOS.md crates/codegen/xai-grok-pager-bin/Cargo.toml | head
test -f crates/codegen/xai-grok-pager/assets/logo/logo07.txt
```

## 5. 上游单文件内容只读对比

```bash
gh api repos/xai-org/grok-build/contents/crates/codegen/xai-grok-pager/src/foo.rs \
  --jq '.content' | base64 -d | head
# 或
git show upstream/main:crates/codegen/xai-grok-pager/src/foo.rs | head
diff -u <(git show upstream/main:path) path
```

## 6. 移植后的版本与日志

```bash
# 版本（若要对齐）
# 编辑 crates/codegen/xai-grok-version/Cargo.toml 并同步其它 crate version 字段

# changelog 文件
# crates/codegen/xai-grok-shell/changelogs/X.Y.Z.md
# crates/codegen/xai-grok-shell/changelogs/X.Y.Z.json
# 更新 CHANGELOG.md 顶部

cp crates/codegen/xai-grok-shell/changelogs/X.Y.Z.md ~/.grok/CHANGELOG.md
cp crates/codegen/xai-grok-shell/changelogs/X.Y.Z.json ~/.grok/CHANGELOG.json
```

## 7. 二进制（易错点）

```bash
# ✅ 正确
cargo build -p xai-grok-pager-bin --release
ls -la target/release/chaos
# 验证字符串是否进了二进制（例）
python3 -c "print('退出应用' in open('target/release/chaos','rb').read().decode('utf-8','replace'))"

# ❌ 错误：只更新 rlib，不更新 chaos
cargo build -p xai-grok-pager --release
```

## 8. 常见失败

| 现象 | 处理 |
|---|---|
| cherry-pick 大量改登录 | `git cherry-pick --abort`，改 path 级只合引擎 |
| 测试断言英文 UI | 改测试期望为中文，或改测 key |
| 用户仍见旧 UI | 未重链 bin 或未重启进程 |
| 更新日志仍旧 | 刷新 `~/.grok/CHANGELOG.*`；CDN 可能盖回旧文案时可设离线缓存策略 |
