# 自定义模型

Chaos 可以接入自定义模型端点，用于替代 Provider、自托管模型，以及覆盖内置设置。本指南说明如何选择模型、配置端点，以及接入第三方 Provider。

---

## 默认模型

Chaos **不内置任何模型**。全新安装的目录是空的，直到你在 `config.toml` 里加上 `[model.*]`（以及可选的 `model_providers`）。在线目录抓取默认关闭（`[features] remote_fetch = false`）。

列出已配置的模型：

```bash
chaos models
```

---

## 选择模型

### CLI 标志

```bash
chaos -p "Hello" -m grok-4.6
```

### 斜杠命令

在 TUI 里可以在会话中途切换模型：

```
/model grok-4.6
```

或使用别名：

```
/m grok-4.6
```

### 模型选择器（Ctrl+M）

在回滚区按 `Ctrl+M` 打开模型选择器。Chaos **不内置任何模型**，列表里只有你的 `[model.*]` 条目（只有设置了 `remote_fetch = true` 与模型列表 URL 时才有远端目录）。提示框获得焦点时，`Ctrl+M` 改为切换多行输入——想不离开提示框就换模型，请用 `/model`。

### 机群白名单（`requirements.toml`）

企业主机可以在签名的 `requirements.toml` 里钉住**可选**集合——不只是默认值。该列表会**取代**用户的 `allowed_models`（不是取并集），因此 `/model`、`Ctrl+M` 和 `-m` 都无法提供列表之外的模型。

```toml
[models]
default = "grok-4.5"
allowed_models = ["grok-4.5", "grok-4*"]
```

机群钉定匹配的是**模型 id**（而不是用户自选的 catalog 键），因此本地的 `[model.<name>]` 条目无法把集合放宽。用户配置里的 `allowed_models` 仍按 catalog 键或模型 id 匹配。省略该键则保留用户配置不变。空数组表示不限制。钉定存在但读不出来时按 fail-closed 处理（什么都选不了）。在模型目录抓取之后，默认值或 `-m` 若落在钉定集合之外会被拒绝——请联系管理员；这份列表用户无法自行编辑。

### 配置默认值

在 `~/.chaos/config.toml`（或兼容的 `~/.grok/config.toml`）里设置持久默认值：

```toml
[models]
default = "gpt-5"
```

`default` 必须是某个 `[model.<key>]` 的 catalog 键。

---

## 支持的 API 后端

Chaos 支持三种 API 后端。在 `[model.*]` 配置里设置 `api_backend`，决定该模型使用哪种协议：

| 取值 | API | 默认 |
|-------|-----|---------|
| `"chat_completions"` | OpenAI Chat Completions (`/v1/chat/completions`) | 是 |
| `"responses"` | OpenAI Responses (`/v1/responses`) | |
| `"messages"` | Anthropic Messages (`/v1/messages`) | |

省略 `api_backend` 时，Chaos 使用 `chat_completions`。

要发送 Provider 专有的认证或版本头——例如 Anthropic 的 `x-api-key`——请用下面介绍的 `extra_headers` 字段。Chaos 会把这些头原样附在发往该端点的每个请求上。

---

## 配置自定义模型

在 `~/.chaos/config.toml` 里用 `[model.<name>]` 段添加自定义模型端点：

```toml
[model.my-model]
model = "model-id"                        # Model identifier sent to the API
base_url = "https://api.example.com/v1"   # OpenAI-compatible endpoint
name = "Display Name"                     # Shown in the model picker
description = "Model description"          # Optional description
api_key = "sk-..."                        # API key for this provider (optional)
env_key = "XAI_API_KEY"                   # Env var holding the API key (optional; string or array)
api_backend = "chat_completions"          # "chat_completions", "responses", or "messages"
reasoning_summary = "concise"             # Responses API only: "none", "auto", "concise", or "detailed"
temperature = 0.7                         # Sampling temperature
top_p = 0.95                              # Nucleus sampling parameter
max_completion_tokens = 8192              # Maximum tokens per response
context_window = 128000                   # Total context window in tokens
extra_headers = { "x-api-key" = "sk-..." } # Extra request headers, sent verbatim (optional)
```

### 凭据解析

Chaos 按以下顺序解析 API 密钥：

1. 模型配置里的 `api_key` 字段
2. `env_key` 指定的环境变量（单个名字或名字数组）。取第一个已设置且非空的值（例如为 SSH `LC_*` 转发设置 `env_key = ["ANTHROPIC_AUTH_TOKEN", "LC_ANTHROPIC_AUTH_TOKEN"]`）
3. 模型自身既没有 `api_key` 也没有 `env_key` 时，就没有凭据可用：本分叉的认证是自带密钥（BYOK），请配置上面两项之一
4. `XAI_API_KEY` 环境变量（全局兜底；为向后兼容，Chaos 也接受 `GROK_CODE_XAI_API_KEY`）

### 上下文窗口

`context_window` 的取值告诉 Chaos 何时触发自动压缩。覆盖一个已知模型时，Chaos 继承该模型的上下文窗口。定义新模型且省略 `context_window` 时，Chaos 默认使用 200,000 个 token，所以请显式设置成与你 Provider 一致的值。

### 请求大小上限

`max_request_bytes` 是你的端点能接受的最大请求体。Chaos 会把较旧的内联图片从对话中逐出，以保持在限制之下，这样带大量截图的会话也能继续工作，而不是被拒绝。省略时，Chaos 按 `api_backend` 取默认值：`messages` 为 30 MB，`chat_completions` 与 `responses` 为 50 MiB。只有当你的宿主执行不同的上限时才设置它。

该上限是端点的属性，所以它也作用于共享的 `[model_providers.<id>]` 块：指向该 provider 的每个模型都继承它；模型自身的 `max_request_bytes` 覆盖 provider 的值。

```toml
[model_providers.messages-gateway]
base_url = "https://gateway.example/v1"
api_backend = "messages"
max_request_bytes = 25000000   # every model on this provider inherits it

[model.claude-sonnet]
model = "claude-sonnet"
model_provider = "messages-gateway"   # inherits 25 MB

[model.claude-opus]
model = "claude-opus"
model_provider = "messages-gateway"
max_request_bytes = 20000000   # per-model override
```

### 全局默认请求头

要把同样的请求头应用到目录里的*每个*模型——内置的、从 `/v1/models` 预取的、以及自定义的——请在全局 `[models]` 段里设置一次，而不必逐个模型重复：

```toml
[models]
extra_headers = { "X-Request-Tags" = "team=example,env=prod" }
```

这些头是每个模型推理请求的基础。某个模型的 `[model.<id>].extra_headers` 会**按单个键**覆盖全局默认值（键名不区分大小写）：在模型上设置的键生效，而只在全局出现的键仍会被该模型继承。与单模型字段一样，它们只附在该模型的推理调用上——不会附到图像生成、视频生成这类独立服务上——因此很适合用来做归属标记（例如成本核算），不必每次出现新模型都重新声明一遍。

### 全局默认值

一些常见的单模型设置也可以在 `[models]` 下设置一次，作为*每个*模型的默认值。单模型的 `[model.<id>]` 值总是优先；全局值只在模型（或服务端的模型列表）没有给出该字段时补上：

```toml
[models]
temperature                 = 0.7
top_p                       = 0.95
max_completion_tokens       = 8192
max_retries                 = 8
rate_limit_retry_threshold  = 4
inference_idle_timeout_secs = 600
subagent_rate_limit_max_attempts = 8
stream_tool_calls           = true
```

这是一组小而固定的全局开关。用于标识具体模型的设置（`model`、`base_url`、`api_key`、`context_window` 等）不能用这种方式设默认值；另有少数设置各有专属配置位置——自动压缩在 `[session]`、系统提示词标签在 `[agent]`、推理强度在 `[models].default_reasoning_effort`——它们仍留在原处。

`rate_limit_retry_threshold` 与 `subagent_rate_limit_max_attempts` 为子代理选择不同的 429 重试路径。配置了 `rate_limit_retry_threshold` 后，这些重试由采样器（sampler）接管，独立的子代理等待循环被禁用——包括它的 150 秒累计等待预算与等待遥测。`subagent_rate_limit_max_attempts` 仅在未设置采样器阈值时生效。

> **关于 `stream_tool_calls`：** 它影响的是请求的*形状*，不只是采样。少数端点（某些 BYOK Provider）希望这一项不被设置；若全局的 `stream_tool_calls = true` 让这类模型出问题，请在该模型的 `[model.<id>]` 块里用 `stream_tool_calls = false` 把它单独排除。

### 请求查询参数

有些网关按查询串做路由或版本选择。`query_params` 会为 Chaos 为该模型发出的每个请求追加百分号编码的查询参数。例如按这种方式选择 API 版本的网关：

```toml
[model.my-gateway]
model = "my-model"
base_url = "https://gateway.example/v1"
api_backend = "responses"
env_key = "GATEWAY_API_KEY"
query_params = { api-version = "2026-07-22" }
```

与 `base_url` 查询串中同名的键会被覆盖（最后一个值胜出），而不是重复。查询参数会保存在会话里，所以不要放密钥：密钥请用 `env_http_headers`。

### 环境变量请求头

`env_http_headers` 把请求头映射到提供其值的环境变量名，这样每次请求的密钥就不必写进 `config.toml`：

```toml
[model.gateway]
model = "my-model"
base_url = "https://gateway.example/v1"
env_http_headers = { "X-Tenant-Token" = "GATEWAY_TENANT_TOKEN" }
```

Chaos 在为会话构建客户端时读取每个变量，值只放在请求头里，从不落盘。变量未设置或为空白时该头被跳过；解析出的值覆盖 `extra_headers` 中同名的条目。静态值用 `extra_headers`，来自环境的值用 `env_http_headers`。

这两个字段也作用于共享的 `[model_providers.<id>]` 块。通过 `model_provider = "<id>"` 指向 provider 的模型，在自己都没设置时继承 provider 的 `query_params` 与 `env_http_headers`，与 `extra_headers` 的继承方式一致。

### 模型通告

`notice` 在该模型被选中期间，在提示框上方显示一条消息。横幅无法收起；切换到没有通告的模型时它才会消失。

```toml
[model.legacy]
model = "legacy-model"
base_url = "https://gateway.example/v1"
notice = { severity = "warning", text = "This model is deprecated on Oct 15. Switch to grok-4.6.", label = "deprecated" }
```

`severity` 设置横幅颜色，取 `info`、`warning` 或 `critical`，默认 `info`。`text` 必填，长文本会换行。`label` 是显示在文本前的可选短标签。自定义模型端点也可以在模型条目上以 `notice` 或 `_meta.notice` 下发同一个对象。要移除内置或远端模型自带的通告，设置 `notice = { text = "" }`。

---

## 覆盖内置模型

可以只覆盖内置模型的某些字段，而不必整体重定义。只写你想改的字段即可：

```toml
# Override only the API key for a default model
[model.grok-4.6]
api_key = "my-api-key"

# Override temperature and add a custom API key
[model.grok-4.6]
temperature = 0.5
api_key = "sk-custom"
```

覆盖内置模型时，Chaos 先取默认配置（包括正确的 `base_url`），再只应用你写出的那些字段。未指定的字段继承默认值。

### 优先级顺序

1. 你的配置（`[model.*]`）—— 最高优先级
2. 从远端 `/v1/models` 预取的模型
3. 硬编码默认值 —— 最低优先级

---

## 提供方示例

### Anthropic（Claude）

通过 Anthropic Messages API 直接使用 Claude 模型：

```toml
[model.claude-opus]
model = "claude-opus-4-6"
base_url = "https://api.anthropic.com/v1"
name = "Claude Opus 4.6"
api_backend = "messages"
context_window = 200000
extra_headers = { "x-api-key" = "sk-ant-...", "anthropic-version" = "2023-06-01" }
```

`messages` 后端使用 Anthropic Messages 协议。Anthropic 用 `x-api-key` 头认证，而不是 `Authorization: Bearer`，所以请把密钥通过 `extra_headers` 传入——Chaos 会原样发送它。

### OpenAI（Chat Completions）

```toml
[model.gpt-4o]
model = "gpt-4o"
base_url = "https://api.openai.com/v1"
name = "GPT-4o"
env_key = "OPENAI_API_KEY"
```

`api_backend` 默认就是 `"chat_completions"`，所以接入 OpenAI 时不必显式设置。

### OpenAI（Responses API）

如果你的 Provider 支持较新的 Responses API：

```toml
[model.gpt-4o-responses]
model = "gpt-4o"
base_url = "https://api.openai.com/v1"
name = "GPT-4o (Responses)"
api_backend = "responses"
env_key = "OPENAI_API_KEY"
```

在 Responses API 上，Chaos 默认请求 `concise` 推理摘要——UI 中显示的推理文本就来自它。`reasoning_summary` 会改变请求：`detailed` 或 `auto` 得到更完整的摘要，`none` 则省略该字段（供会拒绝它的网关使用）。

### AWS Bedrock (Mantle)

Bedrock 的 OpenAI 兼容网关会拒绝 `reasoning.summary`，所以要设置 `reasoning_summary = "none"`。它用 Bedrock API key 作为 bearer token 认证；下面的例子通过一个按名字配置的认证提供程序签发短期 token：

```toml
[auth_provider.bedrock]
command = "aws-bedrock-token"   # prints a Bedrock API key on stdout (e.g. via aws-bedrock-token-generator)
token_ttl_secs = 3600

[model."bedrock-grok-4.6"]
model = "xai.grok-4.6"
base_url = "https://bedrock-mantle.us-west-2.api.aws/openai/v1"
name = "Grok 4.6 (Bedrock)"
api_backend = "responses"
reasoning_summary = "none"
auth_provider = "bedrock"
context_window = 500000
```

### Ollama（本地模型）

用 [Ollama](https://ollama.ai) 在本地运行模型：

```toml
[model.ollama-codellama]
model = "codellama"
base_url = "http://localhost:11434/v1"
name = "CodeLlama (Ollama)"
```

请确认 Ollama 正在运行（`ollama serve`），并且模型已经拉取（`ollama pull codellama`）。

### Together AI

```toml
[model.together-mixtral]
model = "mistralai/Mixtral-8x7B-Instruct-v0.1"
base_url = "https://api.together.xyz/v1"
name = "Mixtral 8x7B"
env_key = "TOGETHER_API_KEY"
```

### 本地 OpenAI 兼容服务器

任何实现了 OpenAI Chat Completions 或 Responses API 的服务器：

```toml
[model.local-llama]
model = "llama-3.1-70b"
base_url = "http://localhost:8080/v1"
name = "Local Llama"
temperature = 0.8
```

---

## 自定义模型端点

把 Chaos 指向一个自定义的、兼容 OpenAI 的 `/v1/models` 端点，而不是默认端点。当你的模型位于企业网关或自托管推理服务之后时用它。

### 环境变量

| 变量 | 必填 | 说明 |
|----------|----------|-------------|
| `GROK_MODELS_BASE_URL` | 是 | 推理用的 Base URL。Chaos 从 `{base_url}/models` 抓取模型列表。 |
| `XAI_API_KEY` | 是 | 作为 `Authorization: Bearer` 发送的 API 密钥。Chaos 也接受 `GROK_CODE_XAI_API_KEY`。 |
| `GROK_MODELS_LIST_URL` | 否 | 模型列表 URL 与 `{base_url}/models` 不同时，用它覆盖。 |

### 配置步骤

```bash
export GROK_MODELS_BASE_URL="https://api.acme.com/v1"
export XAI_API_KEY="xai-..."
chaos
```

### 改用配置文件

```toml
[endpoints]
models_base_url = "https://api.acme.com/v1"

# Override only the API key for a specific model
[model.grok-4.6]
api_key = "my-api-key"
```

把 `[endpoints]` 与部分模型覆盖一起使用时，Chaos 会从 endpoints 配置继承 `base_url`，因此不必在每个 `[model.*]` 段里重复指定。`XAI_API_KEY` 仍然必填；每模型的 `api_key` 或 `env_key` 为该模型的推理请求认证；启动时的模型列表抓取始终使用 `XAI_API_KEY`。

### 认证行为

设置 `models_base_url` 后，Chaos 改用 API key 认证（`Authorization: Bearer`），而不是会话认证。有 API key 就够了，无需其它登录步骤。模型列表请求只用 `XAI_API_KEY` 认证（`Authorization: Bearer`）；若配置了外部认证提供程序（`auth_provider_command`）且没有 `XAI_API_KEY`，请求改发该提供程序的 token；否则（未设置 `XAI_API_KEY` 时）抓取会失败，并提示你设置它。指向自定义主机的推理请求单独认证；每个模型上的 `api_key` 或 `env_key` 会让这些请求也使用 API key。

---

## 网页搜索模型

`web_search` 工具使用单独的模型。这样配置：

```toml
[models]
web_search = "grok-4.20-multi-agent"
```

或通过环境变量：

```bash
export GROK_WEB_SEARCH_MODEL="grok-4.20-multi-agent"
```

如果让网页搜索指向自定义模型，你还需要一条 `[model.*]` 条目，Chaos 才能访问到它。服务端（「后端」）网页搜索只在模型声明了 `supports_backend_search = true`（且该构建启用了后端搜索）时运行；它与 `api_backend` 无关：

```toml
[models]
web_search = "my-custom-model"

[model.my-custom-model]
model = "my-custom-model"
supports_backend_search = true
```

---

## 使用自定义模型

```bash
# List available models (including custom)
chaos models

# Use in the TUI via slash command
/model my-model

# Use in headless mode
chaos -p "Hello" -m my-model

# Set as default in config.toml:
[models]
default = "my-model"
```

---

## 企业部署

一份带自定义模型的企业部署完整配置：

```toml
[cli]
auto_update = false

[auth]
auth_provider_command = "/usr/local/bin/my-company-auth-provider"
auth_provider_label = "Acme Corp"
auth_token_ttl = 3600

[models]
default = "company-grok"

[model.company-grok]
model = "grok-4.6"
base_url = "https://grok-proxy.acme.com/"
name = "Grok 4.6 (Proxy)"
context_window = 128000

[features]
telemetry = false
```

---

## 故障排查

### 找不到模型

```bash
# List available models
chaos models

# Check config.toml for typos in [model.*] sections
```

### 连接错误

确认端点可达：

```bash
curl -s https://api.example.com/v1/models \
  -H "Authorization: Bearer $XAI_API_KEY"
```

### 调试日志

```bash
RUST_LOG=debug GROK_LOG_FILE=/tmp/grok.log chaos
tail -f /tmp/grok.log
```

查找含 `model` 或 `sampling` 的日志条目，以追踪模型选择与 API 调用。
