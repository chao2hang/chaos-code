# Configuration reference

本文件随 CLI 一起分发，启动时解包到 `~/.chaos/docs/user-guide/26-config-reference.md`。它是 `config.toml`、`managed_config.toml` 与 `requirements.toml` 的完整字段清单。概念性说明见 [05-configuration.md](05-configuration.md)。

## How to configure

三个文件用来配置 Chaos，它们由不同的人维护。

| 文件 | Who writes it | Where it lives | Use it to |
| --- | --- | --- | --- |
| `config.toml` | 开发者 | `~/.chaos/config.toml`, and `.chaos/config.toml` in a project | Set personal defaults. Anything here can be changed by the person using the machine. |
| `managed_config.toml` | You, through the console or a deployment tool | `/etc/grok/managed_config.toml` | Ship a starting point to a fleet. A developer's own file overrides it. |
| `requirements.toml` | 你（含签名） | `/etc/grok/requirements.toml`, or macOS device management | Set values a developer cannot change. Keys marked `pin` below hold against every other file, the environment, and the command line. |

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
| `agent.definition` | `string (path)` | `yes` | `user` | Path to an agent definition markdown file with YAML frontmatter. |
| `agent.name` | `string` | `yes` | `user` | Built-in or discovered agent definition name. Also GROK_AGENT and `--agent-profile`. |
| `agent.system_prompt_label` | `string` | `yes` | `user` | Global system-prompt identity; per-model override wins. |

### `announcements`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `announcements` | `array of tables` | `—` | `user` | Remote announcement payloads consumed at load. Not a user-authored table. |

### `auth`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `auth` | `table` | `yes` | `user` | Alias of `[grok_com_config]`; every `grok_com_config.*` key also works as `auth.*`. |
| `auth.auth_provider_command` | `string` | `yes` | `user` | External auth binary; stdout is the token. Also GROK_AUTH_PROVIDER_COMMAND; also valid as `grok_com_config.auth_provider_command`. |
| `auth.auth_provider_label` | `string` | `yes` | `user` | Login button label for an external auth provider. Also GROK_AUTH_PROVIDER_LABEL; also valid as `grok_com_config.auth_provider_label`. |
| `auth.auth_token_ttl` | `number` | `yes` | `user` | Token TTL in seconds for providers that return a bare token. Also GROK_AUTH_TOKEN_TTL; also valid as `grok_com_config.auth_token_ttl`. |
| `auth.disable_api_key_auth` | `boolean` | `pin` | `user` | Refuse API-key auth so only the deployment IdP can log in. Also GROK_DISABLE_API_KEY_AUTH; also valid as `grok_com_config.disable_api_key_auth`. |
| `auth.force_login_team_uuid` | `string / string[]` | `pin` | `user` | Require login to this team UUID, or any of an array; empty array fails closed. Also GROK_FORCE_LOGIN_TEAM_ID; also valid as `grok_com_config.force_login_team_uuid`. |
| `auth.grok_ws_origin` | `string` | `yes` | `user` | Websocket origin for grok.com. Also GROK_WS_ORIGIN; also valid as `grok_com_config.grok_ws_origin`. |
| `auth.grok_ws_url` | `string` | `yes` | `user` | Relay websocket URL. Also GROK_WS_URL; also valid as `grok_com_config.grok_ws_url`. |
| `auth.oauth2` | `table` | `yes` | `user` | OAuth2 provider used when enterprise OIDC is unset; also valid as `grok_com_config.oauth2`. |
| `auth.oauth2.client_id` | `string` | `yes` | `user` | OAuth2 client id. Also GROK_OAUTH2_CLIENT_ID; also valid as `grok_com_config.oauth2.client_id`. |
| `auth.oauth2.issuer` | `string` | `yes` | `user` | OAuth2 issuer URL. Also GROK_OAUTH2_ISSUER; also valid as `grok_com_config.oauth2.issuer`. |
| `auth.oauth2.principal_id` | `string` | `yes` | `user` | Required principal id when `principal_type` is set. Also GROK_OAUTH2_PRINCIPAL_ID; also valid as `grok_com_config.oauth2.principal_id`. |
| `auth.oauth2.principal_type` | `string` | `yes` | `user` | Token principal type, such as Team. Also GROK_OAUTH2_PRINCIPAL_TYPE; also valid as `grok_com_config.oauth2.principal_type`. |
| `auth.oauth2.referrer` | `string` | `yes` | `user` | Referrer for OAuth usage attribution. Also GROK_OAUTH2_REFERRER; also valid as `grok_com_config.oauth2.referrer`. |
| `auth.oauth2.scopes` | `string[]` | `yes` | `user` | OAuth2 scopes. Also GROK_OAUTH2_SCOPES; also valid as `grok_com_config.oauth2.scopes`. |
| `auth.oidc` | `table` | `yes` | `user` | Customer OIDC identity-provider settings; also valid as `grok_com_config.oidc`. |
| `auth.oidc.audience` | `string` | `yes` | `user` | Optional OIDC audience. Also GROK_OIDC_AUDIENCE; also valid as `grok_com_config.oidc.audience`. |
| `auth.oidc.client_id` | `string` | `yes` | `user` | OIDC client id. Also GROK_OIDC_CLIENT_ID; also valid as `grok_com_config.oidc.client_id`. |
| `auth.oidc.issuer` | `string` | `yes` | `user` | OIDC issuer URL. Also GROK_OIDC_ISSUER; also valid as `grok_com_config.oidc.issuer`. |
| `auth.oidc.scopes` | `string[]` | `yes` | `user` | OIDC scopes. Also GROK_OIDC_SCOPES; also valid as `grok_com_config.oidc.scopes`. |
| `auth.preferred_method` | `api_key / oidc` | `yes` | `user` | Pin automatic auth to one method with no fallthrough; also valid as `grok_com_config.preferred_method`. |
| `auth.token_header` | `string` | `yes` | `user` | Header name that carries the CLI auth token; default `xai-grok-cli`; also valid as `grok_com_config.token_header`. |

### `auth_provider`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `auth_provider.<name>` | `table` | `yes` | `user` | Named credential helper used by `[model.<id>] auth_provider`. |

### `auto_mode`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `auto_mode.enabled` | `boolean` | `yes` | `user` | Enable Auto permission mode. |

### `campaigns`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `campaigns` | `array of tables` | `yes` | `user` | Named campaign patches applied below requirements. The deployment publishes these. |

### `cli`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `cli.auto_update` | `boolean` | `pin` | `user` | Check for CLI updates on launch. Also GROK_DISABLE_AUTOUPDATER to suppress. |
| `cli.channel` | `stable / alpha` | `pin` | `user` | Release channel preference. |
| `cli.grove_worktree` | `boolean` or `grove` / `grove-fuse` / `grove-nfs` / `nfs` / `copy` / `true` / `false` / `1` / `0` / `on` / `off` | `yes` | `user` | Session / `-w` Grove vs copy. Default copy. Distinct from creation-mode `cli.worktree_type`. Also `GROK_WORKTREE_TYPE`. Layer order: request → env → local → remote-true; then kill last: remote `grove_worktree = false` → copy. Missing remote settings are not a kill: local/env/request still apply. 上游的 `grok clone` 依赖 Grove，本分叉不含该功能（兼容说明）。 |
| `cli.installer` | `string` | `—` | `user` | Which installer last set up this CLI, used to pick the update path. |
| `cli.maximum_version` | `string` | `pin` | `user` | Highest CLI version that still runs without a hard block. Also GROK_MAXIMUM_VERSION. |
| `cli.minimum_version` | `string` | `pin` | `user` | Lowest CLI version that still runs without a hard block. Also GROK_MINIMUM_VERSION. |
| `cli.nfs_worktree` | same as `cli.grove_worktree` | `yes` | `user` | Read alias of `cli.grove_worktree`. |
| `cli.npm_registry` | `string` | `yes` | `user` | npm registry used by the auto-updater. |
| `cli.required_maximum_version` | `string` | `pin` | `user` | Hard maximum CLI version. Also GROK_REQUIRED_MAXIMUM_VERSION. |
| `cli.required_minimum_version` | `string` | `pin` | `user` | Hard minimum CLI version. Also GROK_REQUIRED_MINIMUM_VERSION. |
| `cli.session_picker_grouped` | `boolean` | `yes` | `user` | Group sessions by repo in the picker and CLI listings. |
| `cli.session_registry` | `boolean` | `yes` | `user` | Participate in the cross-process session registry. |
| `cli.show_tips` | `boolean` | `pin` | `user` | 启动提示。 |
| `cli.use_leader` | `boolean` | `pin` | `user` | Use the leader process for config reload and MCP watches. |
| `cli.worktree_type` | `string` | `yes` | `user` | Creation-mode when set to `linked`, `standalone`, or `git`. The spellings `grove`, `grove-fuse`, `grove-nfs`, `nfs`, and `copy` also feed the session / `-w` Grove gate (same as `cli.grove_worktree`); they are not creation-mode values. |

### `compat`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `compat.claude.agents` | `boolean` | `yes` | `user` | Scan CLAUDE.md. Also GROK_CLAUDE_AGENTS_ENABLED. |
| `compat.claude.hooks` | `boolean` | `yes` | `user` | Scan Claude hooks. Also GROK_CLAUDE_HOOKS_ENABLED. |
| `compat.claude.mcps` | `boolean` | `yes` | `user` | Scan Claude MCP config. Also GROK_CLAUDE_MCPS_ENABLED. |
| `compat.claude.rules` | `boolean` | `yes` | `user` | Scan Claude rules. Also GROK_CLAUDE_RULES_ENABLED. |
| `compat.claude.skills` | `boolean` | `yes` | `user` | Scan Claude skills. Also GROK_CLAUDE_SKILLS_ENABLED. |
| `compat.codex.hooks` | `boolean` | `yes` | `user` | Scan Codex hooks when present. |
| `compat.codex.skills` | `boolean` | `yes` | `user` | Scan Codex skills directories when present. |
| `compat.cursor.agents` | `boolean` | `yes` | `user` | Scan agent definitions from Cursor compat sources. Also GROK_CURSOR_AGENTS_ENABLED. |
| `compat.cursor.hooks` | `boolean` | `yes` | `user` | Scan Cursor hooks. Also GROK_CURSOR_HOOKS_ENABLED. |
| `compat.cursor.mcps` | `boolean` | `yes` | `user` | Scan Cursor mcp.json. Also GROK_CURSOR_MCPS_ENABLED. |
| `compat.cursor.rules` | `boolean` | `yes` | `user` | Scan `.cursor/rules/`. Also GROK_CURSOR_RULES_ENABLED. |
| `compat.cursor.skills` | `boolean` | `yes` | `user` | Scan Cursor skills directories. Also GROK_CURSOR_SKILLS_ENABLED. |

### `dashboard`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `dashboard.enabled` | `boolean` | `yes` | `user` | Show the agent dashboard. |
| `dashboard.grouping` | `state / directory` | `yes` | `user` | How dashboard rows group. |

### `default_auto_mode`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `default_auto_mode` | `boolean` | `yes` | `user` | Start sessions in auto permission mode when no per-session override is set. |

### `diagnostics`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `diagnostics.crash_handler` | `boolean` | `yes` | `user` | Write a panic report under `$CHAOS_HOME/crash/`. Also GROK_CRASH_HANDLER. |

### `disable_web_search`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `disable_web_search` | `boolean` | `yes` | `user` | Drop the web_search tool for this process. Also `--disable-web-search`. |

### `disabled_mcp_servers`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `disabled_mcp_servers` | `string[]` | `yes` | `user` | MCP server names to skip without deleting their `[mcp_servers]` blocks. |

### `disabled_mcp_tools`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `disabled_mcp_tools` | `map<string, string[]>` | `yes` | `user` | Per-server MCP tool deny lists keyed by server name. |

### `doom_loop_recovery`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `doom_loop_recovery.enabled` | `boolean` | `yes` | `user` | Resample confident tool-call loops; set false to disable. |

### `endpoints`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `endpoints.cli_chat_proxy_base_url` | `string` | `pin` | `user` | Session-service API base URL. |
| `endpoints.deployment_key` | `string` | `pin` | `user` | Management key for enterprise deployments. Also GROK_DEPLOYMENT_KEY. |
| `endpoints.feedback_base_url` | `string` | `yes` | `user` | Where feedback submissions go. Also GROK_FEEDBACK_BASE_URL. |
| `endpoints.managed_config_url` | `string` | `yes` | `user` | Override managed config endpoint. Also GROK_MANAGED_CONFIG_URL. |
| `endpoints.models_base_url` | `string` | `pin` | `user` | Custom inference base URL. Also GROK_MODELS_BASE_URL. |
| `endpoints.models_list_url` | `string` | `pin` | `user` | Override model-list URL. Also GROK_MODELS_LIST_URL. Alias `models_endpoint`. |
| `endpoints.trace_upload_bucket` | `string` | `yes` | `user` | Direct gs:// or s3:// bucket for traces; bypasses the proxy. Also GROK_TRACE_UPLOAD_BUCKET. |
| `endpoints.trace_upload_credentials` | `string` | `yes` | `user` | Inline GCS service-account JSON or AWS credentials for that bucket; wins over `trace_upload_credentials_file` and has no environment variable. |
| `endpoints.trace_upload_credentials_file` | `string (path)` | `yes` | `user` | Path to a GCS service-account JSON or AWS credentials file for that bucket. Also GROK_TRACE_UPLOAD_CREDENTIALS_FILE. |
| `endpoints.trace_upload_endpoint_url` | `string` | `yes` | `user` | Custom S3-compatible endpoint for s3:// bucket uploads. Also GROK_TRACE_UPLOAD_ENDPOINT_URL. |
| `endpoints.trace_upload_region` | `string` | `yes` | `user` | AWS region for s3:// bucket uploads; default us-east-1. Also GROK_TRACE_UPLOAD_REGION. |
| `endpoints.trace_upload_url` | `string` | `pin` | `user` | Proxy destination for traces when no direct bucket is set. Also GROK_TRACE_UPLOAD_URL. |
| `endpoints.xai_api_base_url` | `string` | `pin` | `user` | Public xAI API base. Also GROK_XAI_API_BASE_URL. |

### `features`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `features.active_agent_messages` | `boolean` | `pin` | `user` | Enable or disable `active_agent_messages`. Default false. Also `GROK_ACTIVE_AGENT_MESSAGES`. |
| `features.ask_user_question` | `boolean` | `pin` | `user` | Enable or disable `ask_user_question`. Default true. Also `GROK_ASK_USER_QUESTION`. |
| `features.auto_wake` | `boolean` | `pin` | `user` | Enable or disable `auto_wake`. Default true. Also `GROK_AUTO_WAKE`. |
| `features.backend_tools` | `boolean` | `pin` | `user` | Enable or disable `backend_tools`. Default true. Also `GROK_BACKEND_SEARCH`. |
| `features.campaigns` | `boolean` | `yes` | `user` | Enable remote campaign patches. `GROK_CAMPAIGNS=0` still disables even when requirements set this true. |
| `features.cancel_rewind` | `boolean` | `pin` | `user` | Enable or disable `cancel_rewind`. Default true. Also `GROK_CANCEL_REWIND`. |
| `features.codebase_indexing` | `boolean / string[]` | `pin` | `user` | Codebase graph indexing; true indexes git repos, or pass include/exclude globs. |
| `features.compaction_detail` | `none / minimal / balanced / verbose` | `yes` | `user` | Verbatim detail level for `segments` compaction. Also GROK_COMPACTION_DETAIL. |
| `features.compaction_mode` | `summary / transcript / segments` | `yes` | `user` | Compaction strategy. Also GROK_COMPACTION_MODE. |
| `features.compaction_tool_choice` | `string` | `yes` | `user` | Tool-choice hint used during compaction. |
| `features.compaction_verbatim_input` | `boolean` | `pin` | `user` | Enable or disable `compaction_verbatim_input`. Default true. Also `GROK_COMPACTION_VERBATIM_INPUT`. |
| `features.dock` | `boolean` | `pin` | `user` | Enable or disable `dock`. Default false. Also `GROK_DOCK`. |
| `features.feedback` | `boolean` | `pin` | `user` | Enable or disable `feedback`. Default true. Also `GROK_FEEDBACK_ENABLED`. |
| `features.feedback_trace_card` | `boolean` | `pin` | `user` | Show a trace-upload consent question after `/feedback`. Default false. Also `GROK_FEEDBACK_TRACE_CARD`. |
| `features.image_edit_model_override` | `string` | `yes` | `user` | Imagine model id for image_edit. |
| `features.image_gen` | `boolean` | `pin` | `user` | Enable image_gen / `/imagine`. |
| `features.image_gen_model_override` | `string` | `yes` | `user` | Imagine model id for image_gen. Empty defers to the remotely configured default. |
| `features.lsp_tools` | `boolean` | `pin` | `user` | Enable or disable `lsp_tools`. Default false. Also `GROK_LSP_TOOLS`. |
| `features.managed_config` | `boolean` | `yes` | `user` | Fetch managed_config.toml and requirements.toml from the deployment. |
| `features.mcp_auto_restart` | `boolean` | `yes` | `user` | Auto-restart stdio MCP servers after transport failure. Also GROK_MCP_AUTO_RESTART. |
| `features.mcp_liveness_watchers` | `boolean` | `yes` | `user` | Poll MCP transports and push server_status updates. Emergency kill switch when false. |
| `features.mcp_push_server_status` | `boolean` | `yes` | `user` | Pager subscribes to MCP server_status push. Process env GROK_MCP_PUSH_SERVER_STATUS wins at launch. |
| `features.mcp_recursive_config_watch` | `boolean` | `yes` | `user` | Watch `<cwd>/` and `<cwd>/.chaos/` for project MCP config edits. Name is a misnomer; watches are non-recursive. |
| `features.non_git_warning` | `boolean` | `yes` | `user` | Show a blocking warning when Grok starts outside a Git repository. |
| `features.remember_mode` | `boolean` | `—` | `—` | Remember the last permission mode across sessions. Read from user `config.toml` only. |
| `features.remote_fetch` | `boolean` | `pin` | `fleet` | Pin remote model-catalog and asset fetch. Managed wins over the user file when both set. |
| `features.repo_status_in_system_prompt` | `boolean` | `pin` | `user` | Enable or disable `repo_status_in_system_prompt`. Default true. Also `GROK_REPO_STATUS_IN_SYSTEM_PROMPT`. |
| `features.session_recap` | `boolean` | `pin` | `user` | Enable or disable `session_recap`. Default true. Also `GROK_SESSION_RECAP`. |
| `features.session_search` | `boolean` | `pin` | `user` | Enable or disable `session_search`. Default true. Also `GROK_SESSION_SEARCH`. |
| `features.subagent_worktree_snapshot` | `boolean` | `pin` | `user` | Enable or disable `subagent_worktree_snapshot`. Default false. Also `GROK_SUBAGENT_WORKTREE_SNAPSHOT`. |
| `features.support_permission` | `boolean` | `yes` | `user` | Allow the agent to ask permission for tool executions. |
| `features.telemetry` | `boolean / session_metrics / off` | `pin` | `user` | Product telemetry mode. Enterprise default is off. |
| `features.title_refresh` | `boolean` | `pin` | `user` | Early-session auto-title refresh. Pin this in requirements to beat GROK_TITLE_REFRESH. |
| `features.turn_summary` | `boolean` | `pin` | `user` | Enable or disable `turn_summary`. Default true. Also `GROK_TURN_SUMMARY`. |
| `features.two_pass_compaction` | `boolean` | `pin` | `user` | Enable or disable `two_pass_compaction`. Default true. Also `GROK_TWO_PASS_COMPACTION`. |
| `features.video_gen` | `boolean` | `pin` | `user` | Enable video tools / `/imagine-video`. |
| `features.voice_mode` | `boolean` | `pin` | `user` | Enable or disable `voice_mode`. Default true. Also `GROK_VOICE_MODE`. |
| `features.web_fetch` | `boolean` | `pin` | `user` | Enable or disable `web_fetch`. Default false. Also `GROK_WEB_FETCH`. |
| `features.write_file` | `boolean` | `pin` | `user` | Enable or disable `write_file`. Default true. Also `GROK_WRITE_FILE`. |
| `features.zdr_access_enabled` | `boolean` | `pin` | `user` | Advertise ZDR-incompatible tools when the team is on Zero Data Retention. Also `GROK_ZDR_ACCESS_ENABLED`. |

### `feedback`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `feedback.user.command` | `string` | `yes` | `user` | Shell command that prints name and email JSON for feedback submissions. |
| `feedback.user.email` | `string[]` | `yes` | `user` | Sources for the feedback author email (`git_email` or a literal). |
| `feedback.user.name` | `string[]` | `yes` | `user` | Sources for the feedback author name (`os_user` or a literal). |

### `goal`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `goal.enabled` | `boolean` | `yes` | `user` | 启用 `/goal`。 |

### `grok_com_config`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `grok_com_config` | `table` | `yes` | `user` | Grok.com websocket and OAuth/OIDC settings. `[auth]` is an alias. |
| `grok_com_config.auth_provider_command` | `string` | `yes` | `user` | External auth binary; stdout is the token. Also GROK_AUTH_PROVIDER_COMMAND. |
| `grok_com_config.auth_provider_label` | `string` | `yes` | `user` | Login button label for an external auth provider. Also GROK_AUTH_PROVIDER_LABEL. |
| `grok_com_config.auth_token_ttl` | `number` | `yes` | `user` | Token TTL in seconds for providers that return a bare token. Also GROK_AUTH_TOKEN_TTL. |
| `grok_com_config.disable_api_key_auth` | `boolean` | `pin` | `user` | Refuse API-key auth so only the deployment IdP can log in. Also GROK_DISABLE_API_KEY_AUTH. |
| `grok_com_config.force_login_team_uuid` | `string / string[]` | `pin` | `user` | Require login to this team UUID, or any of an array; empty array fails closed. Also GROK_FORCE_LOGIN_TEAM_ID. |
| `grok_com_config.grok_ws_origin` | `string` | `yes` | `user` | Websocket origin for grok.com. Also GROK_WS_ORIGIN. |
| `grok_com_config.grok_ws_url` | `string` | `yes` | `user` | Relay websocket URL. Also GROK_WS_URL. |
| `grok_com_config.oauth2` | `table` | `yes` | `user` | OAuth2 provider used when enterprise OIDC is unset. |
| `grok_com_config.oauth2.client_id` | `string` | `yes` | `user` | OAuth2 client id. Also GROK_OAUTH2_CLIENT_ID. |
| `grok_com_config.oauth2.issuer` | `string` | `yes` | `user` | OAuth2 issuer URL. Also GROK_OAUTH2_ISSUER. |
| `grok_com_config.oauth2.principal_id` | `string` | `yes` | `user` | Required principal id when `principal_type` is set. Also GROK_OAUTH2_PRINCIPAL_ID. |
| `grok_com_config.oauth2.principal_type` | `string` | `yes` | `user` | Token principal type, such as Team. Also GROK_OAUTH2_PRINCIPAL_TYPE. |
| `grok_com_config.oauth2.referrer` | `string` | `yes` | `user` | Referrer for OAuth usage attribution. Also GROK_OAUTH2_REFERRER. |
| `grok_com_config.oauth2.scopes` | `string[]` | `yes` | `user` | OAuth2 scopes. Also GROK_OAUTH2_SCOPES. |
| `grok_com_config.oidc` | `table` | `yes` | `user` | Customer OIDC identity-provider settings. |
| `grok_com_config.oidc.audience` | `string` | `yes` | `user` | Optional OIDC audience. Also GROK_OIDC_AUDIENCE. |
| `grok_com_config.oidc.client_id` | `string` | `yes` | `user` | OIDC client id. Also GROK_OIDC_CLIENT_ID. |
| `grok_com_config.oidc.issuer` | `string` | `yes` | `user` | OIDC issuer URL. Also GROK_OIDC_ISSUER. |
| `grok_com_config.oidc.scopes` | `string[]` | `yes` | `user` | OIDC scopes. Also GROK_OIDC_SCOPES. |
| `grok_com_config.preferred_method` | `api_key / oidc` | `yes` | `user` | Pin automatic auth to one method with no fallthrough. |
| `grok_com_config.token_header` | `string` | `yes` | `user` | Header name that carries the CLI auth token; default `xai-grok-cli`. |

### `harness`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `harness.block_for_upload` | `boolean` | `yes` | `user` | Block turn end until the workspace snapshot upload finishes. |
| `harness.disable_workspace_teleport` | `boolean` | `pin` | `user` | Kill switch for per-turn workspace snapshots. |

### `hints`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `hints.fork_worktree_mode` | `ask / always / never` | `yes` | `user` | Whether `/fork` offers a worktree. |
| `hints.new_session_worktree_mode` | `ask / always / never` | `yes` | `user` | Whether `/new` offers a worktree. |

### `hooks`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `hooks.<event>` | `array of tables` | `yes` | `user` | Matcher groups for a lifecycle event such as PreToolUse or Stop. See Hooks. |
| `hooks.<event>[].hooks[].command` | `string` | `yes` | `user` | Command to run for this hook. `$VAR` is not expanded at load. |
| `hooks.<event>[].hooks[].type` | `command` | `yes` | `user` | Hook handler type. Command hooks are supported. |
| `hooks.<event>[].matcher` | `string` | `yes` | `user` | Tool-name matcher for this hook group. |

### `managed_mcps`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `managed_mcps.enabled` | `boolean` | `pin` | `user` | Fetch managed MCP configs at startup. Also GROK_MANAGED_MCPS_ENABLED. |
| `managed_mcps.gateway_tools_enabled` | `boolean` | `yes` | `user` | Expose managed MCP gateway tools. Also GROK_MANAGED_MCP_GATEWAY_TOOLS_ENABLED. |

### `marketplace`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `marketplace.sources` | `array of tables` | `yes` | `user` | `[[marketplace.sources]]` plugin marketplace repos. |
| `marketplace.require_sha` | `boolean` | `yes` | `user` | Tighten-only: remote plugin installs and updates must pin a full commit sha. Also `GROK_MARKETPLACE_REQUIRE_SHA`. Neither this key nor the env var can turn the gate back off. |

### `mcp`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `mcp.max_output_bytes` | `number` | `yes` | `user` | Cap MCP tool output size in bytes. Project files may set this. |

### `mcp_servers`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `mcp_servers.<name>.args` | `string[]` | `yes` | `user` | `[mcp_servers.<name>]` `args` on a stdio or HTTP MCP server. |
| `mcp_servers.<name>.bearer_token_env_var` | `string` | `yes` | `user` | `[mcp_servers.<name>]` `bearer_token_env_var` on a stdio or HTTP MCP server. |
| `mcp_servers.<name>.command` | `string` | `yes` | `user` | `[mcp_servers.<name>]` `command` on a stdio or HTTP MCP server. |
| `mcp_servers.<name>.cwd` | `string` | `yes` | `user` | `[mcp_servers.<name>]` `cwd` on a stdio or HTTP MCP server. |
| `mcp_servers.<name>.enabled` | `boolean` | `yes` | `user` | `[mcp_servers.<name>]` `enabled` on a stdio or HTTP MCP server. |
| `mcp_servers.<name>.env` | `table` | `yes` | `user` | `[mcp_servers.<name>]` `env` on a stdio or HTTP MCP server. |
| `mcp_servers.<name>.expose_image_base64` | `boolean` | `yes` | `user` | `[mcp_servers.<name>]` `expose_image_base64` on a stdio or HTTP MCP server. |
| `mcp_servers.<name>.headers` | `table` | `yes` | `user` | `[mcp_servers.<name>]` `headers` on a stdio or HTTP MCP server. |
| `mcp_servers.<name>.oauth` | `table` | `yes` | `user` | `[mcp_servers.<name>]` `oauth` on a stdio or HTTP MCP server. |
| `mcp_servers.<name>.oauth_client_id` | `string` | `yes` | `user` | `[mcp_servers.<name>]` `oauth_client_id` on a stdio or HTTP MCP server. |
| `mcp_servers.<name>.oauth_client_secret_env_var` | `string` | `yes` | `user` | `[mcp_servers.<name>]` `oauth_client_secret_env_var` on a stdio or HTTP MCP server. |
| `mcp_servers.<name>.oauth_scopes` | `string[]` | `yes` | `user` | `[mcp_servers.<name>]` `oauth_scopes` on a stdio or HTTP MCP server. |
| `mcp_servers.<name>.setup` | `table` | `yes` | `user` | `[mcp_servers.<name>]` `setup` on a stdio or HTTP MCP server. |
| `mcp_servers.<name>.startup_timeout_sec` | `number` | `yes` | `user` | `[mcp_servers.<name>]` `startup_timeout_sec` on a stdio or HTTP MCP server. |
| `mcp_servers.<name>.tool_timeout_sec` | `number` | `yes` | `user` | `[mcp_servers.<name>]` `tool_timeout_sec` on a stdio or HTTP MCP server. |
| `mcp_servers.<name>.tool_timeouts` | `table` | `yes` | `user` | `[mcp_servers.<name>]` `tool_timeouts` on a stdio or HTTP MCP server. |
| `mcp_servers.<name>.type` | `string` | `yes` | `user` | `[mcp_servers.<name>]` `type` on a stdio or HTTP MCP server. |
| `mcp_servers.<name>.url` | `string` | `yes` | `user` | `[mcp_servers.<name>]` `url` on a stdio or HTTP MCP server. |

### `memory`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `memory.enabled` | `boolean` | `pin` | `user` | Cross-session memory master switch. Also GROK_MEMORY. |

### `model`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `model.<id>` | `table` | `yes` | `user` | Per-model override or BYOK definition. Prefer `env_key` over inline `api_key`. |
| `model.<id>.agent_type` | `string` | `yes` | `user` | Agent definition type associated with this model. |
| `model.<id>.api_backend` | `chat_completions / responses / messages` | `yes` | `user` | Wire protocol for this model. |
| `model.<id>.api_base_url` | `string` | `yes` | `user` | Alternate API base used with XAI_API_KEY resolution. |
| `model.<id>.api_key` | `string` | `yes` | `user` | Inline API key. Prefer `env_key`. Not a secret to put in a shared repo. |
| `model.<id>.auth_provider` | `string` | `yes` | `user` | Name of a `[auth_provider.<name>]` helper that mints this model's bearer token. |
| `model.<id>.auto_compact_threshold_percent` | `integer` | `yes` | `user` | Per-model auto-compact threshold (0-100). |
| `model.<id>.base_url` | `string` | `yes` | `user` | Provider endpoint base URL. |
| `model.<id>.compaction_at_tokens` | `number / table` | `yes` | `user` | Token threshold that triggers compaction for this model. |
| `model.<id>.compactions_remaining` | `string / table` | `yes` | `user` | How compaction leftover context is sent. Alias `send_compactions_remaining`. |
| `model.<id>.context_window` | `number` | `yes` | `user` | Context window tokens; drives auto-compact timing. |
| `model.<id>.description` | `string` | `yes` | `user` | Optional description shown in the picker. |
| `model.<id>.env_http_headers` | `map<string,string>` | `yes` | `user` | HTTP headers populated from environment variables when set. |
| `model.<id>.env_key` | `string / string[]` | `yes` | `user` | Environment variable name(s) holding the provider API key. |
| `model.<id>.extra_headers` | `map<string,string>` | `yes` | `user` | Per-request headers for this model. |
| `model.<id>.hidden` | `boolean` | `yes` | `user` | Hide this model from the picker. Still usable via `-m`. |
| `model.<id>.inference_idle_timeout_secs` | `number` | `yes` | `user` | Idle timeout for streaming inference on this model. |
| `model.<id>.max_completion_tokens` | `number` | `yes` | `user` | Per-model max completion tokens. |
| `model.<id>.max_retries` | `number` | `yes` | `user` | Inference retries for this model. |
| `model.<id>.model` | `string` | `yes` | `user` | Model id sent to the API. |
| `model.<id>.model_family` | `string` | `yes` | `user` | Family id used for compaction and capability grouping. |
| `model.<id>.model_provider` | `string` | `yes` | `user` | Named `[model_providers.<name>]` provider id for this model. |
| `model.<id>.name` | `string` | `yes` | `user` | Label shown in the model picker. |
| `model.<id>.query_params` | `map<string,string>` | `yes` | `user` | Extra query parameters on this model's requests. |
| `model.<id>.reasoning_effort` | `string` | `yes` | `user` | Deprecated per-model effort; prefer `reasoning_efforts`. |
| `model.<id>.reasoning_efforts` | `array of tables` | `yes` | `user` | Allowed reasoning-effort values for this model. |
| `model.<id>.show_model_fingerprint` | `boolean` | `yes` | `user` | Show the provider model fingerprint in the UI when present. |
| `model.<id>.stream_tool_calls` | `boolean` | `yes` | `user` | Per-model tool-call streaming request shape. |
| `model.<id>.supported_in_api` | `boolean` | `yes` | `user` | Whether this catalog entry is offered as a public API model. |
| `model.<id>.supports_backend_search` | `boolean` | `yes` | `user` | Whether the endpoint supports Grok-hosted server-side search tools. |
| `model.<id>.supports_reasoning_effort` | `boolean` | `yes` | `user` | 已弃用；请改用 `reasoning_efforts`。 |
| `model.<id>.system_prompt_label` | `string` | `yes` | `user` | Per-model system-prompt identity label. |
| `model.<id>.temperature` | `number` | `yes` | `user` | Per-model sampling temperature. |
| `model.<id>.top_p` | `number` | `yes` | `user` | Per-model top_p. |
| `model.<id>.use_concise` | `boolean` | `yes` | `user` | Use the concise tool-description pack for this model. |

### `model_providers`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `model_providers.<name>` | `table` | `yes` | `user` | Named custom model provider definition. |

### `models`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `models.agent_type` | `string` | `yes` | `user` | Fallback agent_type for models without a per-model override. |
| `models.allowed_models` | `string[]` | `pin` | `user` | Glob allowlist for the model picker, default, and `-m`. Empty means no restriction. |
| `models.default` | `string` | `pin` | `user` | Model used for new sessions. Also `GROK_DEFAULT_MODEL`, `--model`, `-m`. |
| `models.default_reasoning_effort` | `string` | `yes` | `user` | Default reasoning effort for the default model when the model supports it. |
| `models.disabled_models` | `string[]` | `yes` | `user` | Remove these model IDs from the catalog. Wins over `hidden_models`. |
| `models.extra_headers` | `map<string,string>` | `yes` | `user` | Request headers applied to every model; per-model keys win. |
| `models.hidden_models` | `string[]` | `yes` | `user` | Hide these model IDs from the picker; `-m` can still select them. |
| `models.image_description` | `string` | `yes` | `user` | Vision model used to transcribe user-supplied images. |
| `models.inference_idle_timeout_secs` | `number` | `yes` | `user` | Global idle timeout for streaming inference when a model leaves it unset. |
| `models.max_completion_tokens` | `number` | `yes` | `user` | Global max completion tokens default when a model leaves it unset. |
| `models.max_retries` | `number` | `yes` | `user` | Global inference retry default when a model leaves it unset. |
| `models.prompt_suggestion` | `string` | `yes` | `user` | Model pin for next-prompt ghost text. Unset falls through remote, then the session model. |
| `models.session_summary` | `string` | `yes` | `user` | Model used for session titles and summaries. |
| `models.stream_tool_calls` | `boolean` | `yes` | `user` | Global tool-call streaming request shape; some BYOK endpoints need false. |
| `models.temperature` | `number` | `yes` | `user` | Global sampling temperature default when a model leaves it unset. |
| `models.top_p` | `number` | `yes` | `user` | Global top_p default when a model leaves it unset. |
| `models.web_search` | `string` | `pin` | `user` | Model used by the client `web_search` tool. Also `GROK_WEB_SEARCH_MODEL`. |

### `path_not_found_hints`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `path_not_found_hints` | `boolean` | `yes` | `user` | Enrich path-not-found errors with CWD reminders and similar-name suggestions. |

### `paths`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `paths.extra_rule_dirs` | `string[]` | `yes` | `user` | More rule directories (each contains `*.md`). |
| `paths.extra_skill_dirs` | `string[]` | `yes` | `user` | More skill directories (each contains `<skill>/SKILL.md`). |

### `permission`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `permission.allow` | `string[]` | `yes` | `user` | Compact allow rules such as `Bash(git *)`. Deny beats ask beats allow. Project files may set this. |
| `permission.ask` | `string[]` | `yes` | `user` | Compact ask rules. Project files may set this. |
| `permission.deny` | `string[]` | `yes` | `user` | Compact deny rules. Project files may set this. |
| `permission.rules` | `array of tables` | `yes` | `user` | Verbose action/tool/pattern object rules. Project files may set this. |

### `plugins`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `plugins.disabled` | `string[]` | `yes` | `user` | Plugin IDs to discover but not load. Project files may set this. |
| `plugins.enabled` | `string[]` | `yes` | `user` | Plugin IDs to enable; needed for project plugins that default off. |
| `plugins.paths` | `string[]` | `yes` | `user` | Additional plugin directories. Project files may set this when the folder is trusted. |

### `privacy`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `privacy.privacy_banner_acked` | `string` | `—` | `—` | RFC 3339 UTC timestamp when the local privacy banner was dismissed. The pager reads user `config.toml` only. |

### `relay`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `relay.enabled` | `boolean` | `yes` | `user` | Enable session relay sync. |

### `sandbox`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `sandbox.auto_allow_bash` | `boolean` | `pin` | `user` | Skip bash permission prompts when a sandbox profile is active. Also GROK_SANDBOX_AUTO_ALLOW_BASH. |
| `sandbox.profile` | `off / workspace / read-only / strict / string` | `pin` | `user` | Filesystem sandbox profile. Also `--sandbox` and GROK_SANDBOX. |

### `session`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `session.auto_compact_threshold_percent` | `integer` | `yes` | `user` | Auto-compact when context usage reaches this percent (0–100). |
| `session.load_envrc` | `boolean` | `yes` | `user` | Inject `.envrc` variables into bash. |

### `shell_environment_policy`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `shell_environment_policy.exclude` | `string[]` | `yes` | `user` | Env names to drop from bash. Overlay-allowlisted. |
| `shell_environment_policy.ignore_default_excludes` | `boolean` | `yes` | `user` | Skip the built-in env denylist. Overlay-allowlisted. |
| `shell_environment_policy.include_only` | `string[]` | `yes` | `user` | If set, bash inherits only these env names. Overlay-allowlisted. |
| `shell_environment_policy.inherit` | `string` | `yes` | `user` | Which parent env names bash inherits. Overlay-allowlisted; cannot inject values. |
| `shell_environment_policy.set` | `map<string,string>` | `yes` | `user` | Inject env values into bash. Not overlay-allowlisted. |

### `skills`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `skills.disabled` | `string[]` | `yes` | `user` | Skill names to discover but not activate. |
| `skills.paths` | `string[]` | `yes` | `user` | Additional skill directories. |

### `storage`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `storage.cleanup_ttl_days` | `integer` | `yes` | `user` | Days a session may stay idle before its folder is deleted; media and terminal logs older than this are pruned from live sessions. Default 30. |

### `subagents`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `subagents.enabled` | `boolean` | `pin` | `user` | Subagent / task tool master switch. Also GROK_SUBAGENTS. |
| `subagents.limit_behavior` | `queue / fail` | `yes` | `user` | What to do when the concurrent subagent cap is hit. |
| `subagents.max_concurrent` | `integer` | `yes` | `user` | Max concurrent subagents. |
| `subagents.max_depth` | `integer` | `yes` | `user` | Max nested subagent depth (clamped ≥1). |
| `subagents.models.<name>` | `string` | `yes` | `user` | Per-subagent model id override. |
| `subagents.toggle.<name>` | `boolean` | `yes` | `user` | Enable or disable an individual subagent type. Omitted agents default on. |

### `telemetry`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `telemetry.otel_enabled` | `boolean` | `pin` | `user` | External OTEL master switch. Also GROK_EXTERNAL_OTEL. |
| `telemetry.otel_metrics_exporter` | `otlp / console / none` | `pin` | `user` | External OTEL metrics exporter. Also OTEL_METRICS_EXPORTER. |
| `telemetry.otel_logs_exporter` | `otlp / console / none` | `pin` | `user` | External OTEL logs exporter. Also OTEL_LOGS_EXPORTER. |
| `telemetry.otel_endpoint` | `string` | `pin` | `user` | External OTLP base endpoint. Also OTEL_EXPORTER_OTLP_ENDPOINT. Pin strips developer env and unlisted user/managed file siblings except listed. |
| `telemetry.otel_logs_endpoint` | `string` | `pin` | `user` | Logs-signal OTLP endpoint (verbatim). Also OTEL_EXPORTER_OTLP_LOGS_ENDPOINT. |
| `telemetry.otel_metrics_endpoint` | `string` | `pin` | `user` | Metrics-signal OTLP endpoint (verbatim). Also OTEL_EXPORTER_OTLP_METRICS_ENDPOINT. |
| `telemetry.otel_protocol` | `http/protobuf / grpc` | `pin` | `user` | External OTLP transport. Also OTEL_EXPORTER_OTLP_PROTOCOL. Pin strips per-signal protocol env and unlisted file siblings except listed. |
| `telemetry.otel_logs_protocol` | `http/protobuf / grpc` | `pin` | `user` | Logs-signal OTLP protocol. Also OTEL_EXPORTER_OTLP_LOGS_PROTOCOL. |
| `telemetry.otel_metrics_protocol` | `http/protobuf / grpc` | `pin` | `user` | Metrics-signal OTLP protocol. Also OTEL_EXPORTER_OTLP_METRICS_PROTOCOL. |
| `telemetry.otel_timeout` | `number` | `pin` | `user` | Export timeout in milliseconds. Also OTEL_EXPORTER_OTLP_TIMEOUT. |
| `telemetry.otel_metric_export_interval` | `number` | `pin` | `user` | Metric export interval in milliseconds. Also OTEL_METRIC_EXPORT_INTERVAL. |
| `telemetry.otel_certificate` | `string` | `pin` | `user` | PEM path of extra CA certs for the collector. Also OTEL_EXPORTER_OTLP_CERTIFICATE. CA pin does **not** strip endpoints. |
| `telemetry.otel_logs_certificate` | `string` | `pin` | `user` | Logs-signal CA PEM path. Also OTEL_EXPORTER_OTLP_LOGS_CERTIFICATE. |
| `telemetry.otel_metrics_certificate` | `string` | `pin` | `user` | Metrics-signal CA PEM path. Also OTEL_EXPORTER_OTLP_METRICS_CERTIFICATE. |
| `telemetry.otel_client_certificate` | `string` | `pin` | `user` | PEM path of the mTLS client certificate. Also OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE. Pin strips credential copies, developer endpoints, and unlisted file siblings. |
| `telemetry.otel_client_key` | `string` | `pin` | `user` | PEM path of the mTLS client key. Tokens never live in this file. Also OTEL_EXPORTER_OTLP_CLIENT_KEY. |
| `telemetry.otel_logs_client_certificate` | `string` | `pin` | `user` | Logs-signal mTLS client cert PEM path. Also OTEL_EXPORTER_OTLP_LOGS_CLIENT_CERTIFICATE. |
| `telemetry.otel_logs_client_key` | `string` | `pin` | `user` | Logs-signal mTLS client key PEM path. Also OTEL_EXPORTER_OTLP_LOGS_CLIENT_KEY. |
| `telemetry.otel_metrics_client_certificate` | `string` | `pin` | `user` | Metrics-signal mTLS client cert PEM path. Also OTEL_EXPORTER_OTLP_METRICS_CLIENT_CERTIFICATE. |
| `telemetry.otel_metrics_client_key` | `string` | `pin` | `user` | Metrics-signal mTLS client key PEM path. Also OTEL_EXPORTER_OTLP_METRICS_CLIENT_KEY. |
| `telemetry.otel_metrics_include_session_id` | `boolean` | `pin` | `user` | Attach session.id to metrics. Also OTEL_METRICS_INCLUDE_SESSION_ID. |
| `telemetry.otel_log_user_prompts` | `boolean` | `pin` | `user` | Content gate for prompt text on grok_code.user_prompt. Also OTEL_LOG_USER_PROMPTS. Pinning any content gate without listing a sibling defaults the omitted sibling off. |
| `telemetry.otel_log_tool_details` | `boolean` | `pin` | `user` | Metadata gate for tool-arg preview, paths, and verbatim names. Recommended on for SIEM join. Also OTEL_LOG_TOOL_DETAILS. Does not include full bodies. |
| `telemetry.otel_log_assistant_responses` | `boolean` | `pin` | `user` | Content gate for grok_code.assistant_response text. Unset follows otel_log_user_prompts unless a sibling gate is pinned in requirements. Env-only OTEL_LOG_USER_PROMPTS=1 must set this to 0 (or pin it false) for a prompts-only stream. Also OTEL_LOG_ASSISTANT_RESPONSES. |
| `telemetry.otel_log_tool_content` | `boolean` | `pin` | `user` | Body gate for tool_input, tool_output, full_command, and error_message. Independent of details; default off. CONTENT-only loses verbatim MCP names and paths. Also OTEL_LOG_TOOL_CONTENT. |
| `telemetry.trace_upload` | `boolean` | `pin` | `user` | Upload session traces. Requirements pin beats user config. |

### `tools`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `tools.disable_zdr_incompatible_tools` | `boolean` | `yes` | `user` | Restrict tools that need xAI-hosted output under ZDR. Also GROK_DISABLE_ZDR_INCOMPATIBLE_TOOLS. |
| `tools.media_gen.max_parallel_image_gen_calls` | `integer` | `yes` | `user` | Cap parallel image_gen/image_edit calls in one model step. Also GROK_MAX_PARALLEL_IMAGE_GEN_CALLS. |
| `tools.media_gen.max_parallel_video_gen_calls` | `integer` | `yes` | `user` | Cap parallel video_gen calls in one model step. Also GROK_MAX_PARALLEL_VIDEO_GEN_CALLS. |
| `tools.respect_gitignore` | `boolean` | `pin` | `user` | When true, search and read tools skip gitignored files. Also GROK_RESPECT_GITIGNORE. |
| `tools.zdr_video_output_s3` | `table` | `yes` | `user` | Team S3 bucket for ZDR video output. See ZDR Video Storage. |

### `toolset`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `toolset.ask_user_question.timeout_secs` | `number` | `yes` | `user` | Timeout for the ask_user_question tool. |
| `toolset.bash.auto_background_on_timeout` | `boolean` | `yes` | `user` | Background the command when the foreground timeout fires. |
| `toolset.bash.login_shell_capture` | `boolean` | `yes` | `user` | Capture the user's login shell environment for bash. Overlay-allowlisted. |
| `toolset.bash.max_timeout_secs` | `number` | `yes` | `user` | Cap on model-requested foreground timeouts. |
| `toolset.bash.output_byte_limit` | `number` | `yes` | `user` | Max captured bash output in bytes. |
| `toolset.bash.timeout_secs` | `number` | `yes` | `user` | Foreground bash command timeout in seconds. |
| `toolset.file_toolset` | `standard / hashline` | `yes` | `user` | File edit tool scheme. |
| `toolset.web_fetch.allowed_domains` | `string[]` | `yes` | `user` | Domain allowlist override for web_fetch. |
| `toolset.web_fetch.proxy_endpoint` | `string` | `yes` | `user` | Egress proxy URL for web_fetch. Also GROK_WEB_FETCH_PROXY. |
| `toolset.web_search.allowed_domains` | `string[]` | `yes` | `user` | Domain allowlist for client web_search. Overlay-allowlisted. |
| `toolset.web_search.excluded_domains` | `string[]` | `yes` | `user` | Domain denylist for client web_search. Overlay-allowlisted. |

### `ui`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `ui.approval_mode` | `string` | `yes` | `user` | 已弃用；请改用 `ui.permission_mode`。 |
| `ui.auto_dark_theme` | `string` | `yes` | `user` | Theme when `theme = auto` and the OS is dark. |
| `ui.auto_light_theme` | `string` | `yes` | `user` | Theme when `theme = auto` and the OS is light. |
| `ui.cancel_subagents_on_turn_cancel` | `ask / always_stop / always_continue` | `yes` | `user` | What to do with running subagents when cancelling a parent turn. |
| `ui.collapsed_edit_blocks` | `boolean` | `yes` | `user` | Show edits as one-line +N/-M summaries. Also GROK_COLLAPSED_EDIT_BLOCKS. |
| `ui.combine_queued_prompts` | `boolean` | `yes` | `user` | Merge consecutive plain follow-ups into one turn. |
| `ui.compact_mode` | `boolean` | `yes` | `user` | Denser message padding. Also `/compact-mode`. |
| `ui.confirm_before_rewind` | `boolean` | `yes` | `user` | Ask before rewinding conversation history. |
| `ui.contextual_hints.image_input` | `boolean` | `yes` | `user` | Clipboard image paste tip when the model accepts images. |
| `ui.contextual_hints.plan_mode` | `boolean` | `yes` | `user` | Suggest plan mode (Shift+Tab) for planning-style prompts. |
| `ui.contextual_hints.send_now` | `boolean` | `yes` | `user` | After queuing a mid-turn follow-up, Enter on an empty prompt sends now. |
| `ui.contextual_hints.small_screen` | `boolean` | `yes` | `user` | Suggest `/compact-mode` on short terminals. |
| `ui.contextual_hints.ssh_wrap` | `boolean` | `yes` | `user` | Recommend `chaos wrap` when SSH lacks a clipboard sink. |
| `ui.contextual_hints.undo` | `boolean` | `yes` | `user` | Ctrl+Z restores a wiped prompt draft tip. |
| `ui.contextual_hints.word_select` | `boolean` | `yes` | `user` | After double-click with fold/nav selection, point at Word select in settings. |
| `ui.cursor_blink` | `boolean` | `yes` | `user` | Force blinking (true) or steady (false) block cursor. Unset inherits the terminal. |
| `ui.default_selected_permission` | `string` | `yes` | `user` | Preselected approval row on the first prompt of a session. Also GROK_DEFAULT_SELECTED_PERMISSION. |
| `ui.display_refresh.auto_cadence_enabled` | `boolean` | `yes` | `user` | Match stream/scroll cadence to display refresh rate. Also GROK_DISPLAY_REFRESH_AUTO_CADENCE. |
| `ui.follow_up_behavior` | `queue / steer` | `yes` | `user` | Mid-turn follow-up routing. |
| `ui.fork_secondary_model` | `string` | `yes` | `user` | Model for the secondary agent when forking. Defaults to the main default model. |
| `ui.group_tool_verbs` | `boolean` | `yes` | `user` | Fold consecutive read/search/list tool rows. Also GROK_GROUP_TOOL_VERBS. |
| `ui.hunk_tracker_mode` | `agent_only / all_dirty / off` | `yes` | `user` | File-change hunk tracking. Also GROK_HUNK_TRACKER and `--hunk-tracker-mode`. |
| `ui.invert_scroll` | `boolean` | `yes` | `user` | Reverse vertical scroll direction. Also GROK_INVERT_SCROLL. |
| `ui.keep_text_selection` | `flash / hold / word_select` | `yes` | `user` | In-app selection: brief flash, hold, or double-click word select. |
| `ui.max_thoughts_width` | `number` | `yes` | `user` | Column width for the thoughts panel (40–500). |
| `ui.mouse_reporting_toggle` | `boolean` | `yes` | `user` | Ctrl+R in scrollback toggles terminal mouse capture. Also GROK_MOUSE_REPORTING_TOGGLE. |
| `ui.page_flip_on_send` | `boolean` | `yes` | `user` | Snap the sent prompt to the top of the viewport. |
| `ui.permission_mode` | `default / ask / auto / always-approve` | `yes` | `user` | Default tool-permission behavior. Enterprise locks use requirements.toml. |
| `ui.prompt_suggestions` | `boolean` | `yes` | `user` | Next-prompt ghost text after each turn. Also GROK_PROMPT_SUGGESTIONS; a remote kill-switch can disable it fleet-wide. |
| `prompt_suggestions.max_output_tokens` | `number` | `yes` | `user` | Visible-output tokens for the suggestion call; clamped to 16–256, default 64, with a separate reserve for reasoning. Remote-overridable. |
| `prompt_suggestions.temperature` | `number` | `yes` | `user` | Sampling temperature for the suggestion call (default 0.2). Remote-overridable. |
| `prompt_suggestions.reasoning_effort` | `none / minimal / low / medium / high` | `yes` | `user` | Reasoning effort for the suggestion call; default and `none` disable reasoning, while other values use a supported model effort. Remote-overridable. |
| `ui.remember_tool_approvals` | `boolean` | `yes` | `user` | Show per-tool Always allow options. Also GROK_REMEMBER_TOOL_APPROVALS. |
| `ui.render_mermaid` | `auto / on / off` | `yes` | `user` | How mermaid fences render: clickable open row or raw source. |
| `ui.screen_mode` | `fullscreen / minimal` | `yes` | `user` | Default render mode for plain `chaos`. Restart required. |
| `ui.scroll_lines` | `integer` | `yes` | `user` | Lines per scroll tick (1–10). Also GROK_SCROLL_LINES. |
| `ui.scroll_mode` | `auto / wheel / trackpad` | `yes` | `user` | Scroll input classification. Also GROK_SCROLL_MODE. |
| `ui.scroll_speed` | `integer` | `yes` | `user` | Mouse/trackpad scroll speed multiplier (1–100). Also GROK_SCROLL_SPEED. |
| `ui.show_thinking_blocks` | `boolean` | `yes` | `user` | Show thinking/reasoning blocks while streaming. Also GROK_SHOW_THINKING_BLOCKS. |
| `ui.show_timeline` | `boolean` | `yes` | `user` | Per-turn tick rail instead of the scrollbar. |
| `ui.show_timestamps` | `boolean` | `yes` | `user` | Clock time next to messages. Also `/timestamps`. |
| `ui.simple_mode` | `boolean` | `yes` | `user` | Readline prompt editing when true; experimental vim prompt keys when false. |
| `ui.status_line.command` | `string` | `yes` | `user` | Script for a `command` status line. Campaigns strip this path; a requirements layer still merges it. |
| `ui.status_line.type` | `disabled / command` | `yes` | `user` | Optional status-line row above the shortcuts bar. Off by default. See the status-line user guide. |
| `ui.theme` | `string` | `yes` | `user` | Color theme name, or `auto`/`system` to follow the OS. Also `/theme` and GROK_THEME. |
| `ui.ui_theme` | `string` | `yes` | `user` | Legacy alias for `ui.theme`. |
| `ui.vim_mode` | `boolean` | `yes` | `user` | Vim keys in the scrollback, not the prompt. Also `/vim-mode`. |
| `ui.voice_capture_mode` | `hold / toggle` | `yes` | `user` | Hold-to-talk or press-to-toggle voice capture. |
| `ui.voice_keybind_enabled` | `boolean` | `yes` | `user` | Enable Ctrl+Space / F8 for voice dictation. `/voice` still works when false. |
| `ui.voice_stt_language` | `string` | `yes` | `user` | Speech-to-text language code or `auto`. Overrides `[voice].language` for the session. |
| `ui.yolo` | `boolean` | `pin` | `user` | Always-approve tool calls. Requirements can pin false and block `--yolo`. |

### `version_overrides`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `version_overrides` | `array of tables` | `yes` | `user` | Per-CLI-version config patches applied before merge. See `[[version_overrides]]`. |

### `voice`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `voice.api_base` | `string` | `yes` | `user` | HTTPS API root for speech-to-text. Unset inherits `[endpoints].xai_api_base_url`. |
| `voice.language` | `string` | `yes` | `user` | Preferred STT language catalog code or `auto`. |
| `voice.sample_rate` | `number` | `yes` | `user` | STT capture rate in Hz. |

### `workflows`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `workflows.enabled` | `boolean` | `yes` | `user` | 启用工作流。 |

### `worktree`

| 键 | 类型 / 取值 | requirements.toml 可否设置 | managed_config.toml 是否生效 | 说明 |
| --- | --- | --- | --- | --- |
| `worktree.auto_gc` | `table` | `yes` | `user` | Automatic worktree garbage collection policy. |

## managed_config.toml

`managed_config.toml` 接受上面各表中的所有键。它设置团队默认值，因此开发者自己的 `config.toml` 会覆盖它。想让别人能调整的值放这里，不能调整的值放 `requirements.toml`。

One exception to that rule:

| 键 | 行为 |
| --- | --- |
| `features.remote_fetch` | The managed value wins over the developer's. |

Chaos 先读取 `/etc/grok/managed_config.toml`，再读取 `$CHAOS_HOME/managed_config.toml`，后者由控制台保持同步。后者的值会替换前者的值。

上面各表的 **Managed** 列给出每个键的答案：`fleet` 表示团队值生效，`user` 表示用户文件胜出，`—` 表示忽略本文件。

## requirements.toml

`requirements.toml` 是管理员强制下发的文件。位置依次为 `$CHAOS_HOME/requirements.toml`（带签名的缓存）、`/etc/grok/requirements.toml`，以及 macOS MDM `ai.x.grok`。`config.toml` 各表里的 **Requirements** 列列出该文件接受的每个 `config.toml` 键（`pin` 或 `yes`）。未列出的键不受约束。

These keys exist only in `requirements.toml`:

| 键 | 类型 / 取值 | 默认 | 说明 |
| --- | --- | --- | --- |
| `fail_closed` | `boolean` | `false` | Refuse to start when signed requirements or version_overrides cannot be applied; default false. |
| `features.image_edit` | `boolean` | — | Pin image_edit availability. Requirements only; a user-file entry is unrecognized and unset leaves the remotely configured default. |
| `ui.disable_bypass_permissions_mode` | `boolean` | — | Lock always-approve off. The lock is enforced only from a requirements layer; true in user or managed files is ignored. |

## 设置被拒绝时会发生什么

| 场景 | What Grok Build does |
| --- | --- |
| A developer sets a key you pinned | The pinned value applies. `chaos inspect` lists the requirements file that contributed. |
| A developer sets a key you shipped in `managed_config.toml` | Their value applies, except `features.remote_fetch`. Pin the key instead if it must hold. |
| `requirements.toml` is missing or its signature does not verify | The pins do not apply, and Grok Build starts without them. Set `fail_closed = true` to refuse to start instead. |
| A pinned key names a value this version does not recognise | The key is ignored and the rest of the file still applies. |

## Check what is in effect

在开发者的机器上运行 `chaos inspect`。它会列出每个参与合并的配置文件，包括 requirements 与 managed 层，因此哪条策略没有生效，一条命令就能看清。
