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

### 1.5 三类分类（初步）

| 类别 | 估算占比 | 说明 |
|---|---:|---|
| **A. 真正必要** | ~20% | FFI（libc/syscall/extern "C"）、asm、内存分配器、PTY raw mode |
| **B. 可消除** | ~65% | 主要是 `set_var` / `remove_var` —— 可用 `Command::env` 隔离、或用线程局部 + 一次性写入替代 |
| **C. 需要 SAFETY 注释** | ~15% | 已经是"合理的 unsafe"，但缺少 `// SAFETY:` 注释，审计者读起来累 |

> A/B/C 数字是粗估。精确分类需要逐文件审。

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
2. **unsafe P0 审计**（sandbox + tty-utils）—— 安全边界优先
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
