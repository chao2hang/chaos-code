# 上游移植计划 — 2026-08-13

> 本文件是 `chaos-upstream-sync` skill 工作流 B 的移植方案。先修工具暴露问题，再分批移植上游能力。所有事实均已用 `git`/`gh` 实测核对（2026-08-13）。

## 0. 背景与基线

| 项 | 值 |
|---|---|
| 本地 SOURCE_REV | `a51a1dc62fe20029ac39a665985bba78edbb870f` |
| 本地版本 | `0.2.136` |
| 本地 HEAD | `1159ab59` |
| 上游 main tip | `e5fd4816`（2026-08-12 `Synced from monorepo`） |
| 上游版本 | `1.0.3` |
| merge-base | `b13fa526` |
| 落后 | 2 commits（`be713136` + `e5fd4816`，均 monorepo 快照） |
| Chaos 领先 | 265 commits |
| 差异体量 | 701 文件 / +38605 / -11028（`git diff --stat upstream/main...HEAD` 实测，大版本重构，非小修） |
| 工作树 | 72 dirty 文件，其中 31 个与上游改动重叠 |

## 1. 第一优先：修复 `enter_plan_mode` / `exit_plan_mode` 无法调用

### 根因（2026-08-13 二次排查修正）

**主因不是 reasoning-only 重试，是 BYOK 代理在流式模式下丢弃"空参数"工具调用。**

实锤证据（会话 `019ff969` 的 chat_history + 日志 + 代理直连实验）：
- 用户发「测试进入计划模式」→ 模型 completion_tokens=61 但持久化消息为**空**（ttft=null，无任何内容/工具调用流出）。
- 用户手动进入计划模式后让模型退出 → 模型写完 plan.md，下一 loop completion_tokens=20（一次 `exit_plan_mode` 调用的大小）→ 再次被吞，会话卡在 Active（`plan_mode.json` 至今 `state: Active`）。
- **代理直连实验**（`glm-5.2-fp8` @ vLLM 0.23.0）：
  - `stream:true` + 空参数工具 → `finish_reason:"stop"`，**无任何 tool_calls delta**（3/3 必现）；
  - `stream:false` + 同一请求 → `tool_calls:[{name:"exit_plan_mode",arguments:"{}"}]` 正常；
  - `stream:true` + 带参数工具（read_file）→ 正常流式下发；
  - `stream:true` + schema 加可选 `note` 字段 → 模型总会填写，**调用完整到达**（修复验证 ✅）。
- 全套工具里零参数的只有三个：`enter_plan_mode`、`exit_plan_mode`、`scheduler_list` —— 所以症状精确表现为"计划模式进出全坏，其他工具都好"。

### 修复（已应用，尚未提交）

给三个零参数 Input 结构体各加一个可选 `note` 字段（`Option<String>`，serde default，schemars 进 schema），描述要求模型总是填写 → 流式 arguments 非空 → 代理正常下发：
- `enter_plan_mode/mod.rs` — `EnterPlanModeInput.note`（"what you intend to explore and plan"）
- `exit_plan_mode/mod.rs` — `ExitPlanModeInput.note`（"a one-line summary of the plan"）
- `scheduler/list.rs` — `SchedulerListInput.note`（同类 bug，一并修）

三个文件均已改动（`git diff --stat` 实测 +46/+35/+16 行），但**仍在工作树、未提交**。

上游（xAI 官方后端）不需要此 workaround，同步上游时注意保留（Chaos 专属适配）。

### 次因（保留此前诊断与修复）

reasoning-only 白名单问题确实存在（日志 12 次 `empty response (reasoning_only)` 重试），但重试 1-3 次后可恢复，不是工具丢失的主因。`request_task.rs` 的内容判断修复**仍然正确且必要**（思维模型首 turn 不再重试风暴，逻辑在 `request_task.rs:667-670`，以 `response.empty_reason() == ReasoningOnly` 内容判断替代模型名白名单），保留。该文件同样未提交（`git diff --stat` +23 行）。`xai-grok-sampling-types/src/conversation.rs` 配套改动 +39 行，也未提交。

**注意**：两个修复都需要**重新构建二进制**才能生效。`target/release/chaos` 当前时间戳 08-13 13:48，但 note-field 修复未提交，不能只看时间戳判断是否含修复 —— 验证靠重编 + 实测。

### 待用户确认

```bash
# 1. 先提交这 5 个修复文件（独立 commit，不与上游移植 entangle）
git add crates/codegen/xai-grok-tools/src/implementations/grok_build/enter_plan_mode/mod.rs \
        crates/codegen/xai-grok-tools/src/implementations/grok_build/exit_plan_mode/mod.rs \
        crates/codegen/xai-grok-tools/src/implementations/grok_build/scheduler/list.rs \
        crates/codegen/xai-grok-sampler/src/actor/request_task.rs \
        crates/codegen/xai-grok-sampling-types/src/conversation.rs
git commit -m "fix(sampler): plan_mode/scheduler 零参数工具流式丢失 + reasoning_only 重试风暴"

# 2. 类型检查 + sampling-types 单测
cargo check -p xai-grok-pager-bin -j 4
cargo test -p xai-grok-sampling-types --lib -j 2

# 3. 重建二进制，重启 chaos 会话后重试 enter/exit plan mode
cargo build -p xai-grok-pager-bin --release -j 4
```

卡死的会话 `019ff969`（plan_mode.json 仍 Active）：用 Shift+Tab/`/plan` 手动切出即可，或随新会话自然淘汰。

---

## 2. 上游能力移植（分批，按冲突成本排序）

### 批次 P0 — Changelog 文案（零冲突）

- 路径：`crates/codegen/xai-grok-shell/changelogs/1.0.{1,2,3}.{md,json}` + `CHANGELOG.md` 顶部
- 上游确实存在这三个版本的 md/json（已核对）；本地 `changelogs/` 只到 `0.2.x`，本地 `CHANGELOG.md` 顶条已是 `0.2.136`（国产 provider 预设等 Chaos 专属条目）。
- 理由：纯新增文件，不碰任何 Chaos 专属层。
- 操作：`git checkout upstream/main -- crates/codegen/xai-grok-shell/changelogs/1.0.{1,2,3}.{md,json}` 直接取。
- **注意**：版本号暂不对齐到 1.0.x（保持 0.2.136），只搬文案。`SOURCE_REV` 也不动。上游 1.0.x 的 md/json 只放 `changelogs/` 下**作参考**，不替换本地 `CHANGELOG.md` 的 `0.2.136` 顶条。

### 批次 P1 — 现有 crate 内新增文件（低冲突为主，需逐个接线）

> **纠正原文**：这些**不是新 crate**。`xai-chat-state`、`xai-grok-sandbox`、`xai-tty-utils`、`xai-grok-agent` 本地已存在为 workspace member；上游只是在这几个 crate 里**新增了个别文件**。所以无需改 workspace `members`、无需新 `Cargo.toml`，只需在每个 crate 的 `lib.rs`/`mod.rs` 加 `mod` 声明 + 接好调用方。

逐文件核对上游接线（mod 声明位置、行数、调用方、feature 门控、移植难度）：

| 文件 | 上游 mod 声明 | 行数 | 上游调用方 | feature 门控 | Chaos 专属冲突 | 移植难度 |
|---|---|---|---|---|---|---|
| `browser_verification.rs`（xai-grok-agent/prompt） | `prompt/mod.rs:3 pub mod` | 24 | `shell/.../prompt_build.rs:588,666`（`synthetic_user_rules()`） | 无 | 低 | **低**（但属行为 prompt，注入与否需有意识决定） |
| `allow_path.rs`（xai-grok-sandbox） | `sandbox/lib.rs:29 mod` | 122 | `profiles.rs:12 use ...normalize_allow_path` | 无（crate 整体 enforce 门控，本模块不门控） | 低 | **低** |
| `child_wait.rs`（xai-tty-utils） | `tty-utils/lib.rs:42 mod`+`43 pub use` | 121 | 仅 lib.rs re-export；**本地已有 `reap_killed_bounded`/`ProcessGroup`，可能重复** | 无 | 无 | **低–中**（留意与本地 reap 逻辑重叠） |
| `image_budget.rs`（xai-chat-state） | `chat-state/lib.rs:33 pub mod` | 556 | `actor/request_builder.rs`(3)、`actor/tests.rs`(2)、`shell/.../prepared_compaction_history.rs`、`session_compact_large_body_tests.rs` | 无 | 无 | **中**（真实预算逻辑+测试，非独立） |
| `startup_failure.rs`+`render.rs`（xai-grok-pager/app） | `app/mod.rs:51 mod`+`71 pub use` | — | `app/mod.rs`(8)、`pager-bin/main.rs:1902`、**`headless.rs`(4，保护层)** | 无 | 触及保护层 | **中–高** |

**建议**：首移植项选 `allow_path.rs`（122 行、单调用方 `profiles.rs`、不涉行为 prompt 争议、不碰保护层），验证「文件落地 + `mod` 声明 + 调用方接线 + 编译」流程跑通后再扩。`browser_verification` 虽最小（24 行），但它是注入 agent 行为的 synthetic user rule，需先决定 Chaos 是否启用该工作流，故不作为纯流程验证项。

> **不再推荐 `startup_failure` 作首选项**（纠正原文）：它上游接线点 13 处，散布在 `app/mod.rs`（8 处，本地已 dirty）、`pager-bin/main.rs`（1 处，本地已 dirty）、`headless.rs`（4 处，属 Chaos 保护堡垒）。Chaos 用 BYOK、无 xAI 登录，上游的「connect attempt / fallback」启动失败上下文与本地启动路径语义不同，直 `git checkout` 会撞本地 startup 流程 + 保护层，必须手工合流。**暂缓**，留到独立窗口。

### 批次 P2 — 测试文件剥离（机械迁移，大面积改路径）

上游把 `#[cfg(test)]` 内联块移到 `_tests.rs`（实测 `queue.rs` 上游 3068 行 / 本地 3076 行；`compaction_utils`、`app_view` 等同类）。
- 行为不变，纯文件路径迁移。
- 风险：与本地 31 个 dirty 文件中的测试文件正面相撞。
- **建议**：暂不做。等真正升级版本线时再统一迁移。

### 批次 P3 — 版本号对齐 0.2.136 → 1.0.3（高成本，暂缓）

- lockstep 改 `xai-grok-version` + workspace 所有 crate。
- 与 265 个 Chaos commit 在 31 个文件上相撞。
- **建议**：不在本次移植范围。留到独立的大版本升级窗口。

---

## 3. Chaos 专属保护层（移植时禁止整文件覆盖）

以下文件若被上游 diff 触及，必须手工合流或 skip：

1. **认证** — `xai-grok-shell/src/auth/`、`acp_session.rs` 的登录/OIDC 部分
2. **品牌** — `views/welcome/mod.rs`（logo / 中文）、`chaos` 命名、`xai-grok-pager-bin`
3. **中文 UI** — `slash/commands/`、`views/`、`diagnostics/`、`doctor_cmd/`、`headless.rs`
4. **更新日志产品化** — 欢迎页"更新日志"入口
5. **遥测弱化** — Chaos 侧禁用的 phone-home 不重新打开

> `headless.rs` 同时是 `startup_failure` 上游接线的落点之一（4 处），属本计划 P1 暂缓项的冲突热点。

## 4. 验证清单（每批移植后）

```bash
# 类型检查
cargo check -p xai-grok-pager-bin -j 4

# 相关单测
cargo test -p <crate> --lib -j 2 -- <module>

# L10n Guard（防中文回退，必须跑）
bash scripts/l10n-guard.sh --before main --after HEAD

# 正式二进制
cargo build -p xai-grok-pager-bin --release -j 4
./target/release/chaos --version
```

冒烟：启动 TUI → `/` 看中文 slash → 欢迎页 logo + 更新日志 → 设置/扩展弹窗。

## 5. 执行顺序

1. ✅ 修复 `glm-5` thinking-model 白名单 + note-field 流式丢失（已应用，未提交）
2. ⬜ **提交上述 5 个修复文件**（独立 commit，不与移植混）
3. ⬜ 选分支：续用 `origin/sync/upstream-20260811` 或新开 `sync/upstream-20260813`（本地已有 0811、0807 两条 sync 远程分支）
4. ⬜ 编译验证：`cargo check -p xai-grok-pager-bin -j 4` + `cargo test -p xai-grok-sampling-types --lib -j 2`
5. ⬜ 重建 `chaos` 二进制，重启会话，验证 `enter_plan_mode` / `exit_plan_mode` 可调用
6. ⬜ 批次 P0：搬 changelog 文案（参考用，不替 0.2.136 顶条）
7. ⬜ 批次 P1 首项：移植 `allow_path.rs`（122 行 + 1 调用方 `profiles.rs`），验证 mod 声明 + 编译
8. ⬜ L10n Guard + 冒烟
9. ⬜ 汇报：合入路径、刻意没合的、测试结果、未 push 的分支名

## 附：核对命令留痕

```bash
# 基线
cat SOURCE_REV                                   # a51a1dc6...
grep '^version' crates/codegen/xai-grok-version/Cargo.toml   # 0.2.136
git rev-parse HEAD                               # 1159ab59
git merge-base HEAD upstream/main                # b13fa526
git rev-list --count upstream/main..HEAD         # 265
git rev-list --count b13fa526..upstream/main     # 2
git diff --stat upstream/main...HEAD | tail -1   # 701 files, +38605/-11028

# P1 接线核对（每文件）
git show upstream/main:<lib.rs 路径> | grep -n <模块名>
git show upstream/main:<文件> | wc -l
git grep -n 'mod <名>\|use.*<名>\|::<名>' upstream/main -- '*.rs'
git show upstream/main:<Cargo.toml> | grep -in <名>
```
