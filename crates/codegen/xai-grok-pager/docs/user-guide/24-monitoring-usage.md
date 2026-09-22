# 用量监视（外部 OpenTelemetry）

> **状态：alpha。** 下文的 schema 带版本管理（`grok_code.schema.version = v1`）；
> 新增字段可能不经通知直接出现；重命名/删除会提升
> 版本号并在变更日志中明确说明。

Grok CLI（上游）可以把使用量**指标**和**事件**导出到你所在组织
自己的 OpenTelemetry 收集器，让平台团队能够监视采纳情况、token
消耗、工具权限判定和错误 —— 全程不经过 SpaceXAI，
任何数据都不会流向它。

## 相关设置

以下这些开关彼此独立（也独立于本指南的外部 OTEL 数据流）：

| 设置 | 设置方式 |
|---------|---------------|
| 遥测总开关 | `[features] telemetry` / `GROK_TELEMETRY_ENABLED` |
| 编码数据、保留与训练 | Settings —— `/privacy` 打开对应行 |
| Trace 上传 | `[telemetry] trace_upload` / `GROK_TELEMETRY_TRACE_UPLOAD` |
| 外部 OpenTelemetry | `GROK_EXTERNAL_OTEL` / `[telemetry] otel_*`（本指南） |

另见[认证](02-authentication.md#相关文档)与
[配置](05-configuration.md#telemetry)。

## 外部 OTEL 数据流

外部数据流具有以下特点：

- **默认关闭**，且需要*双重 opt-in*（总开关**和**
  显式选择 exporter）。
- **默认不含内容**：没有提示词、没有助手正文、没有代码、没有
  完整文件路径（只有扩展名）、没有工具参数、没有 bash 命令，MCP/技能/插件
  名称也折叠为类别。可选的内容门控可以重新打开其中一部分。
- **与 SpaceXAI 内部遥测结构性隔离**：它的 exporter 只携带
  你配置的 headers，绝不携带 SpaceXAI 凭据。
- **不受 SpaceXAI 数据保留退出开关影响**：即使 `telemetry`
  被禁用、或团队处于 ZDR（零数据保留），它照常工作。那些
  设置管的是 SpaceXAI 侧的保留；外部数据流只由
  你自己的 OTEL 配置决定。

### ZDR 与本数据流

`/privacy` 和 Zero Data Retention **不会**关闭这条数据流。ZDR 关闭的是
SpaceXAI 侧的保留（产品分析、会话 trace 上传、
编码数据共享）。它不会屏蔽 `GROK_EXTERNAL_OTEL`。

数据流开启时：

- `user.id`、`session.id` 以及 org/team/deployment id 始终导出。
- 只要 OAuth/网关认证带有
  非空地址，`user.email` 就会附加到日志**和**指标。它是身份标识而非内容门控，除了
  关闭数据流之外无法钉定。
- 提示词文本、助手 `response` 和工具主体只在其
  门控开启时导出。第一方产品分析永远收不到这些主体。

要让 ZDR 机器对收集器保持静默，请钉定 `otel_enabled = false`（或干脆
不启用数据流）。只要指标的 SIEM 场景，请把四个 `otel_log_*` 键全部
钉定为 `false`。

## 快速上手

```bash
export GROK_EXTERNAL_OTEL=1                  # master switch
export OTEL_METRICS_EXPORTER=otlp
export OTEL_LOGS_EXPORTER=otlp
export OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf  # or grpc
export OTEL_EXPORTER_OTLP_ENDPOINT=https://collector.corp.example:4318
export OTEL_EXPORTER_OTLP_HEADERS="Authorization=Bearer <collector-token>"
chaos
```

只设 `GROK_EXTERNAL_OTEL=1` 什么也**不会**开启 —— 你还必须选择
至少一个 exporter。反过来，没有总开关，光有 `OTEL_*` 变量同样
什么也开不了。

## 环境变量

| 变量 | 默认 | 含义 |
|---|---|---|
| `GROK_EXTERNAL_OTEL` | `0` | 总开关。与 `GROK_TELEMETRY_ENABLED` 不同，后者管的是 SpaceXAI 内部产品分析 —— 两者治理的是方向相反的数据流。 |
| `OTEL_METRICS_EXPORTER` | `none` | `otlp` \| `console` \| `none`。 |
| `OTEL_LOGS_EXPORTER` | `none` | `otlp` \| `console` \| `none`。控制事件流。 |
| `OTEL_EXPORTER_OTLP_PROTOCOL` | `http/protobuf` | `http/protobuf` \| `grpc`。两种信号共用的基础协议。 |
| `OTEL_EXPORTER_OTLP_LOGS_PROTOCOL` / `..._METRICS_PROTOCOL` | — | 按信号覆盖协议（取值与基础协议相同）。无法识别的取值会禁用数据流。 |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | HTTP 用 `http://localhost:4318`，gRPC 用 `http://localhost:4317` | 基础端点。`http/protobuf` 按 OTLP 规范追加 `/v1/logs` 和 `/v1/metrics`；`grpc` 则原样使用收集器端点。路径追加按**该信号自身的**协议进行。 |
| `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT` / `..._METRICS_ENDPOINT` | — | 按信号覆盖端点，按字面使用。gRPC 场景通常应使用不带 `/v1/...` 路径的收集器端点。 |
| `OTEL_EXPORTER_OTLP_HEADERS`（+ 各信号专用变体） | — | 收集器认证（`k=v,k2=v2`）。是外部 exporter 发送的**仅有** headers，也是唯一受支持的收集器认证机制（没有配置文件 headers 键 —— token 绝不落盘）。 |
| `OTEL_EXPORTER_OTLP_CERTIFICATE`（+ 各信号专用变体） | — | PEM bundle 路径，包含用于校验收集器的额外受信 CA 证书 —— 面向私有/企业 CA 后面的收集器。叠加在默认信任根（系统证书库和内置 Mozilla 根）之上。也可通过 `[telemetry] otel_certificate` 设置。 |
| `OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE` / `OTEL_EXPORTER_OTLP_CLIENT_KEY`（+ 各信号专用的 `…_LOGS_…` / `…_METRICS_…` 变体） | — | mTLS 客户端身份的 PEM **路径**。证书和密钥必须成对设置（基础键或同一信号的键）；只配一半会被忽略并给出警告。仅支持未加密的 PEM 密钥。也可通过 `[telemetry] otel_client_certificate` / `otel_client_key` 设置。 |
| `OTEL_EXPORTER_OTLP_TIMEOUT` | `10000`（毫秒） | 导出超时。 |
| `OTEL_METRIC_EXPORT_INTERVAL` | `60000`（毫秒） | 指标导出间隔。 |
| `OTEL_BLRP_SCHEDULE_DELAY`（别名 `OTEL_LOGS_EXPORT_INTERVAL`） | `5000`（毫秒） | 日志批次间隔。 |
| `OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE` | `delta` | `delta` \| `cumulative`。 |
| `OTEL_METRICS_INCLUDE_SESSION_ID` | `1` | 把 `session.id` 附加到指标（可按基数考虑退出）。 |
| `OTEL_METRICS_INCLUDE_VERSION` | `0` | 把 `app.version` 附加到指标。 |
| `OTEL_LOG_USER_PROMPTS` | `0` | 内容门控：`grok_code.user_prompt` 上的提示词文本（60 KB 上限，已做密钥脱敏）。 |
| `OTEL_LOG_ASSISTANT_RESPONSES` | 未设置时跟随 prompts | 内容门控：`grok_code.assistant_response` 上的 `response`（60 KB 上限，已做密钥脱敏）。未设置时跟随 `OTEL_LOG_USER_PROMPTS`；显式 `0` 表示提示词保持开启而回复关闭。`response_length` 始终导出。只靠环境变量的机群若设了 `OTEL_LOG_USER_PROMPTS=1`，必须设 `OTEL_LOG_ASSISTANT_RESPONSES=0`（或在 requirements 中钉定关闭）才能保持仅提示词的数据流。 |
| `OTEL_LOG_TOOL_DETAILS` | `0` | 元数据门控：4 KB `tool_parameters` 预览、完整文件路径、原样的 MCP/技能/插件名称。**不**包含完整主体。不开 CONTENT 的 DETAILS 是企业默认。 |
| `OTEL_LOG_TOOL_CONTENT` | `0` | 主体门控：`tool_input`、`tool_output`、`full_command` 以及失败时的 `error_message`（已做密钥脱敏；输入/输出/命令 60 KB，error_message 4 KB）。独立于 DETAILS —— 开 CONTENT **不**意味着开 DETAILS。默认关闭。 |

### 推荐的机群门控

企业默认是 **DETAILS 开、CONTENT 关**：只有元数据（路径、4 KB
`tool_parameters` 预览、原样的 MCP/技能/插件名称），没有 Read、bash 或
MCP 结果主体。只有当收集器必须存储这些
主体时才开 CONTENT。CONTENT **不**蕴含 DETAILS —— 只开 CONTENT 会得到 60 KB
的 `tool_output`，但 `tool_name` / `mcp_tool.name` 仍折叠为
`mcp_tool`。

```toml
[telemetry]
otel_log_user_prompts = false
otel_log_assistant_responses = false   # unset would follow prompts; pin off
otel_log_tool_details = true           # metadata for SIEM join
otel_log_tool_content = false          # bodies; independent of details
```

`OTEL_RESOURCE_ATTRIBUTES` 被刻意忽略：resource 由一个固定、
经过审计的属性集合构建。

> **迁移说明：** 旧版本可以把 `OTEL_EXPORTER_OTLP_*` 与
> 产品自带的分析管道共用。该行为已弃用：当
> `GROK_EXTERNAL_OTEL` 已设置时，产品分析会忽略这些变量，而且
> 只要某个配置下产品分析已经消费过它们，CLI 就会拒绝
> 激活外部数据流 —— 你的收集器只会收到你
> 明确 opt-in 的外部数据流。

## 配置文件

组织默认值放在 `config.toml` 现有的 `[telemetry]` 表下
（环境变量优先）。这些键是其他 `[telemetry]`
设置的 `otel_` 前缀对应项：

```toml
[telemetry]
otel_enabled = true
otel_metrics_exporter = "otlp"
otel_logs_exporter = "otlp"
otel_endpoint = "https://collector.corp.example:4318"
otel_protocol = "http/protobuf"  # or "grpc"
# Optional PEM *paths* for private-CA trust and mTLS (never PEM contents):
otel_certificate = "/etc/ssl/corp-ca.pem"
otel_client_certificate = "/etc/ssl/client.crt"
otel_client_key = "/etc/ssl/client.key"
otel_log_user_prompts = false   # admins can pin these via requirements
otel_log_assistant_responses = false
otel_log_tool_details = false   # code default; SIEM fleets usually true — see Recommended fleet gates
otel_log_tool_content = false
```

配置键是 `[telemetry]` 下的 `otel_*`；而**环境变量保持
标准 OTEL 名称**（`GROK_EXTERNAL_OTEL`、`OTEL_*`）以便与生态
互通，所以两层刻意使用不同的命名空间。配置键
`otel_protocol` 对应 `OTEL_EXPORTER_OTLP_PROTOCOL`。CA 和客户端身份上，环境变量
优先于配置文件路径。

刻意不设 `headers` 键：收集器认证请通过
`OTEL_EXPORTER_OTLP_HEADERS` 提供，这样 token 绝不会存储在磁盘上。证书
和密钥的配置键**只能是路径** —— 绝不要把私钥材料
写进 TOML。

签名的 `requirements.toml` 里每一个**出现过的** `[telemetry] otel_*` 键都是
**钉定（pin）**（环境变量无法覆盖）。`managed_config.toml` 不是锁 —— 在那里
环境变量仍然优先。钉定 `otel_endpoint` 会剥掉开发者的通用与
按信号端点环境变量，**连同未列出的用户/受管文件同名键**，
只保留你也一并列出的端点。钉定客户端
证书/密钥同样会剥掉开发者端点和未列出的文件同名键。钉定 CA（`otel_certificate`）
**不会**剥掉端点。如果 requirements 列出了
`otel_log_user_prompts` / `otel_log_tool_details` /
`otel_log_assistant_responses` / `otel_log_tool_content` 中的任何一个
而省略了兄弟键，被省略的门控
默认**关闭**（不要指望 prompts→responses 的回退跨过这条
边界）。

外部数据流只导出**日志和指标**（没有面向客户的
traces exporter）。

机群启用靠一份签名的 `requirements.toml`（目的地、exporter
和内容门控一起搞定）。`user.email` 不是钉定键 —— 它跟随
OAuth/网关身份。Headers 由启动器在 strip 之后保留在进程环境中，
绝不会写进这份 TOML。

## 启动期抑制（为什么头几秒什么都没有）

因为 xAI 可以在整个机群强制关闭这条数据流，CLI 在启动时先保持
不发送，直到确认那个开关是否被设置 —— 它会从 `/v1/settings` 拉取
机群策略，然后才开始导出。健康的
环境里这远不到一秒，完全不可见。

**等待是有界的**，因此一个连不上 xAI 的部署照样导出：

- 如果机群策略根本不可能生效 —— `[features] remote_fetch = false`，或
  `[endpoints] cli_chat_proxy_base_url` 指向 xAI 之外的地址 —— 数据流
  立即启动，由你的本地配置决定。
- 如果策略拉取失败或始终不结束（被防火墙挡住的主机、离线的
  笔记本），尝试结束后照样开始发送，且无论如何都不晚于
  启动后 30 秒。

之后才到达的机群策略仍然生效；它只能
*收紧*（关闭数据流或强制关闭内容门控），绝不能开启
你本地配置没有开启的东西。

如果你的收集器完全收不到数据，请查看调试日志
（`chaos --debug`）里的 `external otel:` 行 —— 它们记录了数据流是否
解析出了配置，以及它是在导出还是被抑制。

## 资源属性

| 属性 | 取值 |
|---|---|
| `service.name` | `grok-cli` |
| `service.version`, `client.version` | 构建/客户端版本 |
| `app.entrypoint` | `cli` \| `headless` \| `agent` |
| `terminal.type` | 终端模拟器品牌 |
| `grok_code.schema.version` | `v1` |

身份属性（`user.id`，以及已知时的 `organization.id` / `team.id` /
`deployment.id`）在认证完成后附加到每个指标数据点和每个
事件上。只要会话以带
非空地址的 OAuth 或网关账号登录，`user.email` 就会附加到日志**和**
指标上 —— 它是身份标识而非内容门控，且绝不会取自
git、API key 或部署 key。`prompt.id`（每次提示的 UUID）只出现在
事件上，绝不会出现在指标上。

## 指标（meter 作用域 `ai.xai.grok_code`）

| 指标 | 单位 | 属性 |
|---|---|---|
| `grok_code.session.count` | `{session}` | 仅基础属性 |
| `grok_code.token.usage` | `{token}` | `type` = `input` \| `output` \| `reasoning` \| `cache_read`；`model` |
| `grok_code.turn.count` | `{turn}` | `outcome` = `completed` \| `cancelled` \| `error`；`model` |
| `grok_code.turn.ttft` | `ms` | `model` |
| `grok_code.turn.ttfm` | `ms` | `model` |
| `grok_code.tool.decision` | `{decision}` | `tool_name`、`decision` = `allow` \| `deny` \| `cancelled` \| `followup`、`access_kind`、`permission_mode` |
| `grok_code.tool.usage` | `{call}` | `tool_name`、`outcome` |
| `grok_code.error.count` | `{error}` | `error_category`、`model` |
| `grok_code.startup.total` | `ms` | `outcome` = `ok` \| `timeout` \| `error`；`auth_mode` |
| `grok_code.startup.interactive` | `ms` | `auth_mode` |
| `grok_code.startup.phase_duration` | `ms` | `phase`、`outcome`、`auth_mode` |
| `grok_code.startup.timeout` | `{timeout}` | `stuck_in`、`auth_mode` |

`startup.total` 度量进程启动到可用会话的耗时，每个进程记录一次；
`outcome` = `timeout` 或 `error` 表示启动结束时还没有可用会话。
`startup.interactive` 记录进程启动到实时循环确认
写入的第一帧，每个进程一次。
`phase_duration` 按步骤拆解连接尝试（`config_load`、
`managed_policy`、`bootstrap`、`model_catalog`、`worker_spawn`、
`leader_connect`、`acp_initialize`、`eager_auth`）；请按它的 `outcome`
（`ok` | `timeout` | `cancelled` | `error`）过滤，这样被截断的样本才不会拉偏
`ok` 百分位。后面的 `app_init`
和 `session_create` 阶段出现在日志时间线和摘要
字符串里，而不在这个指标中。超时时的 `stuck_in` 指出尚未完成的
那个步骤。它往往不是耗时最长的那个步骤，因为
一个不停顿运行完的步骤会在超时被记录之前完成。Chaos 打印的
错误消息反而点名耗时最长的步骤，所以同一个
超时里两者可能指向不同的
步骤。用 `phase_duration` 来比对它们。
`auth_mode` 为 `personal`、`team`、`deployment` 或 `unknown`：
启动成本因类别而异，比较前请先按它拆分。

`turn.ttft` 是从回合开始到任意通道（推理、文本或
工具调用）第一个 token 的时间，`turn.ttfm` 则是从回合开始到
第一条助手文本消息（不含推理和工具调用）的时间，每个回合
在同一时钟上取一个样本，因此 `ttft` 绝不会超过 `ttfm`。没有产生
模型输出的回合不记录 `ttft`；只有推理或只有工具调用的回合记录
`ttft` 但没有 `ttfm`。

没有 `cost.usage` 指标：请把 `grok_code.token.usage` 与你自己的
价格表做 join。`lines_of_code.count` 和 `active_time.total` 计划在
后续阶段提供。

`tool_name` 的取值：内置工具名称原样通过；MCP 工具折叠为
`mcp_tool`，其他非内置工具折叠为 `custom_tool`，除非设置
`OTEL_LOG_TOOL_DETAILS=1`。

## 事件（OTLP 日志记录）

每个事件都带 `event.sequence`、`session.id`、`turn_number`（回合内
序号）、`prompt.id`，外加身份属性。门控图例：**details** =
需要 `OTEL_LOG_TOOL_DETAILS`，**prompts** =
需要 `OTEL_LOG_USER_PROMPTS`，**responses** = 需要
`OTEL_LOG_ASSISTANT_RESPONSES`（未设置时跟随 prompts
门控），**content** = 需要 `OTEL_LOG_TOOL_CONTENT`
（独立于 details；默认关闭）；其余属性只要
数据流处于活动状态就始终导出。

| `event.name` | 属性 |
|---|---|
| `grok_code.session_start` | `model`、`permission_mode`、`mcp_server_count`、`plugin_count`、`skill_count`、`hook_count`、`memory_enabled`、`is_git_repo`、`client_identifier` |
| `grok_code.session_end` | `duration_secs`、`turn_count`、`tool_call_count`、`compaction_count`、`model` |
| `grok_code.user_prompt` | `prompt_length`、`model`、`screen_mode?`（`fullscreen` \| `inline` \| `minimal` \| `headless` \| `other`）、`command_name?`（斜杠/技能名，常开元数据）；`prompt`（**prompts**） |
| `grok_code.assistant_response` | `response_length`；`response`（**responses**；纯工具回合上省略） |
| `grok_code.turn_completed` | `outcome`、`duration_ms`、`tool_call_count`、`model`、`error_category?`、`cancellation_category?` |
| `grok_code.api_request` | `model`、`duration_ms`、`stop_reason?`、`input_tokens`、`output_tokens`、`reasoning_tokens`、`cache_read_tokens` |
| `grok_code.api_error` | `error_category`、`model`、`status_code?`、`duration_ms?` |
| `grok_code.tool_result` | `tool_name`、`outcome`、`success`、`duration_ms`、`file_extension`、`tool_use_id`；`mcp_tool.name` / `mcp_server.name` 折叠形式始终输出（**details** 下原样）；`tool_parameters` 预览 + `file_path`（**details**）；`tool_input`、`tool_output`、`full_command`、`error_message`（**content**） |
| `grok_code.tool_decision` | `tool_name`、`decision`、`access_kind`、`permission_mode`、`source`、`tool_use_id`；MCP 名称折叠形式始终输出（**details** 下原样）；`tool_parameters` 预览（**details**）；`tool_input`、`full_command`（**content**） |
| `grok_code.mcp_server_connection` | `status`、`transport_type`、`duration_ms`、`tool_count?`、`error_type?`；`mcp_server.name` 折叠形式始终输出（**details** 下原样）；`error_message`（**content**） |
| `grok_code.permission_mode_changed` | `from_mode`、`to_mode`、`trigger` |
| `grok_code.skill_activated` | `skill_source`、`trigger` = `slash_command` \| `skill_md_read` \| `skill_tool`；`skill.name`（**详情**） |
| `grok_code.plugin_loaded` | `install_kind?`、`success`、`error_category?`；`plugin_name`（**详情**） |
| `grok_code.compaction` | `duration_ms`、`tokens_before`、`tokens_after`、`model?` |
| `grok_code.subagent` | `phase` = `launched` \| `completed`、`subagent_type?`、`outcome?`、`duration_ms?` |
| `grok_code.auth` | `auth_method` |
| `grok_code.internal_error` | `error_type`（仅类别 —— 无消息、无位置） |
| `grok_code.model_switched` | `from_model`、`to_model`、`success`、`error_code?` |

## 隐私模型

三道相互独立的 fail-closed 机制保护着线上格式：

1. **带类型的 schema**：属性键是封闭枚举；枚举之外的内容
   无法附加。
2. **发送时脱敏**：每个字符串都经过密钥形态清洗和
   主目录清洗，并截断（嵌套工具参数字符串每段
   512→128 字符，DETAILS 的 `tool_parameters` 预览 / CONTENT 的 `error_message`
   为 4 KB，`prompt` / `response` / `tool_input` / `tool_output` /
   `full_command` 为 60 KB）。
3. **导出时校验器**：任何携带非 schema 键、封闭门控键
   或未清洗密钥形态的记录，都会在离开进程之前被丢弃；
   属性键超出 schema 的指标导出会被整体丢弃。

绝不导出：思考/推理文本、原始 API 请求体、`api_key.id`、
机器指纹、订阅层级。提示词文本、助手 `response`、
文件路径、工具参数预览以及 CONTENT 主体（`tool_input`、`tool_output`、
`full_command`、`error_message`）只在其门控开启时导出；第一方
产品分析永远收不到这些主体。
只要 OAuth/网关认证带有地址，`user.email` 就会导出（它不是内容
门控）。

## 收集器配置示例

```yaml
receivers:
  otlp:
    protocols:
      http:
        endpoint: 0.0.0.0:4318
      grpc:
        endpoint: 0.0.0.0:4317

processors:
  batch:

exporters:
  prometheus:
    endpoint: 0.0.0.0:9464

service:
  pipelines:
    metrics:
      receivers: [otlp]
      processors: [batch]
      exporters: [prometheus]
    logs:
      receivers: [otlp]
      processors: [batch]
      exporters: []   # point at your log backend (loki, elasticsearch, …)
```

查询示例（PromQL，配合上面的 Prometheus exporter）：

```promql
# Tokens by model and type across the org, 1h rate
sum by (model, type) (rate(grok_code_token_usage_total[1h]))

# Sessions per team per day
sum by (team_id) (increase(grok_code_session_count_total[1d]))

# Tool-permission denial ratio
sum(rate(grok_code_tool_decision_total{decision="deny"}[1h]))
  / sum(rate(grok_code_tool_decision_total[1h]))
```

## 调试

设置 `OTEL_LOGS_EXPORTER=console` / `OTEL_METRICS_EXPORTER=console` 可以把
脱敏后的记录打印到 **stderr**（在 `agent`/`headless` 入口会被抑制，
以保持采集日志干净）。导出错误不会出现在 TUI 里；请查看
调试日志。
