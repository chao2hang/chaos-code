# 配置参考

本文件随 CLI 一起分发，启动时解包到 `~/.chaos/docs/user-guide/26-config-reference.md`。它是 `config.toml`、`managed_config.toml` 与 `requirements.toml` 的完整字段清单。概念性说明见 [05-configuration.md](05-configuration.md)。

## 如何配置

三个文件用来配置 Chaos，它们由不同的人维护。

| 文件 | 谁写入 | 存放位置 | 用途 |
| --- | --- | --- | --- |
| `config.toml` | 开发者 | `~/.chaos/config.toml`，以及项目中的 `.chaos/config.toml` | 设置个人默认值。这里的任何内容都可以由使用这台机器的人修改。 |
| `managed_config.toml` | 你，通过控制台或部署工具 | `/etc/grok/managed_config.toml` | 向一批机器下发起点配置。开发者自己的文件会覆盖它。 |
| `requirements.toml` | 你（含签名） | `/etc/grok/requirements.toml`，或 macOS 设备管理 | 设置开发者不能修改的值。下文标为 `pin` 的键对其他所有文件、环境变量和命令行都有效。 |

如果你希望别人能调整某些默认值，就用 `managed_config.toml`；如果希望他们不能改，就用 `requirements.toml`。

Chaos 还会读取以下各层，靠后的行优先，除非 requirements 的 pin 或 Managed 列另有说明。

1. 内置默认值。
2. `/etc/grok/managed_config.toml`，然后是 `$CHAOS_HOME/managed_config.toml`（团队默认值；由控制台同步）。
3. `$CHAOS_HOME/config.toml`（你的设置；`/settings` 写在这里）。`$CHAOS_HOME` 默认为 `~/.chaos`。配置根按 `$CHAOS_HOME` → `$GROK_HOME` → 已有 `~/.chaos` → 已有 `~/.grok` → 默认 `~/.chaos` 的顺序解析；旧用户仍可使用 `~/.grok/config.toml`（兼容）。
4. 项目级 `.chaos/config.toml`：只包含 `[mcp_servers]`、`[plugins]`、`[permission]`，以及 `[mcp] max_output_bytes`。
5. `GROK_CONFIG`（内联 JSON）或 `GROK_CONFIG_PATH`（JSON 或 TOML 文件）。仅限白名单内的键。
6. `$CHAOS_HOME/requirements.toml`，然后是 `/etc/grok/requirements.toml`，然后 macOS MDM `ai.x.grok`。管理层。表中标为 `pin` 的键不可覆盖；标为 `yes` 的键在本文件中同样有效。
7. `GROK_*` 环境变量。
8. 命令行标志，例如 `--model`、`--sandbox`、`--yolo`。

运行 `chaos inspect` 或 `chaos inspect --json`，查看哪些文件与值最终生效。

## config.toml

用户级配置位于 `$CHAOS_HOME/config.toml`（默认 `~/.chaos/config.toml`；Windows 为 `%USERPROFILE%\.chaos\config.toml`）。项目级覆盖位于 `.chaos/config.toml`，只贡献 `[mcp_servers]`、`[plugins]`、`[permission]` 与 `[mcp] max_output_bytes`。

**Requirements** 标明同一个键能否在 `requirements.toml` 中设置：`pin` 不可被覆盖（在解析器遵循 pin 的情况下，包括环境变量与 CLI）；`yes` 可在该文件中使用；`—` 不会从 `requirements.toml` 读取。**Managed** 标明团队 `managed_config.toml` 的值生效（`fleet`）还是用户文件胜出（`user`）。

### `agent`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `agent.definition` | `string (path)` | `yes` | `user` | 带有 YAML frontmatter 的代理定义 markdown 文件的路径。 |
| `agent.name` | `string` | `yes` | `user` | 内置或已发现的代理定义名称。等价于 GROK_AGENT 与 `--agent-profile`。 |
| `agent.system_prompt_label` | `string` | `yes` | `user` | 全局系统提示身份；按模型的覆盖优先。 |

### `announcements`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `announcements` | `array of tables` | `—` | `user` | 加载时消费的远程公告负载。不是面向用户手写的表。 |

### `auth`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `auth` | `table` | `yes` | `user` | `[grok_com_config]` 的别名；每个 `grok_com_config.*` 键也都可以写成 `auth.*`。 |
| `auth.auth_provider_command` | `string` | `yes` | `user` | 外部认证二进制文件；stdout 即 token。等价于 GROK_AUTH_PROVIDER_COMMAND；也接受 `grok_com_config.auth_provider_command`。 |
| `auth.auth_provider_label` | `string` | `yes` | `user` | 外部认证提供方的登录按钮标签。等价于 GROK_AUTH_PROVIDER_LABEL；也接受 `grok_com_config.auth_provider_label`。 |
| `auth.auth_token_ttl` | `number` | `yes` | `user` | 返回裸 token 的提供方所用的 token TTL，单位秒。等价于 GROK_AUTH_TOKEN_TTL；也接受 `grok_com_config.auth_token_ttl`。 |
| `auth.disable_api_key_auth` | `boolean` | `pin` | `user` | 拒绝 API-key 认证，使只有部署的 IdP 能登录。等价于 GROK_DISABLE_API_KEY_AUTH；也接受 `grok_com_config.disable_api_key_auth`。 |
| `auth.force_login_team_uuid` | `string / string[]` | `pin` | `user` | 要求登录到这个团队 UUID，或数组中的任意一个；空数组按拒绝处理。等价于 GROK_FORCE_LOGIN_TEAM_ID；也接受 `grok_com_config.force_login_team_uuid`。 |
| `auth.grok_ws_origin` | `string` | `yes` | `user` | grok.com 的 websocket origin。等价于 GROK_WS_ORIGIN；也接受 `grok_com_config.grok_ws_origin`。 |
| `auth.grok_ws_url` | `string` | `yes` | `user` | 中继 websocket URL。等价于 GROK_WS_URL；也接受 `grok_com_config.grok_ws_url`。 |
| `auth.oauth2` | `table` | `yes` | `user` | 企业 OIDC 未设置时使用的 OAuth2 提供方；也接受 `grok_com_config.oauth2`。 |
| `auth.oauth2.client_id` | `string` | `yes` | `user` | OAuth2 client id。等价于 GROK_OAUTH2_CLIENT_ID；也接受 `grok_com_config.oauth2.client_id`。 |
| `auth.oauth2.issuer` | `string` | `yes` | `user` | OAuth2 issuer URL。等价于 GROK_OAUTH2_ISSUER；也接受 `grok_com_config.oauth2.issuer`。 |
| `auth.oauth2.principal_id` | `string` | `yes` | `user` | 设置了 `principal_type` 时要求的 principal id。等价于 GROK_OAUTH2_PRINCIPAL_ID；也接受 `grok_com_config.oauth2.principal_id`。 |
| `auth.oauth2.principal_type` | `string` | `yes` | `user` | token 的 principal 类型，例如 Team。等价于 GROK_OAUTH2_PRINCIPAL_TYPE；也接受 `grok_com_config.oauth2.principal_type`。 |
| `auth.oauth2.referrer` | `string` | `yes` | `user` | OAuth 用量归因用的 referrer。等价于 GROK_OAUTH2_REFERRER；也接受 `grok_com_config.oauth2.referrer`。 |
| `auth.oauth2.scopes` | `string[]` | `yes` | `user` | OAuth2 scopes。等价于 GROK_OAUTH2_SCOPES；也接受 `grok_com_config.oauth2.scopes`。 |
| `auth.oidc` | `table` | `yes` | `user` | 客户 OIDC 身份提供方设置；也接受 `grok_com_config.oidc`。 |
| `auth.oidc.audience` | `string` | `yes` | `user` | 可选的 OIDC audience。等价于 GROK_OIDC_AUDIENCE；也接受 `grok_com_config.oidc.audience`。 |
| `auth.oidc.client_id` | `string` | `yes` | `user` | OIDC client id。等价于 GROK_OIDC_CLIENT_ID；也接受 `grok_com_config.oidc.client_id`。 |
| `auth.oidc.issuer` | `string` | `yes` | `user` | OIDC issuer URL。等价于 GROK_OIDC_ISSUER；也接受 `grok_com_config.oidc.issuer`。 |
| `auth.oidc.scopes` | `string[]` | `yes` | `user` | OIDC scopes。等价于 GROK_OIDC_SCOPES；也接受 `grok_com_config.oidc.scopes`。 |
| `auth.preferred_method` | `api_key / oidc` | `yes` | `user` | 把自动认证固定为单一方法，不做回退；也接受 `grok_com_config.preferred_method`。 |
| `auth.token_header` | `string` | `yes` | `user` | 携带 CLI 认证 token 的 header 名称；默认 `xai-grok-cli`；也接受 `grok_com_config.token_header`。 |

### `auth_provider`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `auth_provider.<name>` | `table` | `yes` | `user` | 具名的凭据 helper，供 `[model.<id>] auth_provider` 使用。 |

### `auto_mode`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `auto_mode.enabled` | `boolean` | `yes` | `user` | 启用 Auto 权限模式。 |

### `campaigns`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `campaigns` | `array of tables` | `yes` | `user` | 具名的 campaign 补丁，应用在 requirements 之下。由部署方发布。 |

### `cli`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `cli.auto_update` | `boolean` | `pin` | `user` | 启动时检查 CLI 更新。等价于 GROK_DISABLE_AUTOUPDATER（用于抑制）。 |
| `cli.channel` | `stable / alpha` | `pin` | `user` | 发布渠道偏好。 |
| `cli.grove_worktree` | `boolean` 或 `grove` / `grove-fuse` / `grove-nfs` / `nfs` / `copy` / `true` / `false` / `1` / `0` / `on` / `off` | `yes` | `user` | 会话 / `-w` 用 Grove 还是复制。默认复制。与创建模式的 `cli.worktree_type` 不同。等价于 `GROK_WORKTREE_TYPE`。分层顺序：请求 → 环境 → 本地 → 远端为 true；然后按最后命中扼杀：远端 `grove_worktree = false` → 复制。远端缺失的设置不算扼杀：本地/环境/请求仍然生效。上游的 `grok clone` 依赖 Grove，本分叉不含该功能（兼容说明）。 |
| `cli.installer` | `string` | `—` | `user` | 上次安装该 CLI 的安装器，用于选择更新路径。 |
| `cli.maximum_version` | `string` | `pin` | `user` | 仍可运行而不被硬性拦截的最高 CLI 版本。等价于 GROK_MAXIMUM_VERSION。 |
| `cli.minimum_version` | `string` | `pin` | `user` | 仍可运行而不被硬性拦截的最低 CLI 版本。等价于 GROK_MINIMUM_VERSION。 |
| `cli.nfs_worktree` | same as `cli.grove_worktree` | `yes` | `user` | `cli.grove_worktree` 的读取别名。 |
| `cli.npm_registry` | `string` | `yes` | `user` | 自动更新器使用的 npm registry。 |
| `cli.required_maximum_version` | `string` | `pin` | `user` | 硬性最高 CLI 版本。等价于 GROK_REQUIRED_MAXIMUM_VERSION。 |
| `cli.required_minimum_version` | `string` | `pin` | `user` | 硬性最低 CLI 版本。等价于 GROK_REQUIRED_MINIMUM_VERSION。 |
| `cli.session_picker_grouped` | `boolean` | `yes` | `user` | 在选择器与 CLI 列表中按仓库分组会话。 |
| `cli.session_registry` | `boolean` | `yes` | `user` | 参与跨进程会话注册表。 |
| `cli.show_tips` | `boolean` | `pin` | `user` | 启动提示。 |
| `cli.use_leader` | `boolean` | `pin` | `user` | 用 leader 进程处理配置重载与 MCP 监视。 |
| `cli.worktree_type` | `string` | `yes` | `user` | 设为 `linked`、`standalone` 或 `git` 时的创建模式。写法 `grove`、`grove-fuse`、`grove-nfs`、`nfs`、`copy` 也会喂给会话 / `-w` 的 Grove 闸门（同 `cli.grove_worktree`）；它们不是创建模式的取值。 |

### `compat`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `compat.claude.agents` | `boolean` | `yes` | `user` | 扫描 CLAUDE.md。等价于 GROK_CLAUDE_AGENTS_ENABLED。 |
| `compat.claude.hooks` | `boolean` | `yes` | `user` | 扫描 Claude hooks。等价于 GROK_CLAUDE_HOOKS_ENABLED。 |
| `compat.claude.mcps` | `boolean` | `yes` | `user` | 扫描 Claude MCP 配置。等价于 GROK_CLAUDE_MCPS_ENABLED。 |
| `compat.claude.rules` | `boolean` | `yes` | `user` | 扫描 Claude rules。等价于 GROK_CLAUDE_RULES_ENABLED。 |
| `compat.claude.skills` | `boolean` | `yes` | `user` | 扫描 Claude skills。等价于 GROK_CLAUDE_SKILLS_ENABLED。 |
| `compat.codex.hooks` | `boolean` | `yes` | `user` | 存在时扫描 Codex hooks。 |
| `compat.codex.skills` | `boolean` | `yes` | `user` | 存在时扫描 Codex skills 目录。 |
| `compat.cursor.agents` | `boolean` | `yes` | `user` | 从 Cursor 兼容来源扫描代理定义。等价于 GROK_CURSOR_AGENTS_ENABLED。 |
| `compat.cursor.hooks` | `boolean` | `yes` | `user` | 扫描 Cursor hooks。等价于 GROK_CURSOR_HOOKS_ENABLED。 |
| `compat.cursor.mcps` | `boolean` | `yes` | `user` | 扫描 Cursor mcp.json。等价于 GROK_CURSOR_MCPS_ENABLED。 |
| `compat.cursor.rules` | `boolean` | `yes` | `user` | 扫描 `.cursor/rules/`。等价于 GROK_CURSOR_RULES_ENABLED。 |
| `compat.cursor.skills` | `boolean` | `yes` | `user` | 扫描 Cursor skills 目录。等价于 GROK_CURSOR_SKILLS_ENABLED。 |

### `dashboard`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `dashboard.enabled` | `boolean` | `yes` | `user` | 显示代理仪表盘。 |
| `dashboard.grouping` | `state / directory` | `yes` | `user` | 仪表盘行的分组方式。 |

### `default_auto_mode`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `default_auto_mode` | `boolean` | `yes` | `user` | 未设置逐会话覆盖时，会话以 auto 权限模式启动。 |

### `diagnostics`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `diagnostics.crash_handler` | `boolean` | `yes` | `user` | 在 `$CHAOS_HOME/crash/` 下写 panic 报告。等价于 GROK_CRASH_HANDLER。 |

### `disable_web_search`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `disable_web_search` | `boolean` | `yes` | `user` | 在本进程中丢弃 web_search 工具。另见 `--disable-web-search`。 |

### `disabled_mcp_servers`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `disabled_mcp_servers` | `string[]` | `yes` | `user` | 要跳过的 MCP server 名称，无需删除对应的 `[mcp_servers]` 块。 |

### `disabled_mcp_tools`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `disabled_mcp_tools` | `map<string, string[]>` | `yes` | `user` | 按服务器名称索引的逐服务器 MCP 工具拒绝列表。 |

### `doom_loop_recovery`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `doom_loop_recovery.enabled` | `boolean` | `yes` | `user` | 对自信的工具调用循环重新采样；设为 false 可禁用。 |

### `endpoints`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `endpoints.cli_chat_proxy_base_url` | `string` | `pin` | `user` | 会话服务 API 的 base URL。 |
| `endpoints.deployment_key` | `string` | `pin` | `user` | 企业部署的管理密钥。另见 GROK_DEPLOYMENT_KEY。 |
| `endpoints.feedback_base_url` | `string` | `yes` | `user` | 反馈提交的去向。另见 GROK_FEEDBACK_BASE_URL。 |
| `endpoints.managed_config_url` | `string` | `yes` | `user` | 覆盖受管配置端点。另见 GROK_MANAGED_CONFIG_URL。 |
| `endpoints.models_base_url` | `string` | `pin` | `user` | 自定义推理 base URL。另见 GROK_MODELS_BASE_URL。 |
| `endpoints.models_list_url` | `string` | `pin` | `user` | 覆盖模型列表 URL。另见 GROK_MODELS_LIST_URL。别名 `models_endpoint`。 |
| `endpoints.trace_upload_bucket` | `string` | `yes` | `user` | 直连的 gs:// 或 s3:// trace 桶；绕过代理。另见 GROK_TRACE_UPLOAD_BUCKET。 |
| `endpoints.trace_upload_credentials` | `string` | `yes` | `user` | 该桶内联的 GCS 服务账号 JSON 或 AWS 凭据；优先于 `trace_upload_credentials_file`，且没有对应的环境变量。 |
| `endpoints.trace_upload_credentials_file` | `string (path)` | `yes` | `user` | 该桶的 GCS 服务账号 JSON 或 AWS 凭据文件路径。另见 GROK_TRACE_UPLOAD_CREDENTIALS_FILE。 |
| `endpoints.trace_upload_endpoint_url` | `string` | `yes` | `user` | s3:// 桶上传用的自定义 S3 兼容端点。另见 GROK_TRACE_UPLOAD_ENDPOINT_URL。 |
| `endpoints.trace_upload_region` | `string` | `yes` | `user` | s3:// 桶上传的 AWS 区域；默认 us-east-1。另见 GROK_TRACE_UPLOAD_REGION。 |
| `endpoints.trace_upload_url` | `string` | `pin` | `user` | 未设置直连桶时 trace 的代理目的地。另见 GROK_TRACE_UPLOAD_URL。 |
| `endpoints.xai_api_base_url` | `string` | `pin` | `user` | 公开的 xAI API base。另见 GROK_XAI_API_BASE_URL。 |

### `features`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `features.active_agent_messages` | `boolean` | `pin` | `user` | 启用或禁用 `active_agent_messages`。默认 false。另见 `GROK_ACTIVE_AGENT_MESSAGES`。 |
| `features.ask_user_question` | `boolean` | `pin` | `user` | 启用或禁用 `ask_user_question`。默认 true。另见 `GROK_ASK_USER_QUESTION`。 |
| `features.auto_wake` | `boolean` | `pin` | `user` | 启用或禁用 `auto_wake`。默认 true。另见 `GROK_AUTO_WAKE`。 |
| `features.backend_tools` | `boolean` | `pin` | `user` | 启用或禁用 `backend_tools`。默认 true。另见 `GROK_BACKEND_SEARCH`。 |
| `features.campaigns` | `boolean` | `yes` | `user` | 启用远程 campaign 补丁。即使 requirements 将其设为 true，`GROK_CAMPAIGNS=0` 仍可禁用。 |
| `features.cancel_rewind` | `boolean` | `pin` | `user` | 启用或禁用 `cancel_rewind`。默认 true。另见 `GROK_CANCEL_REWIND`。 |
| `features.codebase_indexing` | `boolean / string[]` | `pin` | `user` | 代码库图谱索引；true 时索引 git 仓库，也可传入 include/exclude glob。 |
| `features.compaction_detail` | `none / minimal / balanced / verbose` | `yes` | `user` | `segments` 压缩的逐字细节级别。另见 GROK_COMPACTION_DETAIL。 |
| `features.compaction_mode` | `summary / transcript / segments` | `yes` | `user` | 压缩策略。另见 GROK_COMPACTION_MODE。 |
| `features.compaction_tool_choice` | `string` | `yes` | `user` | 压缩期间使用的工具选择提示。 |
| `features.compaction_verbatim_input` | `boolean` | `pin` | `user` | 启用或禁用 `compaction_verbatim_input`。默认 true。另见 `GROK_COMPACTION_VERBATIM_INPUT`。 |
| `features.dock` | `boolean` | `pin` | `user` | 启用或禁用 `dock`。默认 false。另见 `GROK_DOCK`。 |
| `features.feedback` | `boolean` | `pin` | `user` | 启用或禁用 `feedback`。本分叉默认 false：反馈提交依赖上游服务，自定义模型提供方用不上。另见 `GROK_FEEDBACK_ENABLED`。 |
| `features.feedback_trace_card` | `boolean` | `pin` | `user` | 在 `/feedback` 之后显示 trace 上传征询问题。默认 false。另见 `GROK_FEEDBACK_TRACE_CARD`。 |
| `features.image_edit_model_override` | `string` | `yes` | `user` | image_edit 使用的 Imagine 模型 id。 |
| `features.image_gen` | `boolean` | `pin` | `user` | 启用 image_gen / `/imagine`。 |
| `features.image_gen_model_override` | `string` | `yes` | `user` | image_gen 使用的 Imagine 模型 id。留空则回退到远端配置的默认值。 |
| `features.lsp_tools` | `boolean` | `pin` | `user` | 启用或禁用 `lsp_tools`。默认 false。另见 `GROK_LSP_TOOLS`。 |
| `features.managed_config` | `boolean` | `yes` | `user` | 从部署拉取 managed_config.toml 与 requirements.toml。 |
| `features.mcp_auto_restart` | `boolean` | `yes` | `user` | 传输失败后自动重启 stdio MCP 服务器。另见 GROK_MCP_AUTO_RESTART。 |
| `features.mcp_liveness_watchers` | `boolean` | `yes` | `user` | 轮询 MCP 传输并推送 server_status 更新。设为 false 即紧急开关。 |
| `features.mcp_push_server_status` | `boolean` | `yes` | `user` | Pager 订阅 MCP server_status 推送。启动时进程环境 GROK_MCP_PUSH_SERVER_STATUS 优先。 |
| `features.mcp_recursive_config_watch` | `boolean` | `yes` | `user` | 监视 `<cwd>/` 与 `<cwd>/.chaos/` 的项目 MCP 配置改动。名字有误导性；监视是非递归的。 |
| `features.non_git_warning` | `boolean` | `yes` | `user` | 在 Git 仓库之外启动 Grok 时显示阻断式警告。 |
| `features.remember_mode` | `boolean` | `—` | `—` | 跨会话记住上次的权限模式。仅从用户 `config.toml` 读取。 |
| `features.remote_fetch` | `boolean` | `pin` | `fleet` | 固定远端模型目录与资源拉取。两者都设置时受管配置优先于用户文件。 |
| `features.repo_status_in_system_prompt` | `boolean` | `pin` | `user` | 启用或禁用 `repo_status_in_system_prompt`。默认 true。另见 `GROK_REPO_STATUS_IN_SYSTEM_PROMPT`。 |
| `features.session_recap` | `boolean` | `pin` | `user` | 启用或禁用 `session_recap`。默认 true。另见 `GROK_SESSION_RECAP`。 |
| `features.session_search` | `boolean` | `pin` | `user` | 启用或禁用 `session_search`。默认 true。另见 `GROK_SESSION_SEARCH`。 |
| `features.subagent_worktree_snapshot` | `boolean` | `pin` | `user` | 启用或禁用 `subagent_worktree_snapshot`。默认 false。另见 `GROK_SUBAGENT_WORKTREE_SNAPSHOT`。 |
| `features.support_permission` | `boolean` | `yes` | `user` | 允许代理为工具执行请求权限。 |
| `features.telemetry` | `boolean / session_metrics / off` | `pin` | `user` | 产品遥测模式。企业默认为 off。 |
| `features.title_refresh` | `boolean` | `pin` | `user` | 会话早期的自动标题刷新。在 requirements 中 pin 该键可压过 GROK_TITLE_REFRESH。 |
| `features.turn_summary` | `boolean` | `pin` | `user` | 启用或禁用 `turn_summary`。默认 true。另见 `GROK_TURN_SUMMARY`。 |
| `features.two_pass_compaction` | `boolean` | `pin` | `user` | 启用或禁用 `two_pass_compaction`。本分叉默认 false（保持历史行为，需显式开启）。另见 `GROK_TWO_PASS_COMPACTION`。 |
| `features.video_gen` | `boolean` | `pin` | `user` | 启用视频工具 / `/imagine-video`。 |
| `features.voice_mode` | `boolean` | `pin` | `user` | 启用或禁用 `voice_mode`。默认 true。另见 `GROK_VOICE_MODE`。 |
| `features.web_fetch` | `boolean` | `pin` | `user` | 启用或禁用 `web_fetch`。默认 false。另见 `GROK_WEB_FETCH`。 |
| `features.write_file` | `boolean` | `pin` | `user` | 启用或禁用 `write_file`。默认 true。另见 `GROK_WRITE_FILE`。 |
| `features.zdr_access_enabled` | `boolean` | `pin` | `user` | 团队启用 Zero Data Retention 时宣传与 ZDR 不兼容的工具。另见 `GROK_ZDR_ACCESS_ENABLED`。 |

### `feedback`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `feedback.user.command` | `string` | `yes` | `user` | 为反馈提交打印姓名与邮箱 JSON 的 shell 命令。 |
| `feedback.user.email` | `string[]` | `yes` | `user` | 反馈作者邮箱的来源（`git_email` 或字面值）。 |
| `feedback.user.name` | `string[]` | `yes` | `user` | 反馈作者姓名的来源（`os_user` 或字面值）。 |

### `goal`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `goal.enabled` | `boolean` | `yes` | `user` | 启用 `/goal`。 |

### `grok_com_config`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `grok_com_config` | `table` | `yes` | `user` | Grok.com websocket 与 OAuth/OIDC 设置。`[auth]` 是别名。 |
| `grok_com_config.auth_provider_command` | `string` | `yes` | `user` | 外部认证二进制文件；stdout 即 token。另见 GROK_AUTH_PROVIDER_COMMAND。 |
| `grok_com_config.auth_provider_label` | `string` | `yes` | `user` | 外部认证提供方的登录按钮标签。另见 GROK_AUTH_PROVIDER_LABEL。 |
| `grok_com_config.auth_token_ttl` | `number` | `yes` | `user` | 返回裸 token 的提供方所用的 token TTL，单位秒。另见 GROK_AUTH_TOKEN_TTL。 |
| `grok_com_config.disable_api_key_auth` | `boolean` | `pin` | `user` | 拒绝 API-key 认证，使只有部署的 IdP 能登录。另见 GROK_DISABLE_API_KEY_AUTH。 |
| `grok_com_config.force_login_team_uuid` | `string / string[]` | `pin` | `user` | 要求登录到这个团队 UUID，或数组中的任意一个；空数组按拒绝处理。另见 GROK_FORCE_LOGIN_TEAM_ID。 |
| `grok_com_config.grok_ws_origin` | `string` | `yes` | `user` | grok.com 的 websocket origin。另见 GROK_WS_ORIGIN。 |
| `grok_com_config.grok_ws_url` | `string` | `yes` | `user` | 中继 websocket URL。另见 GROK_WS_URL。 |
| `grok_com_config.oauth2` | `table` | `yes` | `user` | 企业 OIDC 未设置时使用的 OAuth2 提供方。 |
| `grok_com_config.oauth2.client_id` | `string` | `yes` | `user` | OAuth2 client id。另见 GROK_OAUTH2_CLIENT_ID。 |
| `grok_com_config.oauth2.issuer` | `string` | `yes` | `user` | OAuth2 issuer URL。另见 GROK_OAUTH2_ISSUER。 |
| `grok_com_config.oauth2.principal_id` | `string` | `yes` | `user` | 设置了 `principal_type` 时要求的 principal id。另见 GROK_OAUTH2_PRINCIPAL_ID。 |
| `grok_com_config.oauth2.principal_type` | `string` | `yes` | `user` | token 的 principal 类型，例如 Team。另见 GROK_OAUTH2_PRINCIPAL_TYPE。 |
| `grok_com_config.oauth2.referrer` | `string` | `yes` | `user` | OAuth 用量归因用的 referrer。另见 GROK_OAUTH2_REFERRER。 |
| `grok_com_config.oauth2.scopes` | `string[]` | `yes` | `user` | OAuth2 scopes。另见 GROK_OAUTH2_SCOPES。 |
| `grok_com_config.oidc` | `table` | `yes` | `user` | 客户 OIDC 身份提供方设置。 |
| `grok_com_config.oidc.audience` | `string` | `yes` | `user` | 可选的 OIDC audience。另见 GROK_OIDC_AUDIENCE。 |
| `grok_com_config.oidc.client_id` | `string` | `yes` | `user` | OIDC client id。另见 GROK_OIDC_CLIENT_ID。 |
| `grok_com_config.oidc.issuer` | `string` | `yes` | `user` | OIDC issuer URL。另见 GROK_OIDC_ISSUER。 |
| `grok_com_config.oidc.scopes` | `string[]` | `yes` | `user` | OIDC scopes。另见 GROK_OIDC_SCOPES。 |
| `grok_com_config.preferred_method` | `api_key / oidc` | `yes` | `user` | 把自动认证固定为单一方法，不做回退。 |
| `grok_com_config.token_header` | `string` | `yes` | `user` | 携带 CLI 认证 token 的 header 名称；默认 `xai-grok-cli`。 |

### `harness`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `harness.block_for_upload` | `boolean` | `yes` | `user` | 阻塞回合结束，直到工作区快照上传完成。 |
| `harness.disable_workspace_teleport` | `boolean` | `pin` | `user` | 逐回合工作区快照的紧急开关。 |

### `hints`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `hints.fork_worktree_mode` | `ask / always / never` | `yes` | `user` | `/fork` 是否提供 worktree。 |
| `hints.new_session_worktree_mode` | `ask / always / never` | `yes` | `user` | `/new` 是否提供 worktree。 |

### `hooks`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `hooks.<event>` | `array of tables` | `yes` | `user` | 生命周期事件（如 PreToolUse 或 Stop）的匹配器组。见 Hooks。 |
| `hooks.<event>[].hooks[].command` | `string` | `yes` | `user` | 此 hook 要运行的命令。加载时不展开 `$VAR`。 |
| `hooks.<event>[].hooks[].type` | `command` | `yes` | `user` | hook 处理器类型。支持 command hook。 |
| `hooks.<event>[].matcher` | `string` | `yes` | `user` | 此 hook 组的工具名匹配器。 |

### `managed_mcps`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `managed_mcps.enabled` | `boolean` | `pin` | `user` | 启动时拉取受管 MCP 配置。也可用 GROK_MANAGED_MCPS_ENABLED。 |
| `managed_mcps.gateway_tools_enabled` | `boolean` | `yes` | `user` | 暴露受管 MCP 网关工具。也可用 GROK_MANAGED_MCP_GATEWAY_TOOLS_ENABLED。 |

### `marketplace`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `marketplace.sources` | `array of tables` | `yes` | `user` | `[[marketplace.sources]]` 插件市场仓库。 |
| `marketplace.require_sha` | `boolean` | `yes` | `user` | 只紧不松：远程插件的安装与更新必须钉住完整 commit sha。也可用 `GROK_MARKETPLACE_REQUIRE_SHA`。本键与该环境变量都无法重新关掉这道闸。 |

### `mcp`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `mcp.max_output_bytes` | `number` | `yes` | `user` | MCP 工具输出的大小上限，单位字节。项目文件可以设置。 |

### `mcp_servers`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `mcp_servers.<name>.args` | `string[]` | `yes` | `user` | stdio 或 HTTP MCP 服务器上的 `[mcp_servers.<name>]` `args`。 |
| `mcp_servers.<name>.bearer_token_env_var` | `string` | `yes` | `user` | stdio 或 HTTP MCP 服务器上的 `[mcp_servers.<name>]` `bearer_token_env_var`。 |
| `mcp_servers.<name>.command` | `string` | `yes` | `user` | stdio 或 HTTP MCP 服务器上的 `[mcp_servers.<name>]` `command`。 |
| `mcp_servers.<name>.cwd` | `string` | `yes` | `user` | stdio 或 HTTP MCP 服务器上的 `[mcp_servers.<name>]` `cwd`。 |
| `mcp_servers.<name>.enabled` | `boolean` | `yes` | `user` | stdio 或 HTTP MCP 服务器上的 `[mcp_servers.<name>]` `enabled`。 |
| `mcp_servers.<name>.env` | `table` | `yes` | `user` | stdio 或 HTTP MCP 服务器上的 `[mcp_servers.<name>]` `env`。 |
| `mcp_servers.<name>.expose_image_base64` | `boolean` | `yes` | `user` | stdio 或 HTTP MCP 服务器上的 `[mcp_servers.<name>]` `expose_image_base64`。 |
| `mcp_servers.<name>.headers` | `table` | `yes` | `user` | stdio 或 HTTP MCP 服务器上的 `[mcp_servers.<name>]` `headers`。 |
| `mcp_servers.<name>.oauth` | `table` | `yes` | `user` | stdio 或 HTTP MCP 服务器上的 `[mcp_servers.<name>]` `oauth`。 |
| `mcp_servers.<name>.oauth_client_id` | `string` | `yes` | `user` | stdio 或 HTTP MCP 服务器上的 `[mcp_servers.<name>]` `oauth_client_id`。 |
| `mcp_servers.<name>.oauth_client_secret_env_var` | `string` | `yes` | `user` | stdio 或 HTTP MCP 服务器上的 `[mcp_servers.<name>]` `oauth_client_secret_env_var`。 |
| `mcp_servers.<name>.oauth_scopes` | `string[]` | `yes` | `user` | stdio 或 HTTP MCP 服务器上的 `[mcp_servers.<name>]` `oauth_scopes`。 |
| `mcp_servers.<name>.setup` | `table` | `yes` | `user` | stdio 或 HTTP MCP 服务器上的 `[mcp_servers.<name>]` `setup`。 |
| `mcp_servers.<name>.startup_timeout_sec` | `number` | `yes` | `user` | stdio 或 HTTP MCP 服务器上的 `[mcp_servers.<name>]` `startup_timeout_sec`。 |
| `mcp_servers.<name>.tool_timeout_sec` | `number` | `yes` | `user` | stdio 或 HTTP MCP 服务器上的 `[mcp_servers.<name>]` `tool_timeout_sec`。 |
| `mcp_servers.<name>.tool_timeouts` | `table` | `yes` | `user` | stdio 或 HTTP MCP 服务器上的 `[mcp_servers.<name>]` `tool_timeouts`。 |
| `mcp_servers.<name>.type` | `string` | `yes` | `user` | stdio 或 HTTP MCP 服务器上的 `[mcp_servers.<name>]` `type`。 |
| `mcp_servers.<name>.url` | `string` | `yes` | `user` | stdio 或 HTTP MCP 服务器上的 `[mcp_servers.<name>]` `url`。 |

### `memory`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `memory.enabled` | `boolean` | `pin` | `user` | 跨会话记忆总开关。另见 GROK_MEMORY。 |

### `model`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `model.<id>` | `table` | `yes` | `user` | 按模型覆盖或 BYOK 定义。优先用 `env_key`，而不是内联 `api_key`。 |
| `model.<id>.agent_type` | `string` | `yes` | `user` | 与该模型关联的代理定义类型。 |
| `model.<id>.api_backend` | `chat_completions / responses / messages` | `yes` | `user` | 该模型使用的线上协议。 |
| `model.<id>.api_base_url` | `string` | `yes` | `user` | 与 XAI_API_KEY 解析配合使用的备用 API 地址。 |
| `model.<id>.api_key` | `string` | `yes` | `user` | 内联 API key。优先用 `env_key`。这不是能放进共享仓库的机密。 |
| `model.<id>.auth_provider` | `string` | `yes` | `user` | 为该模型签发 bearer token 的 `[auth_provider.<name>]` 助手名称。 |
| `model.<id>.auto_compact_threshold_percent` | `integer` | `yes` | `user` | 按模型的自动压缩阈值（0-100）。 |
| `model.<id>.base_url` | `string` | `yes` | `user` | Provider 端点基址。 |
| `model.<id>.compaction_at_tokens` | `number / table` | `yes` | `user` | 触发该模型压缩的 token 阈值。 |
| `model.<id>.compactions_remaining` | `string / table` | `yes` | `user` | 压缩后剩余上下文的发送方式。别名 `send_compactions_remaining`。 |
| `model.<id>.context_window` | `number` | `yes` | `user` | 上下文窗口 token 数；决定自动压缩时机。 |
| `model.<id>.description` | `string` | `yes` | `user` | 可选的说明文字，显示在选择器中。 |
| `model.<id>.env_http_headers` | `map<string,string>` | `yes` | `user` | 设置后从环境变量填充的 HTTP 头。 |
| `model.<id>.env_key` | `string / string[]` | `yes` | `user` | 存放 Provider API key 的环境变量名（可多个）。 |
| `model.<id>.extra_headers` | `map<string,string>` | `yes` | `user` | 该模型每次请求所用的头。 |
| `model.<id>.hidden` | `boolean` | `yes` | `user` | 在选择器中隐藏该模型。仍可用 `-m` 选用。 |
| `model.<id>.inference_idle_timeout_secs` | `number` | `yes` | `user` | 该模型流式推理的空闲超时。 |
| `model.<id>.max_completion_tokens` | `number` | `yes` | `user` | 按模型的最大补全 token 数。 |
| `model.<id>.max_retries` | `number` | `yes` | `user` | 该模型的推理重试次数。 |
| `model.<id>.model` | `string` | `yes` | `user` | 发给 API 的模型 id。 |
| `model.<id>.model_family` | `string` | `yes` | `user` | 用于压缩与能力分组的家族 id。 |
| `model.<id>.model_provider` | `string` | `yes` | `user` | 该模型所用的具名 `[model_providers.<name>]` Provider id。 |
| `model.<id>.name` | `string` | `yes` | `user` | 模型选择器中显示的标签。 |
| `model.<id>.query_params` | `map<string,string>` | `yes` | `user` | 该模型请求上的额外查询参数。 |
| `model.<id>.reasoning_effort` | `string` | `yes` | `user` | 已弃用的按模型推理投入；请改用 `reasoning_efforts`。 |
| `model.<id>.reasoning_efforts` | `array of tables` | `yes` | `user` | 该模型允许的推理投入取值。 |
| `model.<id>.show_model_fingerprint` | `boolean` | `yes` | `user` | 存在时在界面里显示 Provider 的模型指纹。 |
| `model.<id>.stream_tool_calls` | `boolean` | `yes` | `user` | 按模型的工具调用流式请求形态。 |
| `model.<id>.supported_in_api` | `boolean` | `yes` | `user` | 该目录条目是否作为公开 API 模型提供。 |
| `model.<id>.supports_backend_search` | `boolean` | `yes` | `user` | 该端点是否支持由上游托管的服务端搜索工具。 |
| `model.<id>.supports_reasoning_effort` | `boolean` | `yes` | `user` | 已弃用；请改用 `reasoning_efforts`。 |
| `model.<id>.system_prompt_label` | `string` | `yes` | `user` | 按模型的系统提示身份标签。 |
| `model.<id>.temperature` | `number` | `yes` | `user` | 按模型的采样温度。 |
| `model.<id>.top_p` | `number` | `yes` | `user` | 按模型的 top_p。 |
| `model.<id>.use_concise` | `boolean` | `yes` | `user` | 该模型使用精简的工具描述包。 |

### `model_providers`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `model_providers.<name>` | `table` | `yes` | `user` | 具名自定义模型 Provider 定义。 |

### `models`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `models.agent_type` | `string` | `yes` | `user` | 没有按模型覆盖时回退的 agent_type。 |
| `models.allowed_models` | `string[]` | `pin` | `user` | 模型选择器、默认模型与 `-m` 的 glob 白名单。留空表示不限制。 |
| `models.default` | `string` | `pin` | `user` | 新会话使用的模型。另见 `GROK_DEFAULT_MODEL`、`--model`、`-m`。 |
| `models.default_reasoning_effort` | `string` | `yes` | `user` | 默认模型支持时，它所用的默认推理投入。 |
| `models.disabled_models` | `string[]` | `yes` | `user` | 从目录中移除这些模型 ID。优先于 `hidden_models`。 |
| `models.extra_headers` | `map<string,string>` | `yes` | `user` | 应用于所有模型的请求头；按模型的键优先。 |
| `models.hidden_models` | `string[]` | `yes` | `user` | 在选择器中隐藏这些模型 ID；`-m` 仍可选中它们。 |
| `models.image_description` | `string` | `yes` | `user` | 用于转写用户所提供图片的视觉模型。 |
| `models.inference_idle_timeout_secs` | `number` | `yes` | `user` | 模型未设置时，流式推理的全局空闲超时。 |
| `models.max_completion_tokens` | `number` | `yes` | `user` | 模型未设置时，最大补全 token 的全局默认值。 |
| `models.max_retries` | `number` | `yes` | `user` | 模型未设置时，推理重试的全局默认值。 |
| `models.prompt_suggestion` | `string` | `yes` | `user` | 下一条提示的幽灵文本所用模型。未设置时先回退到远端配置，再到会话模型。 |
| `models.session_summary` | `string` | `yes` | `user` | 用于会话标题与摘要的模型。 |
| `models.stream_tool_calls` | `boolean` | `yes` | `user` | 工具调用流式请求的全局形态；某些 BYOK 端点需要设为 false。 |
| `models.temperature` | `number` | `yes` | `user` | 模型未设置时，采样温度的全局默认值。 |
| `models.top_p` | `number` | `yes` | `user` | 模型未设置时，top_p 的全局默认值。 |
| `models.web_search` | `string` | `pin` | `user` | 客户端 `web_search` 工具使用的模型。另见 `GROK_WEB_SEARCH_MODEL`。 |

### `path_not_found_hints`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `path_not_found_hints` | `boolean` | `yes` | `user` | 为「路径不存在」错误补充当前目录提示与相近名称建议。 |

### `paths`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `paths.extra_rule_dirs` | `string[]` | `yes` | `user` | 更多规则目录（每个目录内含 `*.md`）。 |
| `paths.extra_skill_dirs` | `string[]` | `yes` | `user` | 更多技能目录（每个目录内含 `<skill>/SKILL.md`）。 |

### `permission`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `permission.allow` | `string[]` | `yes` | `user` | 紧凑形式的 allow 规则，例如 `Bash(git *)`。deny 优先于 ask，ask 优先于 allow。项目文件可以设置。 |
| `permission.ask` | `string[]` | `yes` | `user` | 紧凑形式的 ask 规则。项目文件可以设置。 |
| `permission.deny` | `string[]` | `yes` | `user` | 紧凑形式的 deny 规则。项目文件可以设置。 |
| `permission.rules` | `array of tables` | `yes` | `user` | 冗长形式的 action/tool/pattern 对象规则。项目文件可以设置。 |

### `plugins`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `plugins.disabled` | `string[]` | `yes` | `user` | 要发现但不加载的插件 ID。项目文件可以设置。 |
| `plugins.enabled` | `string[]` | `yes` | `user` | 要启用的插件 ID；默认关闭的项目插件需要它。 |
| `plugins.paths` | `string[]` | `yes` | `user` | 额外的插件目录。文件夹受信任时，项目文件可以设置。 |

### `privacy`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `privacy.privacy_banner_acked` | `string` | `—` | `—` | 本地隐私横幅被关闭时的 RFC 3339 UTC 时间戳。TUI 只读用户的 `config.toml`。 |

### `relay`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `relay.enabled` | `boolean` | `yes` | `user` | 启用会话中继同步。 |

### `sandbox`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `sandbox.auto_allow_bash` | `boolean` | `pin` | `user` | 沙箱 profile 生效时跳过 bash 权限提示。另见 GROK_SANDBOX_AUTO_ALLOW_BASH。 |
| `sandbox.profile` | `off / workspace / read-only / strict / string` | `pin` | `user` | 文件系统沙箱 profile。另见 `--sandbox` 与 GROK_SANDBOX。 |

### `session`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `session.auto_compact_threshold_percent` | `integer` | `yes` | `user` | 上下文用量达到该百分比时自动压缩（0–100）。 |
| `session.load_envrc` | `boolean` | `yes` | `user` | 把 `.envrc` 变量注入 bash。 |

### `shell_environment_policy`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `shell_environment_policy.exclude` | `string[]` | `yes` | `user` | 要从 bash 中丢弃的环境变量名。纳入叠层白名单。 |
| `shell_environment_policy.ignore_default_excludes` | `boolean` | `yes` | `user` | 跳过内置的环境变量黑名单。纳入叠层白名单。 |
| `shell_environment_policy.include_only` | `string[]` | `yes` | `user` | 设置后，bash 只继承这些环境变量名。纳入叠层白名单。 |
| `shell_environment_policy.inherit` | `string` | `yes` | `user` | bash 继承父进程的哪些环境变量名。纳入叠层白名单；不能注入值。 |
| `shell_environment_policy.set` | `map<string,string>` | `yes` | `user` | 向 bash 注入环境变量的值。不纳入叠层白名单。 |

### `skills`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `skills.disabled` | `string[]` | `yes` | `user` | 要发现但不激活的技能名。 |
| `skills.paths` | `string[]` | `yes` | `user` | 额外的技能目录。 |

### `storage`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `storage.cleanup_ttl_days` | `integer` | `yes` | `user` | 会话文件夹被删除前可闲置的天数；早于该天数的媒体与终端日志会从活跃会话中清理。默认 30。 |

### `subagents`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `subagents.enabled` | `boolean` | `pin` | `user` | 子代理 / 任务工具总开关。另见 GROK_SUBAGENTS。 |
| `subagents.limit_behavior` | `queue / fail` | `yes` | `user` | 并发子代理达到上限时的处理方式。 |
| `subagents.max_concurrent` | `integer` | `yes` | `user` | 并发子代理上限。 |
| `subagents.max_depth` | `integer` | `yes` | `user` | 子代理嵌套深度上限（下限为 1）。 |
| `subagents.models.<name>` | `string` | `yes` | `user` | 按子代理覆盖模型 id。 |
| `subagents.toggle.<name>` | `boolean` | `yes` | `user` | 单独启用或禁用某个子代理类型。未列出的代理默认启用。 |

### `telemetry`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `telemetry.otel_enabled` | `boolean` | `pin` | `user` | 外部 OTEL 总开关。另见 GROK_EXTERNAL_OTEL。 |
| `telemetry.otel_metrics_exporter` | `otlp / console / none` | `pin` | `user` | 外部 OTEL 指标导出器。另见 OTEL_METRICS_EXPORTER。 |
| `telemetry.otel_logs_exporter` | `otlp / console / none` | `pin` | `user` | 外部 OTEL 日志导出器。另见 OTEL_LOGS_EXPORTER。 |
| `telemetry.otel_endpoint` | `string` | `pin` | `user` | 外部 OTLP 基础端点。另见 OTEL_EXPORTER_OTLP_ENDPOINT。钉住会剥离开发者环境变量，以及用户/受管文件里未列出的同级键，但列出的除外。 |
| `telemetry.otel_logs_endpoint` | `string` | `pin` | `user` | 日志信号的 OTLP 端点（逐字）。另见 OTEL_EXPORTER_OTLP_LOGS_ENDPOINT。 |
| `telemetry.otel_metrics_endpoint` | `string` | `pin` | `user` | 指标信号的 OTLP 端点（逐字）。另见 OTEL_EXPORTER_OTLP_METRICS_ENDPOINT。 |
| `telemetry.otel_protocol` | `http/protobuf / grpc` | `pin` | `user` | 外部 OTLP 传输方式。另见 OTEL_EXPORTER_OTLP_PROTOCOL。钉住会剥离各信号的协议环境变量与未列出的同级文件键，但列出的除外。 |
| `telemetry.otel_logs_protocol` | `http/protobuf / grpc` | `pin` | `user` | 日志信号的 OTLP 协议。另见 OTEL_EXPORTER_OTLP_LOGS_PROTOCOL。 |
| `telemetry.otel_metrics_protocol` | `http/protobuf / grpc` | `pin` | `user` | 指标信号的 OTLP 协议。另见 OTEL_EXPORTER_OTLP_METRICS_PROTOCOL。 |
| `telemetry.otel_timeout` | `number` | `pin` | `user` | 导出超时，单位毫秒。另见 OTEL_EXPORTER_OTLP_TIMEOUT。 |
| `telemetry.otel_metric_export_interval` | `number` | `pin` | `user` | 指标导出间隔，单位毫秒。另见 OTEL_METRIC_EXPORT_INTERVAL。 |
| `telemetry.otel_certificate` | `string` | `pin` | `user` | 采集器额外 CA 证书的 PEM 路径。另见 OTEL_EXPORTER_OTLP_CERTIFICATE。CA 钉住**不会**剥离端点。 |
| `telemetry.otel_logs_certificate` | `string` | `pin` | `user` | 日志信号的 CA PEM 路径。另见 OTEL_EXPORTER_OTLP_LOGS_CERTIFICATE。 |
| `telemetry.otel_metrics_certificate` | `string` | `pin` | `user` | 指标信号的 CA PEM 路径。另见 OTEL_EXPORTER_OTLP_METRICS_CERTIFICATE。 |
| `telemetry.otel_client_certificate` | `string` | `pin` | `user` | mTLS 客户端证书的 PEM 路径。另见 OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE。钉住会剥离凭据副本、开发者端点与未列出的同级文件键。 |
| `telemetry.otel_client_key` | `string` | `pin` | `user` | mTLS 客户端密钥的 PEM 路径。token 从不存放于该文件。另见 OTEL_EXPORTER_OTLP_CLIENT_KEY。 |
| `telemetry.otel_logs_client_certificate` | `string` | `pin` | `user` | 日志信号的 mTLS 客户端证书 PEM 路径。另见 OTEL_EXPORTER_OTLP_LOGS_CLIENT_CERTIFICATE。 |
| `telemetry.otel_logs_client_key` | `string` | `pin` | `user` | 日志信号的 mTLS 客户端密钥 PEM 路径。另见 OTEL_EXPORTER_OTLP_LOGS_CLIENT_KEY。 |
| `telemetry.otel_metrics_client_certificate` | `string` | `pin` | `user` | 指标信号的 mTLS 客户端证书 PEM 路径。另见 OTEL_EXPORTER_OTLP_METRICS_CLIENT_CERTIFICATE。 |
| `telemetry.otel_metrics_client_key` | `string` | `pin` | `user` | 指标信号的 mTLS 客户端密钥 PEM 路径。另见 OTEL_EXPORTER_OTLP_METRICS_CLIENT_KEY。 |
| `telemetry.otel_metrics_include_session_id` | `boolean` | `pin` | `user` | 给指标附加 session.id。另见 OTEL_METRICS_INCLUDE_SESSION_ID。 |
| `telemetry.otel_log_user_prompts` | `boolean` | `pin` | `user` | `grok_code.user_prompt` 上提示文本的内容闸门。另见 OTEL_LOG_USER_PROMPTS。钉住任一内容闸门却不列出同级键时，被省略的同级键默认为关闭。 |
| `telemetry.otel_log_tool_details` | `boolean` | `pin` | `user` | 工具参数预览、路径与逐字名称的元数据闸门。为便于 SIEM 关联，建议开启。另见 OTEL_LOG_TOOL_DETAILS。不包含完整正文。 |
| `telemetry.otel_log_assistant_responses` | `boolean` | `pin` | `user` | `grok_code.assistant_response` 文本的内容闸门。未设置时跟随 otel_log_user_prompts，除非在 requirements 里钉住了同级闸门。只想导出提示时，仅设环境变量 OTEL_LOG_USER_PROMPTS=1 还必须把它设为 0（或钉为 false）。另见 OTEL_LOG_ASSISTANT_RESPONSES。 |
| `telemetry.otel_log_tool_content` | `boolean` | `pin` | `user` | tool_input、tool_output、full_command 与 error_message 的正文闸门。与 details 相互独立；默认关闭。只开 CONTENT 会丢掉逐字的 MCP 名称与路径。另见 OTEL_LOG_TOOL_CONTENT。 |
| `telemetry.trace_upload` | `boolean` | `pin` | `user` | 上传会话追踪。requirements 的钉住优先于用户配置。 |

### `tools`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `tools.disable_zdr_incompatible_tools` | `boolean` | `yes` | `user` | 在 ZDR 下限制需要上游托管输出的工具。另见 GROK_DISABLE_ZDR_INCOMPATIBLE_TOOLS。 |
| `tools.media_gen.max_parallel_image_gen_calls` | `integer` | `yes` | `user` | 单个模型步骤里并行 image_gen/image_edit 调用的上限。另见 GROK_MAX_PARALLEL_IMAGE_GEN_CALLS。 |
| `tools.media_gen.max_parallel_video_gen_calls` | `integer` | `yes` | `user` | 单个模型步骤里并行 video_gen 调用的上限。另见 GROK_MAX_PARALLEL_VIDEO_GEN_CALLS。 |
| `tools.respect_gitignore` | `boolean` | `pin` | `user` | 为 true 时，搜索与读取工具跳过被 gitignore 的文件。另见 GROK_RESPECT_GITIGNORE。 |
| `tools.zdr_video_output_s3` | `table` | `yes` | `user` | ZDR 视频输出所用的团队 S3 存储桶。见 ZDR Video Storage。 |

### `toolset`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `toolset.ask_user_question.timeout_secs` | `number` | `yes` | `user` | ask_user_question 工具的超时。 |
| `toolset.bash.auto_background_on_timeout` | `boolean` | `yes` | `user` | 前台超时触发时把命令转为后台。 |
| `toolset.bash.login_shell_capture` | `boolean` | `yes` | `user` | 捕获用户的登录 shell 环境供 bash 使用。纳入叠层白名单。 |
| `toolset.bash.max_timeout_secs` | `number` | `yes` | `user` | 模型请求的前台超时上限。 |
| `toolset.bash.output_byte_limit` | `number` | `yes` | `user` | 捕获的 bash 输出上限，单位字节。 |
| `toolset.bash.timeout_secs` | `number` | `yes` | `user` | 前台 bash 命令超时，单位秒。 |
| `toolset.file_toolset` | `standard / hashline` | `yes` | `user` | 文件编辑工具的实现方案。 |
| `toolset.web_fetch.allowed_domains` | `string[]` | `yes` | `user` | web_fetch 的域名白名单覆盖。 |
| `toolset.web_fetch.proxy_endpoint` | `string` | `yes` | `user` | web_fetch 的出网代理 URL。另见 GROK_WEB_FETCH_PROXY。 |
| `toolset.web_search.allowed_domains` | `string[]` | `yes` | `user` | 客户端 web_search 的域名白名单。纳入叠层白名单。 |
| `toolset.web_search.excluded_domains` | `string[]` | `yes` | `user` | 客户端 web_search 的域名黑名单。纳入叠层白名单。 |

### `ui`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `ui.approval_mode` | `string` | `yes` | `user` | 已弃用；请改用 `ui.permission_mode`。 |
| `ui.auto_dark_theme` | `string` | `yes` | `user` | `theme = auto` 且系统为深色时使用的主题。 |
| `ui.auto_light_theme` | `string` | `yes` | `user` | `theme = auto` 且系统为浅色时使用的主题。 |
| `ui.cancel_subagents_on_turn_cancel` | `ask / always_stop / always_continue` | `yes` | `user` | 取消父回合时如何处理仍在运行的子代理。 |
| `ui.collapsed_edit_blocks` | `boolean` | `yes` | `user` | 把编辑显示为单行 +N/-M 摘要。另见 GROK_COLLAPSED_EDIT_BLOCKS。 |
| `ui.combine_queued_prompts` | `boolean` | `yes` | `user` | 把连续的普通追加提示合并为一个回合。 |
| `ui.compact_mode` | `boolean` | `yes` | `user` | 更紧凑的消息间距。另见 `/compact-mode`。 |
| `ui.confirm_before_rewind` | `boolean` | `yes` | `user` | 回退对话历史前先询问。 |
| `ui.contextual_hints.image_input` | `boolean` | `yes` | `user` | 模型接受图片时提示可粘贴剪贴板图片。 |
| `ui.contextual_hints.plan_mode` | `boolean` | `yes` | `user` | 对计划类提示建议使用计划模式（Shift+Tab）。 |
| `ui.contextual_hints.send_now` | `boolean` | `yes` | `user` | 中局追加提示入队后，在空提示框按 Enter 立即发送。 |
| `ui.contextual_hints.small_screen` | `boolean` | `yes` | `user` | 终端过矮时建议使用 `/compact-mode`。 |
| `ui.contextual_hints.ssh_wrap` | `boolean` | `yes` | `user` | SSH 下没有剪贴板出口时推荐 `chaos wrap`。 |
| `ui.contextual_hints.undo` | `boolean` | `yes` | `user` | 提示 Ctrl+Z 可恢复被清掉的提示草稿。 |
| `ui.contextual_hints.word_select` | `boolean` | `yes` | `user` | 在折叠/导航选择下双击后，指向设置里的 Word select。 |
| `ui.cursor_blink` | `boolean` | `yes` | `user` | 强制方块光标闪烁（true）或常亮（false）。未设置则沿用终端设置。 |
| `ui.default_selected_permission` | `string` | `yes` | `user` | 会话第一个提示上预选的审批行。另见 GROK_DEFAULT_SELECTED_PERMISSION。 |
| `ui.display_refresh.auto_cadence_enabled` | `boolean` | `yes` | `user` | 让流式/滚动节奏匹配显示器刷新率。另见 GROK_DISPLAY_REFRESH_AUTO_CADENCE。 |
| `ui.follow_up_behavior` | `queue / steer` | `yes` | `user` | 中局追加提示的路由方式。 |
| `ui.fork_secondary_model` | `string` | `yes` | `user` | 分叉时次级代理使用的模型。默认为主默认模型。 |
| `ui.group_tool_verbs` | `boolean` | `yes` | `user` | 折叠连续的读取/搜索/列目录工具行。另见 GROK_GROUP_TOOL_VERBS。 |
| `ui.hunk_tracker_mode` | `agent_only / all_dirty / off` | `yes` | `user` | 文件改动的 hunk 跟踪。另见 GROK_HUNK_TRACKER 与 `--hunk-tracker-mode`。 |
| `ui.invert_scroll` | `boolean` | `yes` | `user` | 反转纵向滚动方向。另见 GROK_INVERT_SCROLL。 |
| `ui.keep_text_selection` | `flash / hold / word_select` | `yes` | `user` | 应用内选择：短暂闪示、保持，或双击选词。 |
| `ui.max_thoughts_width` | `number` | `yes` | `user` | 思考面板的列宽（40–500）。 |
| `ui.mouse_reporting_toggle` | `boolean` | `yes` | `user` | 回滚区里 Ctrl+R 切换终端鼠标捕获。另见 GROK_MOUSE_REPORTING_TOGGLE。 |
| `ui.page_flip_on_send` | `boolean` | `yes` | `user` | 把已发送的提示吸附到视口顶部。 |
| `ui.permission_mode` | `default / ask / auto / always-approve` | `yes` | `user` | 工具权限的默认行为。企业锁定使用 requirements.toml。 |
| `ui.prompt_suggestions` | `boolean` | `yes` | `user` | 每回合结束后给出下一条提示的幽灵文本。另见 GROK_PROMPT_SUGGESTIONS；远端总开关可在整个机群上禁用它。 |
| `prompt_suggestions.max_output_tokens` | `number` | `yes` | `user` | 建议调用的可见输出 token 数；限制在 16–256，默认 64，另为推理单独留量。可被远端覆盖。 |
| `prompt_suggestions.temperature` | `number` | `yes` | `user` | 建议调用的采样温度（默认 0.2）。可被远端覆盖。 |
| `prompt_suggestions.reasoning_effort` | `none / minimal / low / medium / high` | `yes` | `user` | 建议调用的推理投入；默认值与 `none` 关闭推理，其余取值会采用模型支持的某个档位。可被远端覆盖。 |
| `ui.remember_tool_approvals` | `boolean` | `yes` | `user` | 显示按工具区分的「始终允许」选项。另见 GROK_REMEMBER_TOOL_APPROVALS。 |
| `ui.render_mermaid` | `auto / on / off` | `yes` | `user` | mermaid 围栏的渲染方式：可点击的打开行，或原始源码。 |
| `ui.screen_mode` | `fullscreen / minimal` | `yes` | `user` | 直接启动 `chaos` 时的默认渲染模式。需要重启。 |
| `ui.scroll_lines` | `integer` | `yes` | `user` | 每档滚动的行数（1–10）。另见 GROK_SCROLL_LINES。 |
| `ui.scroll_mode` | `auto / wheel / trackpad` | `yes` | `user` | 滚动输入的判定方式。另见 GROK_SCROLL_MODE。 |
| `ui.scroll_speed` | `integer` | `yes` | `user` | 鼠标/触控板滚动速度倍数（1–100）。另见 GROK_SCROLL_SPEED。 |
| `ui.show_thinking_blocks` | `boolean` | `yes` | `user` | 流式输出时显示思考/推理块。另见 GROK_SHOW_THINKING_BLOCKS。 |
| `ui.show_timeline` | `boolean` | `yes` | `user` | 用按回合的刻度轨代替滚动条。 |
| `ui.show_timestamps` | `boolean` | `yes` | `user` | 在消息旁显示时钟时间。另见 `/timestamps`。 |
| `ui.simple_mode` | `boolean` | `yes` | `user` | 为 true 时用 Readline 方式编辑提示；为 false 时启用实验性的 vim 提示键位。 |
| `ui.status_line.command` | `string` | `yes` | `user` | `command` 状态行所执行的脚本。campaign 会剥离该路径；requirements 层仍会合并它。 |
| `ui.status_line.type` | `disabled / command` | `yes` | `user` | 快捷键栏上方的可选状态行。默认关闭。参见[《状态行》](25-status-line.md)。 |
| `ui.theme` | `string` | `yes` | `user` | 配色主题名，或用 `auto`/`system` 跟随系统。另见 `/theme` 与 GROK_THEME。 |
| `ui.ui_theme` | `string` | `yes` | `user` | `ui.theme` 的旧别名。 |
| `ui.vim_mode` | `boolean` | `yes` | `user` | 在回滚区而非提示中使用 vim 键位。另见 `/vim-mode`。 |
| `ui.voice_capture_mode` | `hold / toggle` | `yes` | `user` | 按住说话或按一下切换的语音采集方式。 |
| `ui.voice_keybind_enabled` | `boolean` | `yes` | `user` | 启用 Ctrl+Space / F8 语音听写。为 false 时 `/voice` 仍可用。 |
| `ui.voice_stt_language` | `string` | `yes` | `user` | 语音转文字语言代码或 `auto`。本次会话内覆盖 `[voice].language`。 |
| `ui.yolo` | `boolean` | `pin` | `user` | 始终批准工具调用。requirements 可将其钉为 false 以封禁 `--yolo`。 |

### `version_overrides`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `version_overrides` | `array of tables` | `yes` | `user` | 按 CLI 版本应用的配置补丁，在合并前套用。参见 `[[version_overrides]]`。 |

### `voice`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `voice.api_base` | `string` | `yes` | `user` | 语音转文字的 HTTPS API 根地址。未设置时继承 `[endpoints].xai_api_base_url`。 |
| `voice.language` | `string` | `yes` | `user` | 首选的 STT 语言目录代码或 `auto`。 |
| `voice.sample_rate` | `number` | `yes` | `user` | STT 采集采样率，单位 Hz。 |

### `workflows`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `workflows.enabled` | `boolean` | `yes` | `user` | 启用工作流。 |

### `worktree`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `worktree.auto_gc` | `table` | `yes` | `user` | worktree 自动垃圾回收策略。 |

## managed_config.toml

`managed_config.toml` 接受上面各表中的所有键。它设置团队默认值，因此开发者自己的 `config.toml` 会覆盖它。想让别人能调整的值放这里，不能调整的值放 `requirements.toml`。

这条规则有一个例外：

| 键 | 行为 |
| --- | --- |
| `features.remote_fetch` | 受管值优先于开发者自己的值。 |

Chaos 先读取 `/etc/grok/managed_config.toml`，再读取 `$CHAOS_HOME/managed_config.toml`，后者由控制台保持同步。后者的值会替换前者的值。

上面各表的 **Managed** 列给出每个键的答案：`fleet` 表示团队值生效，`user` 表示用户文件胜出，`—` 表示忽略本文件。

## requirements.toml

`requirements.toml` 是管理员强制下发的文件。位置依次为 `$CHAOS_HOME/requirements.toml`（带签名的缓存）、`/etc/grok/requirements.toml`，以及 macOS MDM `ai.x.grok`。`config.toml` 各表里的 **Requirements** 列列出该文件接受的每个 `config.toml` 键（`pin` 或 `yes`）。未列出的键不受约束。

以下键只存在于 `requirements.toml`：

| 键 | 类型 / 取值 | 默认 | 说明 |
| --- | --- | --- | --- |
| `fail_closed` | `boolean` | `false` | 当带签名的 requirements 或 version_overrides 无法应用时拒绝启动；默认 false。 |
| `features.image_edit` | `boolean` | — | 钉住 image_edit 的可用性。仅限 requirements；写进用户文件会被视为无法识别，未设置则沿用远程配置的默认值。 |
| `ui.disable_bypass_permissions_mode` | `boolean` | — | 锁定「始终批准」为关闭。该锁定仅在 requirements 层生效；写在用户文件或 managed 文件里的 true 会被忽略。 |

## 设置被拒绝时会发生什么

| 场景 | Chaos 的行为 |
| --- | --- |
| 开发者设置了某个你钉住的键 | 以钉住的值生效。`chaos inspect` 会列出贡献该值的 requirements 文件。 |
| 开发者设置了你在 `managed_config.toml` 里下发的某个键 | 以其值为准，但 `features.remote_fetch` 例外。若该键必须保持不变，请改为钉住它。 |
| `requirements.toml` 缺失或签名校验不通过 | 钉住项不生效，Chaos 照常启动。可设 `fail_closed = true` 改为拒绝启动。 |
| 钉住的键指向的值本版本不认识 | 该键被忽略，文件其余部分照常生效。 |

## 查看当前生效的配置

在开发者的机器上运行 `chaos inspect`。它会列出每个参与合并的配置文件，包括 requirements 与 managed 层，因此哪条策略没有生效，一条命令就能看清。
