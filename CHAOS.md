# Chaos AI 编码助手

Chaos 是终端 AI 编码助手。它不使用 Grok 登录，也不会在启动时读取或刷新 xAI
登录会话。模型、接口地址和密钥均由用户自行配置。

## 构建与启动

环境要求：Rust（见 `rust-toolchain.toml`）、DotSlash 和 `protoc`。在仓库根目录执行：

```sh
cargo build -p xai-grok-pager-bin --release
./target/release/chaos
./target/release/chaos --version
```

开发模式：

```sh
cargo run -p xai-grok-pager-bin
```

包名仍为上游 `xai-grok-pager-bin`，**产出二进制名为 `chaos`**。请勿使用
`https://x.ai/cli/install.sh`（那是官方 `grok` 安装脚本）。

## 配置文件

用户配置目录解析顺序（**不会自动复制或覆盖**任一侧已有文件）：

1. 环境变量 `$CHAOS_HOME`（优先）
2. 环境变量 `$GROK_HOME`（兼容旧文档 / 测试夹具）
3. 若 `~/.chaos` 目录已存在 → 使用它
4. 否则若 `~/.grok` 目录已存在 → 继续使用它（兼容已有会话、skills、MCP、沙箱）
5. 否则默认使用 `~/.chaos`（全新安装；首次写入时创建）

因此旧用户可继续用 `~/.grok/config.toml`；新安装写入 `~/.chaos/`。若要主动迁到 Chaos 目录，自行复制/移动配置到 `~/.chaos` 即可，程序不会改写 `~/.grok`。

**项目级**配置同样双读，不会覆盖任一侧：

| 路径 | 说明 |
|------|------|
| `.chaos/config.toml` / `.grok/config.toml` | 项目 MCP、plugins、permission 等 |
| `.chaos/skills/` / `.grok/skills/` | 项目 skills（同名时 Chaos 优先） |
| `.chaos/hooks/` / `.grok/hooks/` | 项目 hooks |
| `.chaos/agents/` / `.grok/agents/` | 项目 agent 定义 |
| `.chaos/plugins/` / `.grok/plugins/` | 项目 plugins |
| `.chaos/sandbox.toml` / `.grok/sandbox.toml` | 项目沙箱配置 |

推荐使用 `model_providers` 复用同一提供商的连接和认证设置，再用 `model` 定义具体模型。
密钥优先放在环境变量中，不要把密钥提交到 Git。

**内置模型目录为空。** Chaos 不会预装 Grok 4.5 或其它 xAI 产品模型；
`/model` 列表只来自你的 `[model.*]`（以及可选远程目录）。请在
`config.toml` 中至少配置一个模型，并用 `[models] default = "…"` 指定默认
catalog 键（与 `[model.<key>]` 的键一致）。

### OpenAI 兼容协议

```toml
[model_providers.openai]
base_url = "https://api.openai.com/v1"
api_backend = "responses" # 也可使用 "chat_completions"
auth_scheme = "bearer"
env_key = "OPENAI_API_KEY"

[model.gpt-5]
model = "gpt-5"
name = "GPT-5"
model_provider = "openai"
context_window = 400000
max_completion_tokens = 32768
```

兼容网关、OpenRouter、Ollama、vLLM 等服务时，只需替换 `base_url`、模型名和密钥环境变量。

### Claude 兼容协议

Claude 原生 Messages API 使用 `x-api-key`，不能按 OpenAI Bearer 方式发送：

```toml
[model_providers.anthropic]
base_url = "https://api.anthropic.com/v1"
api_backend = "messages"
auth_scheme = "x_api_key"
env_key = "ANTHROPIC_API_KEY"

[model_providers.anthropic.extra_headers]
anthropic-version = "2023-06-01"

[model.claude-sonnet]
model = "claude-sonnet-4-5"
name = "Claude Sonnet"
model_provider = "anthropic"
context_window = 200000
max_completion_tokens = 16384
```

启动前设置密钥：

```sh
export OPENAI_API_KEY="你的密钥"
# 或
export ANTHROPIC_API_KEY="你的密钥"
```

模型级配置会覆盖 Provider 默认值。`extra_headers` 按 header 名大小写不敏感地逐项合并，
因此模型增加一个 header 时不会丢失 Provider 的 `anthropic-version`。

## Token 用量

在交互界面输入：

```text
/usage
```

Chaos 会显示本次会话的输入、输出、缓存命中、推理 Token、总 Token、模型调用次数、
API 耗时及按模型汇总。提供商没有返回价格时，费用显示为不可用，但 Token 仍会正常统计。

输入 `/context` 可查看当前上下文窗口占用和分类明细。无头模式的 JSON 输出也包含用量字段。

## 动态上下文裁剪

Chaos 的动态上下文裁剪采用“原始历史不变、请求前生成选择性投影”的方式：

- 只压缩已经闭合的旧区间，不修改可恢复、回退和分叉所依赖的原始会话历史。
- 用户原始需求、项目指令、当前目标、近期轮次和进行中的工具调用默认受保护。
- 压缩块可以嵌套；新摘要会继承被合并的旧摘要，避免重复压缩导致信息静默丢失。
- 区间必须有序且不能重叠，部分覆盖已有压缩块会被拒绝。
- 高水位仍使用现有全量压缩作为上下文溢出的可靠兜底。
- 统计使用“原 Token 减去摘要 Token”的净节省量。

上下文占用达到约 60% 时，Chaos 会向模型注入一次带历史索引的中文提示，并提供
`compress` 工具。工具支持一次提交多个有序区间，提交前会在本地重新计算 Token、检查
受保护项及工具调用/结果配对；任一区间不合法时整批不生效。压缩元数据保存在会话目录的
`selective_compaction.json`，恢复会话时会校验索引兼容性。`/context` 中的“动态裁剪节省”
显示当前请求投影累计节省的 Token。

该实现根据公开行为独立设计，没有复制 AGPL 项目的源码或提示词。

## 常见错误

模型返回 401/403 时，请依次检查：

1. `env_key` 指向的环境变量是否存在且非空。
2. `base_url` 是否包含正确的 API 版本路径。
3. OpenAI 是否使用 `auth_scheme = "bearer"`。
4. Claude 是否使用 `api_backend = "messages"` 和 `auth_scheme = "x_api_key"`。
5. Claude 请求是否包含 `anthropic-version`。

Chaos 不提供 `login`、`logout` 或 `/login` 命令。认证失败应修正当前模型的 Provider 配置。
