# 审计跟进报告（2026-08-13）

> 对应分支：`chore/audit-followup`
> 工具：`scripts/ci/ignored-tests.sh` + 本地 Python 分析脚本（见下）
> 基线：Rust 1.94.0，clippy clean

本文件是审计计划第 2 步的"摸底"产出，给后续 unsafe 收敛 / unwrap 治理 /
自更新签名 / ignored 测试台账提供数字依据。

---

## 1. unsafe 分布

### 1.1 全工作区总量

- 含 `unsafe` 关键字的 **1141 处**（全 grep 计数，含注释中的）
- 实际 `unsafe {}` 块 + `unsafe fn` + `unsafe impl/trait` + `unsafe extern`
  约 **~600 个真实不安全构造**（粗略估算，因为 1141 里许多是注释、文档、
  字符串字面量里的）

### 1.2 按 crate 分布（前 20）

| Crate | unsafe 关键字 | unsafe {} block | unsafe fn | impl/trait | extern |
|---|---:|---:|---:|---:|---:|
| `xai-grok-shell` | 364 | 332 | 5 | 1 | 0 |
| `xai-grok-workspace` | 120 | 90 | 0 | 1 | 2 |
| `xai-grok-pager` | 90 | 78 | 0 | 0 | 4 |
| `xai-crash-handler` | 65 | 46 | 10 | 0 | 9 |
| `xai-grok-update` | 53 | 52 | 0 | 0 | 0 |
| `xai-grok-pager-render` | 42 | 36 | 2 | 0 | 1 |
| `xai-tty-utils` | 38 | 35 | 0 | 2 | 1 |
| `xai-grok-tools` | 33 | 28 | 2 | 0 | 0 |
| `xai-grok-pager-bin` | 31 | 26 | 1 | 1 | 1 |
| `xai-system-power` | 29 | 21 | 0 | 3 | 5 |
| `xai-fast-worktree` | 26 | 25 | 0 | 0 | 0 |
| `xai-grok-sandbox` | 18 | 14 | 4 | 0 | 0 |
| `xai-sqlite-journal` | 6 | 6 | 0 | 0 | 0 |

### 1.3 按模式分类

| 模式 | 次数 | 说明 |
|---|---:|---|
| `std::env::set_var` | 355 | Rust 2024 起变 unsafe（线程安全 + 子进程） |
| `std::env::remove_var` | 316 | 同上 |
| `libc::*` | 450 | POSIX 系统调用 / 常量 |
| `std::ptr::*` | 61 | 裸指针操作 |
| `unsafe extern` | 26 | FFI 函数声明 |
| `unsafe fn` | 24 | 不安全函数定义 |
| `syscall` | 32 | 直接系统调用 |
| `transmute` | 5 | 类型强转（最危险） |
| `static mut` | 6 | 静态可变（需 unsafe 访问） |

### 1.4 关键发现

1. **大头是 env var**：`set_var` + `remove_var` 合计 ~671 处，占总 unsafe
   关键字的近 60%。这些都是真实的安全问题（环境变量全局共享，多线程
   读写有 data race；子进程继承也有语义问题），但与"裸指针/内存
   不安全"不是一个量级。
2. **`xai-grok-shell` 一个 crate 占 32%**：是主战场。里面 241/332 块
   是 env var 操作。
3. **高风险模式不多**：`transmute` 只有 5 处，`static mut` 只有 6 处。
   这几个值得先审计。
4. **`xai-grok-sandbox` 只有 18 处** —— 比预期少，说明沙箱的 unsafe
   边界控制得不错。

### 1.4.1 2026-10-02 实测刷新（`xai-grok-shell` env unsafe 收敛后）

上表是 2026-08 的静态计数，此后未刷新。按当前源码重算（`crates/codegen/*/src`
下 `*.rs` 的 `unsafe` 关键字命中行数）：

| 指标 | 2026-08 报告 | 2026-10-02 实测 |
|---|---:|---:|
| 全仓 `unsafe` 关键字行 | 1141 | 904 |
| `xai-grok-shell` `unsafe` 关键字行 | 364 | 49 |
| crate 内 `unsafe { std::env::set_var/remove_var }` | 332 块（含 241 处 env） | 1（`apply_process_env_strip`，本身是带 caller contract 的 `pub unsafe fn`） |
| 全仓 `std::env::set_var/remove_var` 直接调用 | ~671 | 417 |

`xai-grok-shell` 的收敛方式不是删检查，而是把环境写操作收进
`xai-grok-test-support::env` 的单一入口：一把进程级写锁 + 线程本地重入计数，
`set_var`/`remove_var`/`var_os` 各只有一个带 `// SAFETY:` 的块，成对的读改写用
`with_write_lock` 包住以保证原子。因此 1.4 第 1 条"大头是 env var"在其余 crate
仍然成立：剩余 417 处直接调用集中在 `xai-grok-workspace`（136 处 unsafe 关键字行）、
`xai-fast-worktree`（74）、`xai-grok-pager`（70）等，helper 已经放在共享 crate 里，
可以按 crate 继续搬。

同批还修掉一个真实缺陷：`initialize()` 从 `auth.json` 读到 API key、以及
`x.ai/setApiKey` 扩展原本都调用 `std::env::set_var("XAI_API_KEY", ..)`。那是与
其他线程 `std::env::var` 并发的进程全局写（正是 edition 2024 把 `set_var` 判为
unsafe 的原因），并且会把密钥写进之后每个子进程（shell 工具、hook、MCP server）
的 env block。现改为 `agent/auth_method.rs` 内 `RwLock` 保护的 runtime key cell
（`Unset` / `Present` / `Cleared` 三态；`Cleared` 只屏蔽 `XAI_API_KEY`，保留 legacy
变量，与原 `remove_var` 作用域一致），环境不再被运行时改写。

### 1.5 三类分类（初步）

| 类别 | 估算占比 | 说明 |
|---|---:|---|
| **A. 真正必要** | ~20% | FFI（libc/syscall/extern "C"）、asm、内存分配器、PTY raw mode |
| **B. 可消除** | ~65% | 主要是 `set_var` / `remove_var` —— 可用 `Command::env` 隔离、或用线程局部 + 一次性写入替代 |
| **C. 需要 SAFETY 注释** | ~15% | 已经是"合理的 unsafe"，但缺少 `// SAFETY:` 注释，审计者读起来累 |

> A/B/C 数字是粗估。精确分类需要逐文件审。

### 1.7 2026-09-30 P0 unsafe review (sandbox + tty-utils)

A bounded source review covered every current unsafe block/function/impl/extern in
`xai-grok-sandbox` and `xai-tty-utils`, including platform-gated code. It is not a
security sign-off: Windows/macOS compilation and reviewer approval remain open.

- Sandbox production sites reviewed: seccomp `prctl`/`syscall`, pre-exec child
  filter registration, runtime `getuid`, fd-relative sentinel opening/ownership,
  `statvfs`/`fstatvfs`, and fd-pinned `statx`. The invalid filter length guard
  remains bounded at 1..=4096. Review found that a public restricted-network
  installer accepted caller-supplied arbitrary BPF, so callers could install an
  allow-all policy while assuming the crate's network-deny guarantee. The
  low-level installer and BPF builder are crate-private within `child_net`; the
  sandbox crate has a private module and re-exports only the safe
  `restrict_child_network` / `_std` command configuration helpers. Their pre-exec
  closures fetch only the already-built fixed deny policy from a `OnceLock`; no
  downstream caller can provide an arbitrary filter to the restricted spawn API.
  All repository call sites (workspace envrc, MCP servers, pager command/hooks,
  shell terminal/tools/LSP and hook runner) were migrated to those wrappers and
  compile together under `cargo check --all-targets`.
  Namespace lockdown's unsafe contract now explicitly documents irreversible
  TSYNC effects on all threads and subsequent namespace/mount denials. Local
  `statvfs` output-pointer SAFETY comments were added.
- TTY utility production sites reviewed: Unix OOM/pre-exec/parent-death calls,
  pipe fd ownership, Windows Job Object handle operations and Send/Sync,
  process-resource FFI, and stderr descriptor ownership. `redirect_native_stderr`
  and `dup_tui_stderr` used `dup`, which made saved terminal stderr descriptors
  inheritable across exec. They now use `F_DUPFD_CLOEXEC`; the actual subprocess
  redirect regression asserts both descriptors carry `FD_CLOEXEC`. Windows
  `GetStdHandle` is validated before ownership conversion and duplicated into a
  new owned handle with `DuplicateHandle`, avoiding `File::from_raw_handle` on a
  possibly invalid or borrowed standard handle. Job Object Send/Sync contracts
  gained explicit safety rationale. A later check removed the debug-only
  `std::thread::current().id()` call from the pdeathsig pre-exec hook: lazy
  TLS initialization can allocate after fork, so same-thread arm/spawn remains
  a caller contract instead of a post-fork runtime check. Its subprocess
  regression exercises that pre-exec path.
- Evidence: private goal scratch `verification/mt6-sandbox-tty-check.log`,
  `mt6-sandbox-tests-final3.log`, `mt6-child-net-e2e-profile-gating.log`,
  `mt6-child-net-e2e-final-clean.log`, `mt6-sandbox-ignored-tests.log`,
  `mt6-tty-utils-tests-final.log`, `mt6-tty-stderr-cloexec-regression.log`,
  `mt6-pdeath-signal-safety-regression.log`, `mt6-tty-tests-final-portfolio.log`,
  `mt6-sandbox-tests-final-portfolio.log`, `mt6-p0-check-portfolio.log`,
  `mt6-p0-clippy-portfolio.log`, `mt6-p0-fmt-portfolio.log`,
  `mt6-child-net-e2e-final-ack.log`, `mt6-unsafe-clippy-final-ack.log`,
  `verification/final-rust-workspace-tests-after-api.log` (31,172 passed, 0 failed,
  490 ignored; CI thread stack configured).
  Linux sandbox/TTY tests, check, strict
  Clippy and fmt pass. The two sandbox integration tests normally marked ignored
  were explicitly invoked: their `subprocess_entry` probes pass under the
  default test namespace. The network filter blocks connect/send syscalls but
  permits fd `write` for stdio, so its network-denial contract assumes restricted
  children do not inherit already-connected network sockets. Auditing all restricted
  spawn sites and fd inheritance remains required before signoff. A whole-crate grep also shows TTY's test-only OOM env
  mutation is enclosed only by a local mutex used by its sibling tests; that
  does not serialize unrelated environment readers in parallel Rust tests. The
  mutating test itself does a short set→hook-select→remove sequence and passes,
  but moving the check to a helper subprocess is safer follow-up work.
  Windows/macOS paths were source-reviewed only; runner tests remain unverified. All in-repository restricted-spawn consumers were migrated to the root safe helpers; their combined all-target checks and child-spawn E2E gates passed. The subsequent full Rust workspace suite also passed with the CI-required 16 MiB test-thread stack (31,172 passed, zero failed, 490 ignored; private goal scratch `verification/rust-workspace-mt6-final.log`).

### 1.6 高价值审计 crate（先动的 5 个）

按"影响力 × 风险密度"排：

| 优先级 | Crate | 为什么先审 |
|---|---|---|
| P0 | `xai-grok-sandbox` | 安全边界。18 处，量小但意义大。一次审完即可标 "audited" |
| P0 | `xai-grok-auth` | 凭证处理。（实际上 grep 出来 0 unsafe —— 好现象。） |
| P1 | `xai-tty-utils` | 38 处，全是 PTY/raw mode。量适中，属于 A 类为主。 |
| P1 | `xai-crash-handler` | 65 处，10 个 unsafe fn + 9 个 extern，crash dump 路径。 |
| P2 | `xai-grok-shell` | 364 处量太大，但有 2/3 是 env var。先把 env var 模式做掉，数字直接砍 60%。 |

---

## 2. unwrap 分布

### 2.1 总量

| 分类 | 数量 | 占比 |
|---|---:|---:|
| 全部 `.unwrap()` | 27,696 | 100% |
| **生产代码**（排除 `#[cfg(test)]` 模块 + `tests/` + `benches/`） | **2,292** | **8.3%** |
| 测试代码 | 25,404 | 91.7% |

**关键结论**：92% 的 `.unwrap()` 在测试里，是合理的（测试 panic 正常）。
真正需要治理的是生产代码的 **2,292 处**，不是 27,280。

### 2.2 生产 unwrap 按 crate 前 20

| Crate | 生产 unwrap | 生产 expect | 生产 panic! | 生产占比 |
|---|---:|---:|---:|---:|
| `xai-grok-shell` | 1,270 | 534 | 62 | 55.4% |
| `xai-grok-pager` | 332 | 213 | 19 | 14.5% |
| `xai-grok-tools` | 222 | 233 | 15 | 9.7% |
| `xai-grok-workspace` | 118 | 63 | 3 | 5.1% |
| `xai-grok-config` | 71 | 0 | 0 | 3.1% |
| `xai-grok-sampling-types` | 50 | 22 | 37 | 2.2% |
| `xai-grok-test-support` | 46 | 38 | 21 | 2.0% |
| `xai-grok-sandbox` | 28 | 5 | 0 | 1.2% |

The historical production-unwrap count for `xai-grok-update` was 6. A focused
2026-09-25 change removed the retry-loop `last_err.unwrap()` in
`version::fetch_gcs_channel_pointer`, retaining the real network-failure path
and adding a structured fallback when no response/error was recorded. The five
progress-template unwraps were subsequently replaced with `progress_style_or_default`,
which logs a warning and returns the corresponding default style on invalid
format strings. An actual helper test covers that fallback; the update library
suite and strict Clippy pass. A current source review finds no production
`.unwrap()` matches in the updater source files examined; test-module unwraps
remain. This closes only the bounded updater progress-template and retry-error
slices, not the full B batch or the separate sandbox unsafe review.

This report's other production unwrap/unsafe counts and initial ignored-test
inventory are historical and have not been refreshed by this bounded review.
Use the Rust-aware CI ignored scanner and current audit tooling before citing
those workspace-wide figures; do not infer current counts from this report.
| `xai-fsnotify` | 26 | 4 | 1 | 1.1% |
| `xai-grok-pager-minimal` | 26 | 3 | 1 | 1.1% |
| `xai-circuit-breaker` | 12 | 1 | 0 | 0.5% |
| `xai-grok-sampler` | 10 | 6 | 11 | 0.4% |
| `xai-fast-worktree` | 8 | ... | ... | 0.3% |
| ... | ... | ... | ... | ... |

### 2.3 关键发现

1. **`xai-grok-shell` 一个 crate 占 55%** 的生产 unwrap。动它效果最大。
2. **shell 里 176 个是 `lock().unwrap()`**（Mutex 中毒）—— 这是 Rust
   惯用法，通常不算债务（lock 中毒就该 panic）。真正需要处理的是剩
   下的 ~1,100 个。
3. **`xai-grok-sandbox` 只有 28 个生产 unwrap** —— 安全边界的代码质
   量不错。
4. **`expect` 覆盖率**：shell 有 534 个 expect 对 1270 个 unwrap，意
   味着约 30% 已经有理由；pager 是 213/332 = 64%；tools 是 233/222 =
   105%（tools 里 expect 比 unwrap 多，好习惯）。

### 2.4 治理优先级

| 批 | Crates | 预估生产 unwrap 数 | 理由 |
|---|---|---:|---|
| A | `xai-grok-sandbox` + `xai-grok-auth` + `xai-grok-secrets` | 28 + 0 + 0 | 安全边界，量小，一次搞定 |
| B | `xai-grok-update` | 6 | 自更新路径，失败代价大，量极小——立刻就能 100% 清掉 |
| C | `xai-grok-shell`（lock 以外的 ~1,100 个） | 1,094 | 量大，但 impact 也最大 |
| D | `xai-grok-pager` | 332 | UI 层，panic 影响体验但不丢数据 |

> B 批 `xai-grok-update` 只有 6 个生产 unwrap —— 这是真的吗？grep 显示
> 562 个总 unwrap，但 556 个在测试里。是的，生产代码几乎都用了 proper
> error handling。这是个好消息。

---

## 3. ignored 测试

The initial inventory below is historical and superseded by the Rust-aware scanner
and the current Q4 CSV/baseline referenced in `docs/ci-test-debt.md` and
`docs/ignored-audit-2026q4-summary.md`. Do not cite its old counts as current.

首次运行结果已过期。2026-09-23 重跑统计时发现 `scripts/ci/ignored-tests.sh` 的 CSV 转义和尾部注释判定有缺陷；初步输出已撤回，详见 `docs/ignored-audit-2026q4-summary.md`。

### 3.1 当前状态

- 全工作区总量、裸属性数、review date 覆盖率：**待统计器修复后重新测量**。
- 直接源代码行扫描至少找到 218 条无 Rust reason 字符串的 `#[ignore]` 属性；该结果不是完整统计。
- fork 专属债务：见 `docs/ci-test-debt.md` 的独立口径表，不从无效快照推算。

### 3.2 分布（历史快照，已过期）

| Crate | ignore 数 | 主要类型 |
|---|---:|---|
| `xai-grok-pager` | 318 | PTY e2e (151) + scripted scenarios (32) + fork billing (16) + spawn-real-binary (13) + ... |
| `xai-grok-shell` | 147 | 主要是 upstream 本身的 e2e/long-running |
| `xai-fsnotify` | 22 | — |
| `xai-grok-tools` | 15 | — |
| `xai-grok-pager-pty-harness` | 10 | PTY 环境依赖 |
| `xai-file-utils` | 6 | 集成测试（S3 等） |

### 3.3 结论

- 当前仍有 **228 条裸 `#[ignore]`**，必须逐项补充准确原因，不能用统一占位文案批量掩盖。
- 403 条未注明 review date；需按 crate 与执行环境分批复核，决定恢复、加理由/期限或移除。
- 旧版 528 / 0 / 0 数字与当前实测不符，已由 Q4 CSV 快照取代。

---

## 4. 自更新签名

### 4.1 已落地（2026-09-23 复核）

- `crates/codegen/xai-grok-update/src/signature.rs` 提供 Ed25519 `verify_bytes` / `verify_file`；公钥通过编译期 `CHAOS_SIGNING_PUBLIC_KEY` 注入，未配置时使用占位公钥。
- `auto_update.rs` 下载后会读取 `.sig` 并调用 `verify_file`；是否强制验签由 `CHAOS_REQUIRE_SIG` 或 `require-sig` feature 决定。
- `release.yml` 会在私钥 secret 和公钥 variable 均配置时签名二进制；缺任一项时会跳过签名，workflow 仍允许发布未签名产物。
- `install.sh` 对存在的签名执行验证，签名缺失时仍可继续；`install.ps1` 同样仅在 Python、cryptography、公钥和签名文件均可用时验证，缺少条件时跳过。`install.bat` 调用 PowerShell 安装流程。
- **实测配置状态**：2026-09-23，`gh secret list` / `gh variable list` 未列出 `CHAOS_SIGNING_PRIVATE_KEY` 或 `CHAOS_SIGNING_PUBLIC_KEY`。这里只检查名称，不读取任何密钥值。

### 4.2 发布阻断项（P1，未完成）

- 生成并安全配置匹配的 `CHAOS_SIGNING_PRIVATE_KEY` secret 与 `CHAOS_SIGNING_PUBLIC_KEY` repository variable；执行真实签名和验证闭环。
- 为正式 release 构建启用 `require-sig`，并确认编译进二进制的公钥非占位值；未配置密钥时 release 必须失败，而非静默发布无签名二进制。
- 将安装脚本验签保证统一：支持平台应在存在签名配置时拒绝缺失/无效签名；若环境依赖（如 Windows Python/cryptography）不可用，明确安全策略并提供可验证实现。
- 增加端到端发布/安装测试：有效签名接受、签名不匹配拒绝、签名缺失拒绝、错误密钥拒绝；覆盖自动更新和安装脚本。

这些项目需要仓库维护者配置并保管供应链密钥。当前不能声称签名链路已满足发布门禁；完成前不得创建新 release tag。

### 4.3 格式与开关决策

- 当前使用裸 base64 Ed25519 签名 sidecar（不是完整 minisign 格式）；保持现状，减少格式迁移范围。
- `CHAOS_REQUIRE_SIG=0` 是绕过强制验签的环境开关。正式发布启用 `require-sig` 后，应评审是否保留紧急绕过；若保留，必须在安全文档中明确其影响并测试。

---

## 5. 下一步建议

按"投入产出比"排：

1. **B 批 unwrap 治理**（`xai-grok-update` 6 个 + sandbox 28 个）
   — 量小、位置重要、1 天内能全清
2. **unsafe P0 审计**（sandbox + tty-utils）—— 安全边界优先。2026-09-28 对 Linux seccomp installers 增加空程序和 >4096 指令长度拒绝的真实入口回归；这是局部边界修复，不代表逐项审计完成。
3. **签名集成进 auto_update**（把代码接上真实下载链路）
4. **shell crate env var unsafe 消除**（一次砍 ~60% 的 unsafe 数量）
5. **A 批 unwrap 治理**（shell 的 1,270 个 —— 大工程，分批）

---

## 6. 脚本 / 方法

- `scripts/ci/ignored-tests.sh`：ignore 统计
- unsafe 分布：bash one-liner（见 `crates/` 下 `grep -rE '\bunsafe\b'`）
- unwrap 生产/测试拆分：临时 Python 脚本（`/tmp/count_unwrap.py`，约 50
  行，用 brace-depth 跟踪 `#[cfg(test)]` 模块边界）。如需保留可归档到
  `scripts/dev/`。
