# Changelog

## 0.2.123

### #15 `--disallowed-tools` TUI 支持

- 从 headless-only 警告列表中移除 `--disallowed-tools` 和 `--tools`，现在 TUI 模式也生效。
- CLI 传入的 `--disallowed-tools` 和 `--tools` 接入 `ConnectFlags` → `CliAgentOverrides`，TUI 会话中内置工具按列表过滤。
- Leader 模式下两个 flag 会被识别为 unsupported 并发出警告。

### #16 配置模型参数不再发起 HTTP 请求

- `go_configure_model` 移除了 `load_models_for` 调用，打开「配置模型参数」界面不再冻结 UI 等待远端模型列表拉取。
- 用户直接手动输入模型 ID 即可配置参数。

### `/fallback` 命令

- 新增 `/fallback` 斜杠命令，管理备用模型链。
- 子命令：`set`（替换整链）/ `add`（追加）/ `remove`（移除）/ `clear`（清空）。
- 持久化到 `~/.grok/config.toml` `[fallback].models`。
- Agent 配置新增 `FallbackConfig` 结构体，sampler 层可读取备用模型列表。

### `/adhd` 命令

- 新增 `/adhd` 斜杠命令，切换 ADHD 技能集成。
- 用法：`/adhd`（切换）/ `/adhd on` / `/adhd off`。
- 开启后自动将 ADHD 辅助规则注入每个会话的系统提示词。
- 规则来源：https://github.com/uditakhourii/adhd
- 持久化到 `~/.grok/config.toml` `[adhd].enabled`。

## 0.2.122

### Token 用量修复

- **自动持久化**：每轮对话结束时自动将 token 用量写入 sqlite，不再仅在打开 `/usage` 面板时才触发。
- **Sentinel 归一**：火山方舟等网关在 SSE `usage.model` 里回传 `"auto"` 而非配置的模型名（如 `ark-code-latest`），现在自动用 `sampling_config.model` 重写，避免全部归到 `auto` 桶。
- **去重**：`record_session_usage` 写库前先 DELETE 同 session 的旧行，防止 auto/真实模型双计。
- **历史回填**：新增 `scripts/backfill-usage.py`，扫描文件系统历史会话 JSONL，将 sentinel 模型名重写为配置模型并写入 sqlite。

### #17 TUI 汉化

- 目标详情视图（goal detail）：状态标签、进度条目、完成度评估、最近历史、命令提示等全面汉化。
- Agent 状态栏：`goal_phase_label` 各阶段（校验中/规划中/执行中/空闲/失败/已中断/预算/完成）及 chip 名（"目标"）。
- 权限提示：编辑/bash/MCP 授权选项、始终允许/始终拒绝前缀、followup placeholder。
- 计划提示（plan nudge）："在规划？可用计划模式，快捷键 …"。
- 回退对话（rewind）："当前有一个轮次正在运行。"/"是否在回退前取消它？"/"取消轮次并回退"/"让它继续跑完"。
- Dashboard 模式标签：`plan` → "计划"、`always-approve` → "总是批准"、`auto` → "自动"。
- 首启 folder-trust（pager-minimal）："是否信任该目录下的内容？"/"允许，继续"/"拒绝，退出"。
- 截断指示器：`Ctrl-F to expand` → `Ctrl-F 展开`。
- 上下文信息栏：技能/MCP 服务器/工具计数等汉化。
- Scrollback verb group：读取/运行/搜索/子代理等动词标签汉化。
- Dashboard 行状态：Working → "运行中"、Response → "回复" 等。
- Session-scoped 命令在 dashboard 上的错误提示："/{name} only works in a session" → "请先打开会话再运行 /{name}。"

### 已知限制

- CJK 标签在 context info bar 中的列对齐尚未使用 unicode-width-aware padding，可能导致视觉上轻微错位（功能不受影响）。
- 50 个预先存在的单元测试失败（品牌 Chaos vs Grok、subagent replay count、extensions modal assertion 等），与本版本无关。
