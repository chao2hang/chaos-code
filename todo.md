# Headless 模式重写计划

## 背景

上游 grok-build `5da6962` → `dd04f39` 之间对 `xai-grok-pager` 的 headless 模式做了完整架构重写：
- `headless.rs` 从 2142 行缩减到 1550 行（提取子模块）
- 新增 `headless/` 目录 19 个文件，约 3000+ 行
- 涉及 ext_protocol、reducer/messages、CLI 子命令等

## 上游变更清单

### 新增文件（19 个）

| 文件 | 行数 | 职责 |
|------|------|------|
| `headless/cli.rs` | ~249 | headless CLI 子命令解析（`--print`/`--json` 等模式） |
| `headless/ext_protocol.rs` | ~308 | 外部协议适配（VS Code 等编辑器集成） |
| `headless/ext_protocol_tests.rs` | ~405 | ext_protocol 测试 |
| `headless/reducer/mod.rs` | ~50 | reducer 入口 |
| `headless/reducer/acp.rs` | ~201 | ACP 消息 reducer |
| `headless/reducer/messages/mod.rs` | ~817 | 消息处理核心 |
| `headless/reducer/messages/partial.rs` | ~242 | 流式部分消息处理 |
| `headless/reducer/messages/state.rs` | ~186 | reducer 状态 |
| `headless/reducer/messages/usage.rs` | ~100 | 用量提取 |
| `headless/reducer/messages/web_search.rs` | ~80 | Web 搜索结果处理 |
| `headless/reducer/messages/wire.rs` | ~120 | 线协议序列化 |
| `headless/reducer/messages/tests/*.rs` (7 文件) | ~1300 | 消息处理测试 |

### 修改文件

- `headless.rs`：从单体 2142 行拆分为模块化 1550 行，核心逻辑移入 `headless/` 子目录
- `app/event_loop.rs`：882 行变更，与 headless 交互路径调整
- `app/cli.rs`：新增 CLI 子命令路由

## 依赖分析

### 上游依赖
- `ratatui-textarea`（已在 Cargo.toml 中）
- `xai-grok-sampling-types` 的消息类型（已有）
- ACP 协议类型（已有）

### Chaos 兼容性
- **品牌**：headless 输出中可能包含 `Grok` 品牌字符串，需替换为 `Chaos`
- **认证**：headless 模式可能引用 OIDC 路径，Chaos 已剥离，需移除
- **遥测**：`metric_donate` 相关代码需继续跳过
- **config**：headless CLI 可能读取 `[endpoints]` 配置，Chaos 默认关闭

## 迁移策略

### 阶段 1：文件提取与编译（1-2 天）
1. 从上游 `dd04f39` 提取 19 个新文件到 `headless/` 目录
2. 用上游版本替换 `headless.rs`（1550 行）
3. 更新 `mod.rs` / `lib.rs` 模块声明
4. 修复编译错误（品牌替换、OIDC 移除、遥测跳过）
5. 确认 `cargo check -p xai-grok-pager --lib` 通过

### 阶段 2：event_loop 适配（1 天）
1. 从上游 diff 中提取 `event_loop.rs` 的 headless 交互变更
2. 手动适配，保留 Chaos 的 provider/error 处理逻辑
3. 修复 `app/cli.rs` 的子命令路由

### 阶段 3：测试与验证（1 天）
1. 运行 `cargo test -p xai-grok-pager --lib -- headless`
2. 验证 `chaos --print "hello"` 基本功能
3. 验证 `chaos --json` 输出格式
4. 确认 ext_protocol 不影响 TUI 模式

## 风险评估

| 风险 | 影响 | 缓解 |
|------|------|------|
| event_loop 变更引入回归 | TUI 交互异常 | 分阶段提交，每步 cargo check |
| 品牌字符串遗漏 | 用户看到 Grok | 编译后全局 grep |
| OIDC 引用编译失败 | 编译阻断 | 逐个移除，用 Chaos config 替代 |
| 测试失败 | 功能不完整 | 标记 #[ignore]，逐步修复 |

## 建议时间线

- **第 1 天**：阶段 1（文件提取 + 编译修复）
- **第 2 天**：阶段 2（event_loop 适配）
- **第 3 天**：阶段 3（测试验证）
- **第 4 天**：缓冲 + 发版

总计约 3-4 个工作日，建议作为 v0.2.127 独立发布。

## 当前验收结果（2026-07-31）

- [x] `xai-grok-pager` headless 模块拆分与模块声明通过 `cargo check`。
- [x] `xai-grok-pager-bin` CLI 路由通过 `cargo check`；`-p/--print`、`--output-format`、`--json-schema` 已接入 `headless::run_single_turn`。
- [x] headless 测试集：125 passed，0 failed。
- [x] slash 模式支持测试：6 passed，0 failed。
- [x] 用量聚合兼容：保留可选 `cacheCreationTokens`，避免输入/缓存桶重叠。
- [x] Windows MSVC CLI 构建：通过关闭 dev debuginfo、追加 `/DEBUG:NONE`，并为 CLI 主线程预留 `/STACK:8388608`，不再触发 PDB 限制或栈溢出。
- [x] CLI 冒烟：`chaos --help` 返回 0；`chaos -p ""` 返回正常参数错误（exit 1），不再触发 `STATUS_STACK_OVERFLOW`。

结论：headless 迁移、Windows CLI 构建和可执行文件级冒烟均已完成；不需要 Linux。
