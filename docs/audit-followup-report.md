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

### 1.4.2 2026-10-02 第二次刷新（env 写入收敛推广到四个 crate 后）

同一天把 1.4.1 的 helper 继续搬到其余高频 crate。两组口径都给出，因为
"关键字行"会把 `// SAFETY:`、`unsafe fn`、FFI 文档一并算进来，容易高估进展：

| 指标 | 2026-10-02 第一次 | 2026-10-02 第二次 |
|---|---:|---:|
| `crates/codegen/*/src` `unsafe` 关键字行 | 904 | 763 |
| `crates/**/*.rs` 中 `unsafe {` 块数（`git grep -c 'unsafe {'`） | 768 | 586 |
| `crates/codegen` 内 `std::env::set_var/remove_var` 直接调用 | 417 | 222 |

作为参照，上游 `SOURCE_REV 72a61251fcff` 的 `unsafe {` 块数是 **1028**，所以这条
口径上本分支已经从 1028 降到 586。逐 crate（`unsafe {` 块数 / 裸 env 调用数）：

| crate | 本步前 | 本步后 |
|---|---|---|
| `xai-grok-workspace` | 112 / 110 | 4 / 1 |
| `xai-grok-update` | 27 / 30 | 1 / 0 |
| `xai-fast-worktree` | 70 / 32 | 55 / 4 |
| `xai-grok-pager` | 58 / 39 | 25 / 3 |

`xai-fast-worktree` 只降了 15 个块，因为它的 `unsafe` 主要在
`nfs/client.rs`（AF_UNIX socket、`poll`、`flock`）和 `api/gc/process_scan.rs`
（procfs / `proc_vnodepathinfo`）这类 FFI 上，与环境变量无关；关键字行只从 74 降到
72 也是同一个原因。剩余 222 处裸 env 调用里，有 5 处是**有意保留**的生产写入，
因为加锁 helper 在 `xai-grok-test-support`，对 library/bin 的非 `cfg(test)` 代码来说
它只是 dev-dependency，正常构建不可达。这 5 处各自带 `// SAFETY:` 或 `# Safety`
说明（pager 3 处、workspace daemon bin 1 处、fast-worktree 的跨 crate 测试辅助
`clear_auto_gc_env_for_test` 1 处）。要让它们也走同一入口，需要把写锁下沉到一个
独立叶子 crate，或给 `xai-fast-worktree` 加 optional dependency + feature；两条路都会
改变正常构建的依赖图，留作后续独立决定。

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
- Evidence: not-retained log `verification/mt6-sandbox-tty-check.log`,
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
  Windows/macOS paths were source-reviewed only; runner tests remain unverified. All in-repository restricted-spawn consumers were migrated to the root safe helpers; their combined all-target checks and child-spawn E2E gates passed. The subsequent full Rust workspace suite also passed with the CI-required 16 MiB test-thread stack (31,172 passed, zero failed, 490 ignored; not-retained log `verification/rust-workspace-mt6-final.log`).

### 1.6 高价值审计 crate（先动的 5 个）

按"影响力 × 风险密度"排：

| 优先级 | Crate | 为什么先审 |
|---|---|---|
| P0 | `xai-grok-sandbox` | 安全边界。18 处，量小但意义大。一次审完即可标 "audited" |
| P0 | `xai-grok-auth` | 凭证处理。（实际上 grep 出来 0 unsafe —— 好现象。） |
| P1 | `xai-tty-utils` | 38 处，全是 PTY/raw mode。量适中，属于 A 类为主。 |
| P1 | `xai-crash-handler` | 65 处，10 个 unsafe fn + 9 个 extern，crash dump 路径。 |
| P2 | `xai-grok-shell` | 364 处量太大，但有 2/3 是 env var。先把 env var 模式做掉，数字直接砍 60%。 |

### 1.8 2026-10-02 全仓实测（可复现口径）

> **绝对值已由 §1.9 取代（2026-10-04）。** 这一节量的是 `fa9c1358` 那棵树配**当时的**扫描器，
> §2.6 修掉扫描器两处口径错误之后 420 这个数就不成立了（同一棵树配当前扫描器是 429）。保留它
> 是因为「每个 crate 的 unsafe 主要来自什么」那张表仍然有效，§1.9 直接沿用。

上面 1.1–1.6 与 1.4.1/1.4.2 都是 `grep` 关键字行数，1.4.1/1.4.2 连命令都没有记录，
因此无法复现。2026-10-02 起统一用 `scripts/ci/panic-site-census.py`（口径见 §2.5）：
它把 unsafe 分成 `unsafe {}` 块 / `unsafe fn` / `unsafe impl` / `unsafe extern` 四类，
并且和 unwrap 一样区分"生产"与"任意构建"。

```
$ python3 scripts/ci/panic-site-census.py            # 逐 crate 表格
unsafe by kind, every build: 596 block, 25 fn, 7 impl, 26 extern
654 unsafe sites in all, 420 in production
```

1.3 的"模式分类"表（`libc::*` / `std::ptr::*` / `transmute` / `static mut`）不在这个
口径内 —— 那是一张**风险类型**表而不是**位置**表，仍只能当 2026-08 的快照读；生产
unsafe 的位置与数量以 §1.9 为准。下面这张 top 10 是当时的数，逐 crate 的「主要来自什么」
仍按原样保留，§1.9 沿用它的描述。

| Crate | 生产 unsafe | 主要来源 |
|---|---:|---|
| `xai-crash-handler` | 58 | crash dump 路径，`unsafe fn` + `extern` 集中 |
| `xai-fast-worktree` | 52 | AF_UNIX / `poll` / `flock` / procfs FFI |
| `xai-tty-utils` | 52 | PTY raw mode |
| `xai-system-power` | 29 | 平台 FFI |
| `xai-grok-pager-render` | 27 | 终端 ioctl / 终端能力探测 |
| `xai-grok-pager-bin` | 21 | 入口处的平台调用 |
| `xai-grok-foreign-sessions` | 21 | 外部会话集成 |
| `xai-grok-tools` | 19 | 进程 spawn |
| `xai-grok-sandbox` | 19 | 安全边界（1.6 的 P0 项，量级没变） |
| `xai-grok-shared` / `xai-grok-pager` | 17 / 17 | |

与 1.2 的 2026-08 表对比要小心口径：1.2 数的是**关键字行**（把 `// SAFETY:` 注释、
文档、字符串里的 `unsafe` 也算进来），census 数的是**真实构造**，且只算被某个 crate
root 可达的文件。两个方向都有偏差，所以只有"同一口径下的前后对比"有意义，跨口径
对比没有意义 —— 这正是 §2.5 存在的理由。

### 1.9 2026-10-04 位置表刷新（生产 unsafe 的当前 top 10）

§1.8 那张表量的是 `fa9c1358` 那棵树配**当时的**扫描器：把那条命令放回那棵树重跑，输出的最后
两行与 §1.8 逐字相同（`654 unsafe sites in all, 420 in production`，`596 block, 25 fn, 7 impl,
26 extern`）。§2.6 修掉扫描器的两处口径错误之后，420 这个数就不再成立。这一节给的是**当前
扫描器**下的同一张表，并把 420 到今天的差拆成两笔分别量：纯口径变动，与代码真变动。

同一命令在当前树上的实测（`8a52ff0a`）：

```
$ python3 scripts/ci/panic-site-census.py            # 逐 crate 表格
unsafe by kind, every build: 599 block, 25 fn, 7 impl, 26 extern
657 unsafe sites in all, 428 in production
```

生产 unsafe 的 top 10。"两天前"这一列是把**当前**扫描器放回 `fa9c1358` 那棵树量的，为的是让
这一列与左列只差代码；它不等于 §1.8 里的同名数字，那一个还叠着扫描器的偏差（`xai-grok-pager`
在 §1.8 是 17，配当前扫描器是 21）。「主要来源」沿用 §1.8 逐 crate 核对过的描述：

| Crate | 生产 unsafe | 两天前 | 主要来源 |
|---|---:|---:|---|
| `xai-crash-handler` | 58 | 58 | crash dump 路径，`unsafe fn` + `extern` 集中 |
| `xai-tty-utils` | 52 | 52 | PTY raw mode |
| `xai-fast-worktree` | 52 | 52 | AF_UNIX / `poll` / `flock` / procfs FFI |
| `xai-system-power` | 29 | 29 | 平台 FFI |
| `xai-grok-pager-render` | 27 | 27 | 终端 ioctl / 终端能力探测 |
| `xai-grok-pager` | 21 | 21 | |
| `xai-grok-pager-bin` | 21 | 21 | 入口处的平台调用 |
| `xai-grok-foreign-sessions` | 21 | 21 | 外部会话集成 |
| `xai-grok-sandbox` | 19 | 19 | 安全边界（1.6 的 P0 项，量级没变） |
| `xai-grok-tools` | **18** | 19 | 进程 spawn；全仓 top 10 里只有这一行动过 |

420 到今天的 428 是两笔方向相反的变化叠加出来的，必须分开记：

| 变化 | 数值 | 性质 |
|---|---:|---|
| 420 → 429 | +9 | 纯口径，代码一行未动 |
| 429 → 428 | −1 | 代码真减一处 |

**+9 那笔是 §2.6 的扫描器修正。** 缺陷 2 把写着 `not(test)` 的属性当成测试门控，修好之后 10 处
unsafe 第一次被算进生产；缺陷 1（字符串字面量在第一个 `"` 处结束）修好之后又有 1 处从生产里退出。

**−1 那笔是代码真的少了一处。** `45628088` 把持久 shell 的状态读端改成 `AsyncFd` 驱动，
`shell_state.rs` 里 `spawn_blocking` 中那处 `unsafe { File::from_raw_fd(fd.as_raw_fd()) }` 随之
删掉，没有新的 unsafe 顶上来。`--list` 对得上：该 crate 的 `shell_state.rs` 由 4 处变 3 处，
`cgroup.rs`、`static_shell.rs`、`terminal.rs`、`persistence.rs` 一处没动。

把当前扫描器同时放到那棵树与今天的树上逐 crate 比对，上面十行**只有 `xai-grok-tools` 变了**，
其余九个 crate 的生产 unsafe 一个不多一个不少。其余三列同样纹丝不动：生产 `.unwrap()` 300、
`.expect()` 594、`panic!` 130，与 §2.6 表里「两处都修」那一行完全一致。会动的只有「任意构建」
的 `.unwrap()` 总数，从 31 888 涨到 32 034；生产那一列没变，所以这净增的 146 处全部落在
`cfg(test)` 之内。

`--list <crate>` 打印某个 crate 的每一个生产位点（`path:line kind`），上表任何一个数都能这样
摊开核对；`xai-grok-tools` 今天这 18 处分布在 `cgroup.rs`（8）、`shell_state.rs`（3）、
`static_shell.rs`（3）、`terminal.rs`（3）、`persistence.rs`（1）。

---

## 2. unwrap 分布

> 2.1 与 2.2 是 2026-08 的手工口径（口径本身没有记录下来，因此无法复现，也无法与
> 后续任何一次测量比较）。2026-10-02 起以 **§2.5** 为准。

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

### 2.5 2026-10-02 全仓实测（可复现口径）

2.1–2.4 无法复现：2,292 / 27,696 这两个数没有任何记录说明"排除 `#[cfg(test)]`"
到底是怎么判定的，因此既不能重跑，也不能和以后任何一次测量比较。本节换一个
写进口径的扫描器，并把当天的数字固化成 CI 棘轮。

**口径**（`scripts/ci/panic-site-census.py`）：

1. 扫描对象：`crates/**/*.rs`。
2. 每个文件先做**保位空白化**：注释、字符串、字节串、raw string、字符字面量
   的内容替换成空格（长度不变，行号不变），因此 `"unwrap"`、`// expect(` 之类
   不再命中，而 `.unwrap()` 的偏移仍然对得上原文。
3. **是否属于测试**按三条规则判定，任一成立即算测试：
   - 位置：`tests/` / `benches/` / `examples/` 目录下的文件；
   - 整文件门控：文件级 `#![cfg(test)]`，或整个文件体被一个 `cfg(test)` 属性包住；
   - 区间门控：命中点落在某个 `cfg(...)` 属性区间内，且该 `cfg` 表达式**要求**
     `test`（2026-10-04 之前的口径是"表达式里出现 `test`"，那是错的，见 §2.6）。
     `all` 继承任一分量的要求，`any` 只继承全部分量的要求，`not(..)` 不要求任何东西：
     因此 `#[cfg(all(test, unix))]` 是测试门控，
     `#[cfg(any(target_os = "linux", all(unix, test)))]` 与
     `#[cfg_attr(test, allow(dead_code))]` 都不是。`#[cfg(test)] #[path = "x_tests.rs"] mod tests;`
     这种本仓最常见的形状会被解析成"声明边被 test 门控 + 目标文件整文件算测试"。
4. **可达性**：只有被某个 crate root 通过 `mod` 链可达的文件才计入。crate root 取
   cargo 的约定（`src/lib.rs`、`src/main.rs`、`src/bin/*.rs`、`src/bin/*/main.rs`、
   `build.rs`、`tests/*.rs`、`tests/*/main.rs`、`benches/`、`examples/`）加上
   `Cargo.toml` 里 `[[bin]]/[[test]]/[[example]] path = "….rs"` 显式声明的文件。
   子模块查找遵循 rustc 规则：crate root 的 `mod x;` 找 `../x.rs`，普通模块的
   `mod x;` 找 `<stem>/x.rs`，`mod.rs` 用父目录。测试门控的边不参与"生产可达"，
   但参与"任意构建可达"。
5. 计数项：`.unwrap()`、`.expect(`、`panic!/unreachable!/todo!/unimplemented!`、
   `unsafe {}` 块 / `unsafe fn` / `unsafe impl` / `unsafe extern`。

**复现命令**：

```
$ python3 scripts/ci/panic-site-census.py                    # 逐 crate 表格 + 汇总
$ python3 scripts/ci/panic-site-census.py --json             # 机器可读
$ python3 scripts/ci/panic-site-census.py --list CRATE       # 某 crate 的逐文件明细
$ python3 scripts/ci/test-panic-site-census.py               # 生成 fixture 自测（非空转证明）
$ python3 scripts/ci/panic-site-census.py --check-baseline scripts/ci/panic-site-baseline.tsv
$ python3 scripts/ci/panic-site-census.py --check-uncompiled scripts/ci/uncompiled-sources.txt
```

后两条已进 `.github/workflows/ci.yml` 的 guards step：生产位点数**只允许减少**
（变多即 CI 失败，变少会打印出来），"没有任何构建会编译的源文件"集合**一字不许变**
（新增文件、或某条记录其实又被编译了，都失败）。

**2026-10-02 实测**（完整输出与时间戳见 `docs/verification/panic-site-census-2026-10-02.txt`）：

| 指标 | 2.1 的 2026-08 口径 | 2026-10-02 口径 |
|---|---:|---:|
| 全部 `.unwrap()` | 27,696 | 31,621 |
| 生产 `.unwrap()` | 2,292（8.3%） | **351（1.1%）** |
| 生产 `.expect()` | 未统计总量 | 596 |
| 生产 `panic!/unreachable!/todo!/unimplemented!` | 未统计总量 | 132 |
| unsafe 位点（任意构建 / 生产） | 未统计总量 | 654 / **420** |

生产位点最多的 crate（前 10，全部为**生产**数）：

| Crate | unwrap | expect | panic! | unsafe | 全量 `.unwrap()` |
|---|---:|---:|---:|---:|---:|
| `xai-grok-pager` | 57 | 133 | 30 | 17 | 5,476 |
| `xai-grok-shell` | 46 | 116 | 29 | 13 | 8,167 |
| `xai-grok-test-support` | 56 | 40 | 22 | 7 | 172 |
| `xai-grok-workspace` | 47 | 26 | 3 | 3 | 2,810 |
| `xai-grok-tools` | 16 | 122 | 8 | 19 | 3,397 |
| `xai-grok-sandbox` | 42 | 4 | 1 | 19 | 225 |
| `xai-grok-pager-render` | 8 | 8 | 4 | 27 | 438 |
| `xai-computer-hub-sdk` | 1 | 45 | 1 | 0 | 50 |
| `xai-grok-pager-pty-harness` | 0 | 15 | 3 | 2 | 27 |
| `xai-grok-agent` | 5 | 9 | 0 | 0 | 818 |

`xai-grok-test-support` 的"生产"数看着反常（172 个 unwrap 里 56 个算生产），因为它
本身就是测试基础设施：它的 `src/` 不是 `cfg(test)`，但只作为 dev-dependency 被链接。
读表时要把它当成"生产形态的测试代码"，2.4 的治理优先级不应把它排进去。

2.3 的几条结论在新口径下大多不再成立：shell 不再是"55% 的生产 unwrap"，全仓生产
unwrap 只有 351 个，其中 `xai-grok-pager` + `xai-grok-shell` 合计 103 个（29%）；
`expect` 现在整体多于 `unwrap`（596 vs 351），说明历史上"该写理由的地方基本写了"。

**扫描器本身发现的、比数字更重要的一类问题**：12 个 `.rs` 文件**没有任何构建会编译
它们** —— 没有 crate root 能通过 `mod` 链走到。测试文件处于这个状态就等于"有人以为
自己有这份覆盖"。清单固化在 `scripts/ci/uncompiled-sources.txt`：

```
crates/codegen/xai-grok-pager/src/app/dispatch/tests/usage_partial_failure.rs
crates/codegen/xai-grok-pager/src/scrollback/blocks/credit_limit.rs
crates/codegen/xai-grok-pager/src/slash/commands/fallback.rs
crates/codegen/xai-grok-pager/src/views/dashboard/render_tests.rs        (4333 行)
crates/codegen/xai-grok-pager/src/views/dashboard/state_tests.rs         (6101 行)
crates/codegen/xai-grok-pager/src/views/shortcuts_help_tests.rs          (2316 行)
crates/codegen/xai-grok-shell/src/session/acp_session_impl/incomplete_end_turn.rs
crates/codegen/xai-grok-shell/src/session/acp_session_impl/selective_compaction.rs
crates/codegen/xai-grok-shell/src/session/dcp_config.rs
crates/common/xai-grok-compaction/src/strategies/mod.rs
crates/common/xai-grok-compaction/src/strategies/deduplication.rs
crates/common/xai-grok-compaction/src/strategies/purge_errors.rs
```

其中 `/adhd` 是**已确认的用户可见回归**并已修好：`slash/commands/adhd.rs`（106 行）
随 release `480aa28a "release 0.2.123: #15 #16 /fallback /adhd"` 发出去过，但
`commands/mod.rs` 里从来没有 `mod adhd;`（`git log -S"mod adhd"` 对该文件返回空），
所以该命令在发布构建里根本不存在；同时 `[adhd].enabled` 仍被
`xai-grok-shell/src/agent/config.rs` 解析、并在 `xai-grok-pager/src/acp/mod.rs` 注入。
现已补上 `pub mod adhd;` 与 `builtin_commands()` 里的注册，并加两个守护测试：
`every_command_file_in_this_directory_is_declared()`（目录里每个 `*.rs` 必须被声明，
例外要在 `DELIBERATELY_UNDECLARED` 里写明理由，例外若指向已不存在的文件也失败）与
`the_adhd_toggle_is_reachable_through_the_registry()`。

`/fallback` **故意没有复活**：全仓没有任何代码解析 `[fallback] models`，采样器没有
fallback 链，声明它只会 advertised 一个静默无操作的命令；理由写在那个例外表里，
TODO 也登记了。`xai-grok-compaction/src/strategies/` 三个文件的"确实没被编译"是
经验证而不是推断的：给 `mod.rs` 追加 `fn deliberately_broken(({{{ not rust` 之后
`cargo check -p xai-grok-compaction --offline` 依然干净通过（文件随后按字节还原）。

**扫描器的非空转证明**：对固定 fixture（`scripts/ci/test-panic-site-census.py` 生成的
`demo` crate，含 `src/bin/tool.rs` 兄弟子模块、`#[path]` 声明、`cfg(all(test, unix))`、
`cfg_attr(not(test), …)`、`#![cfg(test)]` 整文件、`tests/common/mod.rs`、以及一个
故意不被声明的文件）逐个注入错误实现，每次都必须有检查失败：

| 注入的错误口径 | 失败的检查数 |
|---|---:|
| 不承认任何文件是 crate root | 9 |
| 忽略"哪些 `mod` 边被 test 门控" | 5 |
| 把每个文件都当作 crate root | 5 |
| 把"无构建编译"的文件也计入位点 | 4 |
| 同一文件里只要有 test 声明就把整个文件的声明都判为 test 门控 | 4 |
| 忽略普通（非 test 门控）声明 | 4 |

### 2.6 2026-10-04 口径修正：扫描器两处判错，一处凭空造出 69 个生产位点，一处把写着 `not(test)` 的位点判成测试代码

2026-10-04 那批 Windows 修复触发了 census 门禁报"生产位点变多"，追下去发现两个新位点根本在
`#[cfg(test)] mod tests` 里 —— 也就是说门禁红是因为扫描器**看不见**那个门控。顺着这条线查出两处
口径错误，两处都会改变上面所有数字，因此这一节取代 §1.8 与 §2.5 表里的绝对值（**口径**没变，变的是
实现；两张表的"相对结论"仍然成立，但 §2.4 的 A 批按新数已经不存在）。今天的绝对位置表见 §1.9。
完整过程、逐位点核对与变异证据见
`docs/verification/panic-site-census-2026-10-04.log`。

**缺陷 1：字符串字面量在第一个 `"` 处结束。** Rust 里反斜杠转义下一个字符，因此以 `\"` 收尾的字面量
在扫描器眼里没有结束，真正的收尾引号被留在待扫描文本里，转而**开启**第二个字面量，一路吞到下一个引号，
把它覆盖的字节全部空白化。被覆盖的如果是一个 `#[cfg(test)]`，那个测试模块就失去了唯一标记，里面每个
panic 都被判成生产；被覆盖的是 `{` 时，闭合测试模块的括号走查直接失衡。触发它的真实代码是
`out.push_str("\\\"");`。

**缺陷 2：判定"这条 `cfg` 是不是测试门控"用的是"表达式里出现 `test`"。** 正确的问题是
"`test` 是不是**被要求**"：`all` 继承任一分量的要求，`any` 只继承全部分量的要求，`not(..)` 不要求任何
东西，`cfg_attr` 因为**永远**构建该条目而永远不是门控。最刺眼的一条实证是
`xai-fast-worktree/src/nfs/mod.rs:271` 的 `#[cfg(all(target_os = "linux", not(test)))]` —— 这条属性
自己写着 `not(test)`，旧规则却因为它"提到了 test"而把里面的 `unsafe { libc::access(...) }` 判成测试代码。

**修正后的实测**（同一条命令，同一棵树）：

```
$ python3 scripts/ci/panic-site-census.py
    TOTAL   300  594  130  429   32005
    32005 .unwrap() calls in all, 300 of them outside any cfg(test) span (0%);
    658 unsafe sites, 429 in production.
```

下表三行跑的是**同一棵 2026-10-04 的树**，唯一变量是扫描器；最后一行就是门禁里的基线。

| 扫描器状态 | unwrap | expect | panic | unsafe |
|---|---:|---:|---:|---:|
| 两处都未修 | 353 | 596 | 132 | 420 |
| 只修字面量转义 | 294 | 589 | 130 | 419 |
| 两处都修 | **300** | **594** | **130** | **429** |

（§1.8 与 §2.5 的 351 是在 2026-10-02 那棵树上量的；与第一行差的那 2 个 `.unwrap()` 来自本批新增的
`startup_trace.rs:296,297`，旧扫描器把它们判成生产，修好之后判成测试。）

**两处缺陷的作用方向不同，必须分开记。** 按 `path:line:kind` 逐位点比对：

| 修复 | 新计入生产 | 不再计入生产 | 净变化 |
|---|---:|---:|---:|
| 缺陷 1（字面量转义） | 8 个 `.expect()` | 59 unwrap + 15 expect + 2 panic + 1 unsafe = 77 | **−69** |
| 缺陷 2（`cfg` 规则） | 6 unwrap + 5 expect + 10 unsafe = 21 | 0 | **+21** |

那 59 里有一行是 `xai-grok-shell/src/agent/config_model_override_parse.rs:879`，同一行两个
`.unwrap()`，按行去重的清单会少算一个 —— 这类"看起来是 58 其实是 59"的差一位，只有逐位点比对能发现。

还有一个数字方向值得单独记：旧扫描器在全树里只认出 **31,752** 个 `.unwrap()`，修好之后是 **32,007**，
unsafe 位点从 657 到 658。也就是说缺陷 1 不只是把测试位点算成生产，它同时**抹掉了** 255 个调用 ——
一个既会虚报又会漏报的扫描器，它的任何一个数字都不能单独引用。

**缺陷 1 修好后才第一次被算进生产的 8 个位点，逐条读过**（全部是 `.expect()`）：

| 位点 | 为什么它确实在发布构建里 |
|---|---|
| `xai-grok-pager/src/app/acp_handler/session_notification.rs:235`、`xai-grok-pager/src/scrollback/text_selection.rs:1259` | 前者是 `app.agents.get_mut(&id).expect("find_session_match returned an existing AgentId")`，后者是模块级 `static URL_RE: LazyLock<Regex>` 的 `.expect("URL regex must compile")`；两条都在普通代码里，此前是被吞掉的引号连带把上面的 `#[cfg(test)]` 标记一起吃掉了 |
| `xai-grok-sandbox/src/deny/glob.rs:370,521` | `matches.lock().expect(..)` 与 `matches.into_inner().expect(..)`，所在函数门控是 `#[cfg(all(feature = "enforce", target_os = "linux"))]`（`insert_match` 与被 `393` 行门控的收集函数），`mod tests` 从 527 行才开始；同文件另外 46 个位点确实全在 `mod tests` 内，所以这个 crate 同时是"多算"和"漏算"的样本 |
| `xai-grok-shared/src/clipboard.rs:1656,1685,1698,1760` | `#[cfg(not(target_os = "macos"))] mod platform` 里的 `run_pipe_in`、`run_capture_out_with_status`（两处）、`read_x11_primary_with_tools`，分别是 `argv.split_first().expect("argv non-empty")` 两次、`child.stdout.take().expect("stdout piped")`、`.expect("X11 PRIMARY tool must define read argv")`。门控属性里**根本没有 `test`** —— 所以这 4 个与缺陷 2 无关，纯粹是被缺陷 1 吞掉了标记 |

**缺陷 2 修好后才第一次被算进生产的 21 个位点，逐条读过**：

| 位点 | 门控与内容 |
|---|---|
| `xai-fast-worktree/src/nfs/mod.rs:278` | `#[cfg(all(target_os = "linux", not(test)))] fn grove_fuse_ready()` 里的 `unsafe { libc::access("/dev/fuse", W_OK) }`。属性明写 `not(test)`，旧规则仍判为测试 |
| `xai-grok-pager/src/app/mod.rs:1260,1268,1294,1308` | `#[cfg(any(windows, test))] mod win_native_selection` 里 `#[cfg(windows)] mod imp` 的 `unsafe extern "system"` 声明与 `GetStdHandle` / `GetConsoleMode` / `SetConsoleMode` 调用；审计里此前没有任何一个 unsafe 数字包含这段控制台模式 FFI |
| `xai-grok-env/src/lib.rs:158,159,165,173,174` | `#[cfg(any(test, feature = "test-support"))] struct EnvVarGuard` 的 5 处 `unsafe { set_var / remove_var }` |
| `xai-grok-bundle/src/lib.rs:519,521,522`、`xai-grok-shell/src/agent/models/startup_prefetch.rs:218,253,259` | `#[cfg(any(test, feature = "test-support"))]` 的 helper，`.unwrap()` |
| `common/xai-circuit-breaker/src/clock.rs:47`、`xai-grok-workspace/src/handle.rs:5086,5093`、`.../session/tool_config.rs:548,572` | `any(test, feature = "test-hooks"/"test-support")` 形状的 helper，`.expect()`。它们算"生产"是口径**故意**保守的结果：feature 一开就编译进去，扫描器无权假设没人开 |

那 3 个 `startup_prefetch.rs` 位点（`clear_for_tests`、`inject_with_origin_for_tests`、
`inflight_for_tests`）在 2026-10-05 被整批清掉（同文件 16 个生产 unwrap 全部去除，
`xai-grok-shell` 生产 unwrap 39 → 23，见 `docs/verification/lock-poison-prefetch-2026-10-05.log`），
上表记的是 2026-10-04 那棵树，照抄行号会找不到东西。

反过来被**正确移出**生产的（缺陷 1 之前凭空算进来的）：`xai-grok-sandbox/src/deny/glob.rs` 46 处
（41 unwrap / 4 expect / 1 panic）与 `deny/mod.rs:417`、`xai-grok-workspace/src/permission/auto_mode/mod.rs`
14 处与 `bash_command_splitting.rs` 5 处、`xai-grok-shell/src/agent/config_model_override_parse.rs`
3 处、`xai-grok-tools/src/reminders/task_completion.rs` 4 处，全部在 `mod tests` 内。落到 crate 级别，
与门禁基线（`HEAD` 提交的 `panic-site-baseline.tsv`）逐行比对：

| Crate | 基线（2026-10-02） | 现在 | 差 |
|---|---|---|---|
| `xai-grok-sandbox` | 42 / 4 / 1 / 19 | **0** / 2 / 0 / 19 | −42 unwrap、−2 expect、−1 panic |
| `xai-grok-workspace` | 47 / 26 / 3 / 3 | 35 / 23 / 3 / 3 | −12 unwrap、−3 expect |
| `xai-grok-tools` | 16 / 122 / 8 / 19 | 16 / 118 / 8 / 19 | −4 expect |
| `xai-grok-shell` | 46 / 116 / 29 / 13 | 46 / 116 / 28 / 13 | −1 panic |
| `xai-grok-pager` | 57 / 133 / 30 / 17 | 57 / **135** / 30 / **21** | +2 expect、+4 unsafe |
| `xai-grok-shared` | 0 / 4 / 0 / 17 | 0 / **8** / 0 / 17 | +4 expect |
| `xai-grok-bundle` | 0 / 0 / 0 / 0 | **3** / 0 / 0 / 0 | +3 unwrap |
| `xai-grok-env` | 0 / 0 / 0 / 0 | 0 / 0 / 0 / **5** | +5 unsafe |
| `xai-circuit-breaker` | 0 / 0 / 0 / 0 | 0 / **1** / 0 / 0 | +1 expect |
| 合计 | 351 / 596 / 132 / 420 | **300 / 594 / 130 / 429** | −51 / −2 / −2 / +9 |

**结论层面的影响，三条**：

1. §2.4 的 A 批理由是"`xai-grok-sandbox` 28 个生产 unwrap，安全边界，量小"。§2.5 实测该 crate 是 42
   个，修正之后是 **0** 个 —— 那 42 个全部在 `mod tests` 里。A 批剩下的 auth 与 secrets 本来就是 0，
   所以**这一批没有对象**；如果还要按安全边界排序，正确的输入是 `unsafe` 数（sandbox 19，没变）而不是
   unwrap 数。
2. `xai-grok-pager`（57）与 `xai-grok-shell`（46）仍是生产 unwrap 最多的两个 crate，C/D 两批的排序
   不受影响；受影响的是所有**小于** 12 的数，它们都可能整体挪动。
3. **"生产"在这个口径下的含义要说清楚** —— 它是"无法证明只在测试构建里编译"，而不是"一定会在用户机器上
   执行"。上表最后两行的 feature 门控 helper 与 `xai-grok-test-support` 属于前者中的前者，做下一轮治理
   排序时应先按"feature 是否真的在发布 profile 里开"过一遍；反过来 `not(test)` 门控的那些（第 1 行的
   `grove_fuse_ready`）是**只有**发布构建才编译的代码，优先级应该高于同数量级的普通位点。

### 2.7 2026-10-05 口径修正第三处：一条永远为假的 `cfg` 把 23 条测试藏了起来，扫描器把它们记成生产位点

`crates/codegen/xai-grok-shell/src/agent/auth_method.rs:593` 写的是 `#[cfg(any())]`，门住整个
`mod tests`。`any()` 没有分量，因此在任何 target、任何 feature 树、任何 profile 下都是假 ——
rustc 在名字解析**之前**就把整个条目丢掉。丢掉的东西不报警：空 `any()` 是一条合法条件，
只是永不为真，rustc 与 clippy 都不说话。于是三件事同时成立：`cargo test` 编译不到它，所以
它不出现在任何测试报告里；`#[ignore]` 台账数不到它，因为台账统计的是被编译进去又被标掉的
测试；本节的扫描器把它算成生产 —— 它的 `cfg` 求值器只回答"这条属性是否**要求** `test`"
（§2.6 缺陷 2 修的就是这个），而空 `any()` 什么都不要求，于是那个模块里的 panic 全部落进
生产那一列。

**这条属性被写下三次。** 逐提交读 `mod tests` 前面那行属性（只列状态变化的提交，
`f4dc6d53`、`ed6d5436`、`ca69978c`、`228ccf83` 沿用前一行的状态）：

| 提交 | 日期 | `mod tests` 的属性 | 模块内测试数 |
|---|---|---|---:|
| `c68e39f6` | 2026-07-16 | `#[cfg(test)]` | 21 |
| `e9d8fa94` | 2026-07-26 | `#[cfg(any())]` | 21 |
| `780d1388` | 2026-08-03 | `#[cfg(test)]` | 21 |
| `a5589e95` | 2026-08-05 | `#[cfg(test)]` | 24 |
| `0a9313e7` | 2026-08-07 | `#[cfg(any())]` | 21 |
| `bb7f39d5` | 2026-08-31 | `#[cfg(test)]` | 24 |
| `8dd3a8f9` | 2026-09-04 | `#[cfg(any())]` | 23 |
| `HEAD` | 2026-10-05 | `#[cfg(any())]` | 23 |

上游同步两次把模块恢复成 `#[cfg(test)]`（`780d1388`、`bb7f39d5`），fork 层合并又两次把它盖
回去（`0a9313e7` 是 "merge fixup: shell fork-layer re-merge (WIP round 1)"，`8dd3a8f9` 是
"sync upstream changes into chaos 0.3.1"），而且盖回去的那两次同时把上游较新的模块版本换掉
了（24 条变 21 条、24 条变 23 条）。最后一段从 2026-09-04 起一直没被摘掉，到今天 31 天。
也就是说：这既不是一个十周前的一次性决定，也不是单纯的"忘了删"，而是**同一个决定被合并反复
重新做出，没有任何一处信号会响**。本节末尾的门禁就是那个信号。

同一棵 2026-10-05 的树，唯一的变量是扫描器 —— 修正前那份从
`git show HEAD:scripts/ci/panic-site-census.py` 取出后单独运行（两条命令的实际输出见
`docs/verification/never-true-cfg-2026-10-05.log` §3）：

| 扫描器 | 全树 `.unwrap()` 总数 | 落在任何 `cfg(test)` 之外 |
|---|---:|---:|
| 修正前 | 32181 | 300 |
| 修正后 | 32181 | **293** |

两行的总调用数一样，改的只是归属：那 13 个位点从来就在树里，只是不该出现在生产那一列。
（下表是把 97 行 `--write-baseline` 输出逐列相加；列序是 `unwrap / expect / panic / unsafe`。）

| 扫描器状态 | 生产 unwrap | 生产 expect | 生产 panic | 生产 unsafe |
|---|---:|---:|---:|---:|
| 修正前 | 300 | 595 | 130 | 428 |
| 修正后 | **293** | **589** | 130 | 428 |

`xai-grok-shell` 那一行从 `46 / 116 / 28 / 13` 变成 **`39 / 110 / 28 / 13`**，逐位点清单
（`--list xai-grok-shell`）从 203 条变成 190 条。差的那 13 条全在 `auth_method.rs` 里，
按行号列全（行号指修正前那份文件，全部落在被丢掉的 `593-1228` 区间内）：

| 类型 | 行号 |
|---|---|
| `.unwrap()` ×7 | 884、1021、1040、1054、1100、1110、1157 |
| `.expect()` ×6 | 845、847、885、887、1022、1096 |

unsafe 两列一个都没动（657 / 428）—— 这条盲区只影响 unwrap 与 expect，因为那个模块里没有
任何 `unsafe`。

**口径怎么改的。** "永远为假"不等于"测试代码"，也不等于"生产代码"，它是第三种状态：**没有
任何构建编译它**。扫描器现在把这类 span 从生产那一列摘出去，但不并进测试那一列。进一步的
推论是模块声明也归它管：`#[cfg(any())] mod x;` 丢掉的是声明本身，于是 `x.rs` 变成"没有任何
`mod` 编译它"的文件，正确归属是 §2.5 那条 uncompiled 台账
（`scripts/ci/uncompiled-sources.txt`），不是生产也不是测试。这条推论现在有夹具守着 ——
`scripts/ci/test-panic-site-census.py` 里 `dropped_decl_host/mod.rs` 与
`dropped_decl_host/dropped_decl.rs` 那一对，宿主自己的 `.unwrap()` 仍然算生产，被丢掉的那个
文件一个位点也不进任何一列。这对夹具本身是二次写对的：第一版把两个文件平铺在 `src/` 下，
而 `src/host.rs` 里的 `mod x;` 找的是 `src/host/x.rs`，于是那条声明根本解析不到任何文件，
被丢掉的文件因为"没人声明"（另一个原因）落进台账，断言看着绿、其实对 `module_declarations`
里那条规则零覆盖 —— 把 `continue` 改成 `if False` 之后六个断言一个都不红。改成 Rust 真正
会有的目录形状之后，同一个变异会让 `unwrap_prod` 从 13 变 14、uncompiled 台账从 2 个文件变
1 个，六条断言同时红。

**为什么不让它再发生一次。** 新门禁 `scripts/ci/check-dead-cfg.py` 把 `crates/` 与 `bin/` 里
每一条 `cfg`、`cfg_attr`、`cfg!` 谓词折成三值结论（真 / 假 / 看不清），报出判为假的这些；
`cfg_attr` 只看第一个逗号之前的谓词部分，因为它的条目是**永远**构建的。读数逻辑放在
`scripts/cfg_lib.py`，扫描器与门禁共用同一份 —— 两处各写一个 `cfg` 求值器，正是这类盲区的
来源。它在 `scripts/` 而不是 `scripts/ci/`，因为那个目录里的每个文件都必须是某个入口跑起来
的门禁，而一个被两个门禁 import 的库不是（`scripts/notices_lib.py` 同理）。今天全树的读数是
`2979 files, no cfg predicate that can never hold`。

**与 §2.6 的关系：三处口径错误的作用方向各不相同。** §2.6 缺陷 1 让扫描器既虚报又漏报；
缺陷 2 是"提到了 `test`"与"要求 `test`"之差，只会把生产位点漏成测试；本节这条**只会虚报**，
而且虚报方向与缺陷 2 恰好相反 —— 它把不存在于任何二进制里的文本报成生产。三条修完之后，
生产那一列的含义仍然是"无法证明只在测试构建里编译"，但现在"证明不了"与"根本不编译"是两句
话，后者有台账可查。

**本轮同时改的是文本本身。** 模块回到 `#[cfg(test)]` 之后，那 23 条测试逐条判定，四个去向：
**11 条删掉**（断言的是分叉已经删掉的排序与 pin 语义，模块头按名字逐条写明理由，而不是悄悄
删）；**11 条改写后留下**（分类矩阵、凭据判据、env 变量优先级、`AuthManager` 读 legacy token
那条路）；**1 条折进**分叉不变量那条按全部输入跑的断言
（`after_cached_token_unavailable_prefers_api_key_when_advertiseable`）；**3 条新增**，覆盖
仍然可达但已经没人调用的 login 方法构造器，以及 `(true, None)` 那个必须 panic 的组合。
改完 `agent::auth_method` 一共 20 条测试：4 条 runtime key、14 条本模块、2 条分叉不变量，
后者把 `preferred_method` 的三个取值 × 5 个布尔 × `login_label` 的 96 种组合全跑一遍。
这 5 个布尔输入字段如今**没有一个**能改变输出，而这本身就是被测的那条不变量。
总 `.unwrap()` 数从 32181 降到 32176（删掉的测试里带着 5 个），生产那一列纹丝不动停在 293 ——
那 5 个从来就不在生产里，而这正是本节这条修正要说的事。

**哪些数字要按本节重读。** 同一棵树上两个扫描器的差是生产 unwrap −7、生产 expect −6，
unsafe 两列不变。所以 §2.6 表里「两处都修」那一行（300 / 594 / 130 / 429）以及 §1.9、§2.2 的
绝对值，生产 unwrap 与生产 expect 两列要各自往下读这么多。那几张表跑的是 2026-10-04 的树，
与本节的修正前读数相比还差着一天：生产 expect 多 1、生产 unsafe 少 1，这部分与本节的修正
无关。落到 crate 级：`xai-grok-shell` 的生产 unwrap 从 46 变 39、生产 expect 从 116 变 110，
`--list` 从 203 条变 190 条，门禁基线 `scripts/ci/panic-site-baseline.tsv` 里唯一变动的就是
这一行。

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

- `crates/codegen/xai-grok-signature/src/lib.rs`（2026-10-02 自 `xai-grok-update/src/signature.rs` 抽成叶子 crate，原路径以 `pub use xai_grok_signature as signature` 保留）提供 Ed25519 `verify_bytes` / `verify_file`；公钥通过编译期 `CHAOS_SIGNING_PUBLIC_KEY` 注入，未配置时使用占位公钥。
- `auto_update.rs` 下载后会读取 `.sig` 并调用 `verify_file`；是否强制验签由 `CHAOS_REQUIRE_SIG` 或 `require-sig` feature 决定。
- `release.yml` 会在私钥 secret 和公钥 variable 均配置时签名二进制；缺任一项时会跳过签名，workflow 仍允许发布未签名产物。
- `install.sh` 对存在的签名执行验证，签名缺失时仍可继续；`install.ps1` 同样仅在 Python、cryptography、公钥和签名文件均可用时验证，缺少条件时跳过。`install.bat` 调用 PowerShell 安装流程。
- **实测配置状态**：2026-09-23，`gh secret list` / `gh variable list` 未列出 `CHAOS_SIGNING_PRIVATE_KEY` 或 `CHAOS_SIGNING_PUBLIC_KEY`。这里只检查名称，不读取任何密钥值。

### 4.1b 2026-10-02 复核：上列四条已被实测推翻

下面每条都由当天可复跑的指令得出，原始 transcript 见
`docs/verification/release-signature-v0.4.2-2026-10-02.log` 与
`docs/verification/install-sh-linux-2026-10-02.log`。

- **密钥确实已配置**。`gh secret list -R chao2hang/chaos-code` 列出 `CHAOS_SIGNING_PRIVATE_KEY`（created 2026-08-14）与 `NPM_TOKEN`；`gh variable list` 列出 `CHAOS_SIGNING_PUBLIC_KEY`。4.1 记录的「未列出」与本次观测矛盾，且私钥创建时间早于该次复核，因此那句要么当时命令作用在错误的仓库/凭据上，要么当时未认证；保留原文不删，但**不得**再作为现状引用。
- **`release.yml` 已不再允许静默发布未签名产物**：`Sign binaries` 与构建步骤都对两个变量做 `test -n` 并以 `::error::` 失败退出，release 构建使用 `--features xai-grok-update/require-sig`。4.1 的「缺任一项时会跳过签名」不成立。
- **安装脚本已 fail closed**：sidecar 缺失、公钥缺失、python/cryptography 缺失三种情况在 `install.sh` / `install.ps1` / `install.bat` 中都是报错退出，只有显式 `CHAOS_SKIP_SIGNATURE=1` 才降级；由 `scripts/ci/test-installer-signature-policy.py` 固定。
- **真实闭环已跑通**：`v0.4.2` 的六个产物全部带 `.sig`，`signature::verify_file` 在仓库公钥下逐个接受，翻转一字节即拒收（`scripts/verify-release-signature.sh`，16 项检查全绿）。

### 4.1c 本轮新发现的真实缺陷：文档里的安装命令原本装不完

4.2 第 4 条要求端到端安装测试；把它写出来并真的跑一次之后，暴露出一个此前任何文本断言都看不到的缺陷：

- `install.sh` / `install.ps1` / `install.bat` 验签所需公钥**只**来自 `CHAOS_SIGNING_PUBLIC_KEY` 环境变量，而 README 头条命令 `curl -fsSL https://raw.githubusercontent.com/.../install.sh | bash` 不会设置它。三者都在验签处 fail closed，因此这条被文档推荐的命令在任何干净机器上都无法完成安装。
- 更糟的是失败时机：公钥与 python 依赖的检查原本写在**下载之后**，用户要先等 150 MB+ 传完才看到「缺少公钥」。
- 修复：三个安装脚本内置同一把公钥（`DEFAULT_SIGNING_PUBLIC_KEY`，`CHAOS_SIGNING_PUBLIC_KEY` 仍可覆盖，供自签名的 fork 使用），并把前置检查移到下载之前。公钥本就是公开信息（同一值已在公开 repo variable 里），签名依赖的是只在 Actions secret 中的私钥半边。
- 「设置成空字符串」仍按错误处理，所以 fail-closed 分支依然可达、可测。

### 4.1d 把安装脚本真的跑起来之后，又暴露两个缺陷（2026-10-02）

4.1c 的修复仍然只停留在「文本层正确」。把 `scripts/install-sh-in-docker.sh` 跑到第二次，
才看到下面两条：

- **`install.sh` 从未调用 `verify_checksum()`**。函数在，`SHA256SUMS` 的抓取、比对、
  三条错误路径都在，但没有任何调用点（`git show 73a9d7c8:scripts/install.sh` 里只有
  `verify_signature` 被调用）。也就是说 `curl | bash` 传完 160MB 之后根本不查摘要就装；
  函数上方注释写的是相反的行为。`install.ps1` / `install.bat` 是内联实现，不受影响。
  修复：`verify_checksum` 在 `verify_signature` 之前、`chmod +x` 之前调用；
  `test-installer-signature-policy.py` 增加结构性断言（两个检查各必须有且只有一个顶层
  调用点、顺序正确、且早于 `chmod +x "$TMP"`），删掉调用行即失败，证明断言有效。
- **对照实验自己也在说谎**。`install-sh-in-docker.sh` 用
  `in_container sh -c 'CHAOS_SIGNING_PUBLIC_KEY=""; bash ...'` 传环境变量，而
  `VAR=x; cmd` 只给 shell 赋值、不会带进子进程，于是「空白公钥快速失败」用了内置公钥跑完并
  exit 0，「外来公钥拒收」同样会假绿。改为 `env VAR= cmd`，并加一条前置探针断言控制变量
  确实进了子进程环境，探针不过就直接终止。
- 修好之后的完整运行：19 项全绿，含 `checksum OK (0ee7d6ee…a7b4bfa2)`（与
  `verify-release-signature.sh` 独立重算的摘要一致）、空白公钥在下载前即失败、外来公钥
  拒收且已装产物字节不变。原始 transcript：
  `docs/verification/install-sh-linux-2026-10-02.log`。
- 同一轮用真实 PowerShell 解析 `scripts/install.ps1`，发现它自 `21f5a186` 起就无法解析
  （多余右花括号导致 `The Try statement is missing its Catch or Finally block`），
  README 的 `irm … | iex` 从未可能成功。修复于 `4d3eb266`，并由
  `scripts/ci/check-powershell-syntax.py` 在 `platform tests` 两条 leg 上以 `--require`
  把住。**注意**：解析不等于安装，`install.ps1` / `install.bat` 至今没有任何真实执行证据。

### 4.2 发布阻断项（P1，剩余部分）

- ~~生成并安全配置匹配的 secret 与 repository variable；执行真实签名和验证闭环~~ → 已完成，见 4.1b。
- ~~为正式 release 构建启用 `require-sig`，并确认编译进二进制的公钥非占位值；未配置密钥时 release 必须失败~~ → `release.yml` 已如此；未注入公钥的构建会在 `signature::public_key()` 处返回 `NoPublicKey` 而拒绝下载，这一行为由 `scripts/verify-release-signature.sh` 的第 7 项检查真实验证（不带变量重新编译 example，确认其拒绝）。
- ~~将安装脚本验签保证统一~~ → 已完成，见 4.1c。
- ~~增加端到端发布/安装测试：有效签名接受、签名不匹配拒绝、签名缺失拒绝、错误密钥拒绝~~ → 覆盖情况：有效接受与错误密钥拒绝由 `scripts/verify-release-signature.sh`（真实产物）与 `scripts/install-sh-in-docker.sh`（真实安装流程 + 内置公钥 + 外来公钥拒收 + 空白公钥快速失败）执行；签名缺失拒绝由 `crates/codegen/xai-grok-update/tests/test_update_feed_e2e.rs` 的 loopback feed 驱动 `run_update` 覆盖。**仍未覆盖**：`install.ps1` / `install.bat` 的真实执行（无 Windows 环境，只有文本策略测试）。
- 仍未完成：`CHAOS_REQUIRE_SIG=0` 绕过的保留策略评审（见 4.3）；`chaos update` 在 Windows 上的 `.exe` 改名路径。

原结语句「这些项目需要仓库维护者配置并保管供应链密钥……完成前不得创建新 release tag」已被事实越过：密钥已在仓库中，`v0.4.2`（2026-09-23，标记 Latest）已发布带签名的六个产物。保留此段仅为记录当时的判断依据。

### 4.3 格式与开关决策

- 当前使用裸 base64 Ed25519 签名 sidecar（不是完整 minisign 格式）；保持现状，减少格式迁移范围。
- `CHAOS_REQUIRE_SIG=0` 是绕过强制验签的环境开关。正式发布启用 `require-sig` 后，应评审是否保留紧急绕过；若保留，必须在安全文档中明确其影响并测试。

---

## 5. 下一步建议

按"投入产出比"排（2026-10-02 按 §2.5 的实测数重排；此前条目引用的 2.1/2.2 数字
已不可复现，括号里给的是新口径下的量级）：

1. **清掉 12 个"无构建编译"的文件**（§2.5）—— 不是数字问题而是覆盖假象，
   每个文件要么接上 `mod`、要么删；`/adhd` 已经证明这一类里藏着真实的用户可见回归。
2. **生产 unwrap 治理**（全仓 351 个，pager 57 + shell 46 + test-support 56）
   —— 比原计划的"1,270 个"小两个数量级，可以先做 `panic!`/`unreachable!` 那 132 处。
3. **unsafe P0 审计**（sandbox 19 处 + tty-utils 52 处）—— 安全边界优先。2026-09-28
   对 Linux seccomp installers 增加空程序和 >4096 指令长度拒绝的真实入口回归；
   这是局部边界修复，不代表逐项审计完成。
4. **签名集成进 auto_update**（把代码接上真实下载链路）
5. **shell crate env var unsafe 消除**（一次砍 ~60% 的 unsafe 数量）
   —— 已完成大半，见 1.4.1 / 1.4.2；剩余按 §1.9 的位置表推进。

---

## 6. 脚本 / 方法

- `scripts/ci/ignored-tests.sh`：ignore 统计
- unsafe / unwrap / panic! 位点统计与生产-测试拆分：**统一为**
  `scripts/ci/panic-site-census.py`（口径见 §2.5），配
  `scripts/ci/test-panic-site-census.py`（fixture 自测）、
  `scripts/ci/panic-site-baseline.tsv`（生产位点棘轮）、
  `scripts/ci/uncompiled-sources.txt`（"无构建编译"集合棘轮），
  三条命令已在 `.github/workflows/ci.yml` 的 guards step 里。
  当天完整输出：`docs/verification/panic-site-census-2026-10-02.txt`。
- 本节原先记录的 2026-08 方法（`grep -rE '\bunsafe\b'` 的 bash one-liner、
  未归档的临时 `/tmp/count_unwrap.py`）已被上一条取代：两者口径都没有记录，
  因此产物数字不再被引用。
