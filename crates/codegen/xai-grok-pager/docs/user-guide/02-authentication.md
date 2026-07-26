# Authentication（Chaos）

> **Chaos 分支说明：** 本产品**不使用** Grok / xAI 浏览器登录、OIDC 或订阅门墙。
> 模型、接口地址与密钥均由用户在配置文件中自带（BYOK）。完整示例见仓库根目录
> [CHAOS.md](../../../../CHAOS.md)。下文上游 Grok 登录说明已停用。

---

## 认证方式

Chaos 仅通过 **Provider API Key** 访问模型：

1. 在解析后的用户配置根写入 `config.toml`（顺序见 [CHAOS.md](../../../../CHAOS.md)：
   `$CHAOS_HOME` → `$GROK_HOME` → 已有 `~/.chaos` → 已有 `~/.grok` → 默认 `~/.chaos`）。
   配置 `model_providers` 与 `model`。程序**不会**自动复制或覆盖任一侧已有目录。
2. 密钥优先放在环境变量中（`env_key`），不要写入 Git。
3. 使用 `/provider` 在 TUI 中管理 Provider 列表、密钥与默认模型。

Chaos **不提供**可用的 `/login`、`/logout` 或浏览器 OAuth 流程。若仍看到历史登录相关入口，应改为配置 Provider。

---

## 快速配置：OpenAI 兼容协议

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

兼容网关、OpenRouter、Ollama、vLLM 等时，替换 `base_url`、模型名与密钥环境变量即可。

启动前：

```sh
export OPENAI_API_KEY="你的密钥"
chaos
```

---

## 快速配置：Claude 原生 Messages API

Claude 使用 `x-api-key`，不能按 OpenAI Bearer 方式发送：

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

```sh
export ANTHROPIC_API_KEY="你的密钥"
chaos
```

模型级配置会覆盖 Provider 默认值。`extra_headers` 按 header 名大小写不敏感地逐项合并。

---

## 常见错误（401 / 403）

按顺序检查：

1. `env_key` 指向的环境变量是否存在且非空。
2. `base_url` 是否包含正确的 API 版本路径。
3. OpenAI 兼容是否使用 `auth_scheme = "bearer"`。
4. Claude 是否使用 `api_backend = "messages"` 与 `auth_scheme = "x_api_key"`。
5. Claude 请求是否包含 `anthropic-version`。

认证失败应修正 Provider 配置，而不是尝试登录 xAI 账号。

---

## 与上游 Grok Build 的差异

| 能力 | 上游 Grok Build | Chaos |
|------|-----------------|-------|
| 浏览器登录 grok.com | 默认 | 不支持 |
| OIDC / 企业 SSO | 支持 | 不支持 |
| `XAI_API_KEY` / xAI 会话 | 支持 | 不作为产品路径 |
| 用户自带 Provider | 有限 | **唯一**认证路径 |
| `/login` `/logout` | 产品命令 | 未注册；请用 `/provider` |

历史路径 `~/.grok/auth.json` 与 OIDC 配置与 Chaos 无关；请勿依赖。

---

## 相关文档

- 仓库根 [CHAOS.md](../../../../CHAOS.md) — 构建、模型配置、Token 统计、动态上下文裁剪
- [Custom Models](11-custom-models.md) — BYOK / 兼容端点（若仍写 Grok 登录，以本文与 CHAOS.md 为准）
- [Configuration](05-configuration.md) — `config.toml` 其他项
