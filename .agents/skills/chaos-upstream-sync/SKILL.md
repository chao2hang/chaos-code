---
name: chaos-upstream-sync
version: 1.0.0
description: "跟踪 GitHub 上 xai-org/grok-build（Grok Build）的更新，并安全移植到本仓库 chaos-code（Chaos 分支）。**仅当用户明确提到「上游 / grok-build / upstream」时才触发**；否则不触发。触发词：当用户说「同步上游」「看 grok build 更新」「移植上游改动」「merge grok」「对齐 SOURCE_REV」「上游有没有新版本」时使用。覆盖：查 releases/tags/commits/changelog、对照本地 SOURCE_REV 与版本号、分流可移植 vs Chaos 专属冲突、分批 cherry-pick/merge、编译与单测、更新日志。"
metadata:
  requires:
    bins: ["git", "gh", "cargo"]
  repos:
    upstream: "https://github.com/xai-org/grok-build"
    local: "https://github.com/chao2hang/chaos-code"
---

# Chaos ← Grok Build 上游同步

本 skill 指导 Agent **只读侦察上游 → 评估 → 经用户确认后移植**。默认**不要**自动 `git push` 或 force 覆盖 Chaos 专属改造。

## 何时用

| 用户意图 | 动作 |
|---|---|
| 上游有没有新提交 / 新版本 | **侦察**（只读） |
| 把某次 release / 某段 commit 合进来 | **评估 + 移植计划**，确认后执行 |
| 某上游 PR/文件在 Chaos 里有没有 | **对照路径** |
| 同步后编译挂了 | **排障**（见 references） |

**不要**在用户只是说「拉取更新」「pull」「同步」时触发本 skill——那通常指从本仓库自己的 `origin`（`chao2hang/chaos-code`）拉取，跑 `git pull` 即可，与上游无关。
**不要**在用户只是问「Chaos 怎么配模型」时触发本 skill（那是 `CHAOS.md`）。

## 固定事实（本仓库）

| 项 | 值 |
|---|---|
| 上游公开源 | `https://github.com/xai-org/grok-build`（默认分支 `main`） |
| 本项目 | `chaos-code`，产物二进制包名 **`xai-grok-pager-bin`**，可执行文件 **`chaos`**（`target/release/chaos`） |
| 上游对齐标记 | 仓库根 `SOURCE_REV` = 上游 monorepo 同步时的 commit SHA |
| 产品版本 | `crates/codegen/xai-grok-version/Cargo.toml` 的 `version`（与 pager/shell 等 lockstep） |
| 用户可见更新日志缓存 | `~/.grok/CHANGELOG.md` + `CHANGELOG.json`（CDN 失败时靠磁盘） |
| 仓库内 changelog 源 | `crates/codegen/xai-grok-shell/changelogs/<version>.{md,json}` + `crates/codegen/xai-grok-shell/CHANGELOG.md` |
| Chaos 专属说明 | 根目录 [`CHAOS.md`](../../../../CHAOS.md) |

**编译可执行文件务必：**

```bash
cargo build -p xai-grok-pager-bin --release
# 不是 -p xai-grok-pager（只编库，不重链 chaos 二进制）
```

## Chaos 专属层（移植时默认保护，禁止无脑覆盖）

上游改动若触及下列主题，**必须单独评估**，多数应 **skip / 手工重写**，不能整文件盖掉：

1. **认证 / 登录** — 去掉 Grok/OIDC 硬依赖、用户自带 `model_providers` / API Key（见 `CHAOS.md`）
2. **品牌与文案** — 中文 UI、`chaos` 命名、欢迎页 logo（`assets/logo/logo0{5,7}.txt`）
3. **更新日志产品化** — 欢迎页「更新日志」、中文 changelog 条目
4. **遥测 / 电话回家** — Chaos 侧弱化或禁用时，不要把上游强制遥测原样合入
5. **二进制 / 包名** — `xai-grok-pager-bin` 的 `[[bin]] name = "chaos"` 等

细节与路径清单见 [`references/chaos-fork-map.md`](references/chaos-fork-map.md)。

---

## 工作流 A — 侦察（默认只读，可直接做）

目标：回答「上游相对我们落后多少、有什么值得合」。

### A1. 本地基线

在 `chaos-code` 仓库根：

```bash
cat SOURCE_REV
git log -1 --oneline
rg -n '^version' crates/codegen/xai-grok-version/Cargo.toml
# 可选：本机缓存日志是否陈旧
head -20 "${GROK_HOME:-$HOME/.grok}/CHANGELOG.md" 2>/dev/null
```

### A2. 上游快照（优先 `gh`）

```bash
# 最新 main
gh api repos/xai-org/grok-build/commits/main \
  --jq '{sha:.sha, date:.commit.committer.date, msg:.commit.message|split("\n")[0]}'

# 最近 tags / releases（若有）
gh api repos/xai-org/grok-build/tags --jq '.[0:10]|.[]|{name,sha:.commit.sha[0:12]}'
gh api repos/xai-org/grok-build/releases --jq '.[0:5]|.[]|{tag:.tag_name, published:.published_at, name}'

# SOURCE_REV 对照：上游树里的 SOURCE_REV（monorepo 指纹）
gh api repos/xai-org/grok-build/contents/SOURCE_REV --jq '.content' | base64 -d; echo

# 公开 changelog 站点（产品文案，非 git）
# https://x.ai/build/changelog
```

也可用网页：`https://github.com/xai-org/grok-build/commits/main`。

### A3. 差分范围

若本地已配置 remote `upstream`：

```bash
git remote add upstream https://github.com/xai-org/grok-build.git 2>/dev/null || true
git fetch upstream --tags
git rev-parse SOURCE_REV   # 或 cat SOURCE_REV
git log --oneline "$(cat SOURCE_REV)..upstream/main" | head -50
git diff --stat "$(cat SOURCE_REV)"..upstream/main | tail -40
```

若本地 `SOURCE_REV` 与上游 git commit 不是同一对象（monorepo 指纹 vs 公开仓 commit），以：

1. 公开仓 `main` tip SHA  
2. 公开仓 `SOURCE_REV` 文件内容  
3. 本地 `SOURCE_REV`  

**三者对照**写进报告，不要假装能精确 `git merge-base`。

### A4. 输出侦察报告（固定结构）

向用户输出：

```markdown
## 上游侦察报告
- 本地 SOURCE_REV: …
- 本地版本 (xai-grok-version): …
- 上游 main tip: … (date)
- 上游 SOURCE_REV 文件: …
- 落后概况: N commits / 主要目录 …
- 高价值变更（建议移植）: …
- 高风险 / 与 Chaos 冲突: …
- 建议动作: 仅观察 | 分批 cherry-pick | 完整 merge 窗口
```

**侦察阶段禁止：** `git merge`、改业务代码、覆盖 `~/.grok/CHANGELOG*`（除非用户明确要求刷新缓存）。

---

## 工作流 B — 移植（需用户确认范围）

### B1. 确认范围

用一句话对齐，例如：

- 「只合 `xai-grok-pager` 里与 queue/interjection 相关的 commits」  
- 「对齐到上游 tag / 某 SHA」  
- 「只更新 changelogs 与 docs」

未确认范围时，只给计划，不动工作树（除非用户说「直接做」）。

### B2. 准备分支

```bash
git status -sb   # 脏树先 stash 或另开 worktree
git switch -c sync/upstream-$(date +%Y%m%d)
git fetch upstream
```

优先 **worktree** 隔离大合并：

```bash
git worktree add ../chaos-code-upstream-sync sync/upstream-$(date +%Y%m%d)
```

### B3. 合入策略（按冲突成本选）

| 策略 | 适用 |
|---|---|
| **cherry-pick 单 commit / 小串** | 明确 bugfix、单模块 |
| **path 限定 checkout** | 只要上游某目录/文件：`git checkout <sha> -- path` 后手工解决 |
| **merge upstream/main** | 长期对齐、冲突可接受；必须保留 Chaos 专属层 |
| **拒绝整仓覆盖** | 禁止 `git reset --hard upstream/main` 冲掉 Chaos |

Path 级移植步骤见 [`references/port-playbook.md`](references/port-playbook.md)。

### B4. 冲突解决原则

1. **行为 / 引擎 / 工具协议** — 倾向上游（修 bug、新能力）  
2. **登录、凭证、中文、logo、chaos bin** — 倾向本 fork  
3. **两边都改同一函数** — 先读两侧意图，**手工合流**，禁止「全选 theirs/ours」  
4. 冲突文件落在专属层时，在 PR/汇报里点名

### B5. 验证清单（移植后必做）

> **WSL / 低内存机器强制约束：** `cargo` 默认按 `nproc` 起 `rustc`，本 workspace 单个大 crate（`xai-grok-tools`、`xai-grok-workspace`、`xai-grok-pager`）编译峰值 2~4 GB，32 并发直接把 WSL2 的 `vmmem` 打爆，触发 OOM Killer 甚至冻结宿主 Windows。**下面所有 `--workspace` 级命令必须带 `-j 4`**，单 crate 迭代用 `-j 2`。可在 shell 里长期 `export CARGO_BUILD_JOBS=4`。若已经开始卡：`wsl --shutdown` 后在 `~/.wslconfig` 加 `[wsl2]\nmemory=12GB\nswap=8GB\nprocessors=6`。

```bash
# 类型检查（快）
cargo check -p xai-grok-pager-bin -j 4

# 相关单测（按改动收窄）
cargo test -p xai-grok-pager --lib -j 2 -- <module_filters>

# 全量自检（仅在必要时；务必带 -j 4）
cargo check --workspace -j 4
cargo clippy --workspace -j 4 -- -D warnings
cargo test  --workspace -j 4

# 正式二进制（用户可跑）
cargo build -p xai-grok-pager-bin --release -j 4
./target/release/chaos --version
```

冒烟：启动 TUI → `/` 看中文 slash 说明 → 设置/扩展弹窗 → 欢迎页 logo 与更新日志。

### B6. 版本与更新日志

若行为或用户可见面有变：

1. 若要对齐上游版本号：同步 `xai-grok-version` 与 workspace 内 lockstep crates（**整仓一致**，不要只改一个 package）  
2. 更新 `crates/codegen/xai-grok-shell/changelogs/<ver>.{md,json}` 与 `CHANGELOG.md` 顶部  
3. 刷新本机缓存（用户机器）：

```bash
cp crates/codegen/xai-grok-shell/changelogs/<ver>.md "${GROK_HOME:-$HOME/.grok}/CHANGELOG.md"
cp crates/codegen/xai-grok-shell/changelogs/<ver>.json "${GROK_HOME:-$HOME/.grok}/CHANGELOG.json"
```

4. 可选：更新根 `SOURCE_REV` 为本次对齐的上游指纹（与公开仓约定一致时）

### B7. 交付说明

完成后给用户：

- 合入了哪些上游 commit / 路径  
- 刻意 **没** 合哪些（及原因）  
- 测试命令与结果  
- 是否需要重启 `target/release/chaos`  
- 未 push 的分支名（默认不 push，除非用户要求）

---

## 工作流 C — 持续跟踪（可选）

```bash
# 每周或按需
gh api repos/xai-org/grok-build/commits/main --jq '{sha:.sha[0:12], date:.commit.committer.date, msg:.commit.message|split("\n")[0]}'
# 与本地 SOURCE_REV / 上次报告对比
```

可把结果追加到仓库笔记（若用户指定路径）；**不要**未经允许写远程 issue。

---

## 安全与禁止

- 不提交 / 不打印用户的 API Key、`~/.grok` 里的 token  
- 不默认 `git push --force`、不删除用户未跟踪文件  
- 不把上游「必须登录 xAI」逻辑在未讨论的情况下重新打开  
- CDN 更新日志 URL 可能与 fork 无关；**产品文案以仓库 changelogs + 本地缓存为准**

## 参考

- [`references/chaos-fork-map.md`](references/chaos-fork-map.md) — Chaos 专属路径与冲突热点  
- [`references/port-playbook.md`](references/port-playbook.md) — 具体移植命令与目录优先级  
- [`references/verify.md`](references/verify.md) — 编译 / 测试 / 冒烟清单  
- 上游仓库：https://github.com/xai-org/grok-build  
- 产品 changelog：https://x.ai/build/changelog  
- 本 fork 说明：仓库根 `CHAOS.md`
