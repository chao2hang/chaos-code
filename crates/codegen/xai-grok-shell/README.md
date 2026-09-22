# Chaos

基于终端的 AI 编程助手与代理框架。

你可以把它当作交互式 TUI 使用，也可以通过无头模式与 Agent Client Protocol (ACP) 集成到你自己的应用里。

## 快速开始

```bash
# Install
npm i -g chaos-code

# Interactive TUI
chaos

# Headless (for scripts/automation)
chaos -p "Explain this codebase"

# Agent mode (for IDE/app integration)
chaos agent stdio
```

## 目录

- [安装](#installation)
- [认证](#authentication) — 浏览器登录、API key、OIDC、外部认证 Provider
- **使用 Chaos**
  - [交互式 TUI](#interactive-tui) — 快捷键、斜杠命令、文件引用
  - [无头模式](#headless-mode) — 脚本、CI/CD、输出格式
  - [Agent 模式](#agent-mode) — stdio、ACP 集成
  - [SSH 透传](#ssh-passthrough-grok-ssh) — Apple Terminal 剪贴板支持
- **配置**
  - [配置文件](#configuration) — 通用设置、遥测、LSP、企业部署
  - [自定义模型](#custom-models) — BYOK、Ollama、OpenAI、自定义端点
  - [MCP 服务器](#mcp-servers) — 外部工具集成
- **定制**
  - [项目规则 (AGENTS.md)](#agentsmd) — 每个项目的系统提示指令
  - [技能](#skills) — 可复用的提示包
  - [代理配置](#agent-profiles) — 自定义代理定义
  - [子代理](#subagents) — 并行的子会话、角色、人格
  - [插件](#plugins) — 外部工具/技能包
  - [钩子](#hooks) — 项目生命周期脚本
- **功能**
  - [记忆](#memory) — 跨会话的知识持久化
  - [沙箱](#sandbox) — 操作系统级文件系统/网络隔离
- **参考**
  - [自省（`chaos inspect`）](#introspection)
  - [Claude Code 兼容性](#claude-code-compatibility)
  - [内置工具](#built-in-tools)
  - [会话持久化](#session-persistence) — 存储布局、恢复
  - [文件位置](#file-locations)
  - [环境变量](#environment-variables)
  - [故障排查](#troubleshooting)
- [用 Chaos 构建](#building-with-grok) — 无头 API、ACP SDK 集成

---

## 安装

用 npm 安装（发布的包名是 `chaos-code`，装好后的命令是 `chaos`）：

```bash
# Install latest stable
npm i -g chaos-code

# Or via the install script, which installs under ~/.chaos/bin
# (the legacy ~/.grok/bin is still supported)
curl -fsSL https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.sh | bash

# Build from source — the binary lands at target/release/chaos
cargo build -p xai-grok-pager-bin --release
```

验证安装：

```bash
chaos --version
```

更新到最新版本：

```bash
chaos update
```

---

## 认证

Chaos 是 **BYOK**（自带密钥）：它不使用 xAI 账号登录，也没有浏览器 OAuth。你只要在 `~/.chaos/config.toml` 里配好要用的 Provider 凭据，就能开始使用。

配 Provider 的入口有两个：TUI 里运行 `/provider` 斜杠命令打开 Provider/模型配置面板，或直接编辑 `~/.chaos/config.toml`。`XAI_API_KEY` 以及每个模型自己的 `api_key` / `env_key` 都照常生效。`chaos login` 只是为保持命令路径兼容而保留的空操作：它不启动任何登录流程，只打印一句提示，让你去配置 Provider。

中文认证说明随安装包一起落到配置目录：`~/.chaos/docs/user-guide/02-authentication.md`。

### 浏览器登录（默认）

> **注意：** 本节讲的是上游的 xAI 账号登录（grok.com 浏览器 OAuth）。该能力需要 xAI 账号，而 Chaos 不登录 xAI 账号，所以在 Chaos 里不适用。下面保留的是上游的做法，供对照了解。

上游行为：首次启动时，Grok 会打开浏览器，让你用 grok.com 完成认证：

```bash
chaos
```

凭据保存在 `~/.grok/auth.json` 中，跨会话有效。token 7 天后过期；需要时 Grok 会提示你重新认证。

### 重新认证

要更换 Provider 或修复凭据问题，请编辑 `~/.chaos/config.toml`，或在 TUI 里用 `/provider`。Chaos 没有账号登录流程，所以也没有「重新登录」这一步——`chaos login` 只是个兼容桩，不会启动任何登录流程：

```bash
chaos login
```

### API 密钥

在 CI/CD、自动化，或没有浏览器访问的环境里，使用 [console.x.ai](https://console.x.ai) 的 API key：

```bash
export XAI_API_KEY="xai-..."
chaos
```

Chaos 是 BYOK：API key 就是默认的认证方式，不需要任何登录。也可以把凭据写进 `~/.chaos/config.toml`，用每个模型的 `api_key` 直接给 key，或用 `env_key` 指向存放 key 的环境变量。

### OIDC（客户 SSO）

> **注意：** 本节讲的是上游面向企业的 OIDC / SSO 登录（用你自己的 IdP 取代 `accounts.x.ai`）。该能力需要 xAI 账号体系配合，Chaos 不登录 xAI 账号，所以在 Chaos 里不适用；下面保留上游内容供对照。

用你自己的身份提供方（Okta、Azure AD、Auth0）代替 `accounts.x.ai` 来认证开发者。

**1. 在你的 IdP 里注册一个公共客户端：**
- 授权类型：`Authorization Code with PKCE`
- 重定向 URI：`http://127.0.0.1/callback`（CLI 使用随机临时端口；按 [RFC 8252 §7.3](https://tools.ietf.org/html/rfc8252#section-7.3)，大多数 IdP 把 loopback 重定向视为与端口无关）
- 不需要客户端密钥（只用 PKCE，见 [RFC 8252](https://tools.ietf.org/html/rfc8252)）

**2. 配置 CLI**（配置文件或环境变量）：

```toml
# ~/.chaos/config.toml
[grok_com_config.oidc]
issuer = "https://acme.okta.com"
client_id = "0oa1b2c3d4e5f6g7h8i9"
```

```bash
# Or via environment variables
export GROK_OIDC_ISSUER="https://acme.okta.com"
export GROK_OIDC_CLIENT_ID="0oa1b2c3d4e5f6g7h8i9"
```

企业客户通常还会覆盖 API 端点，指向自己的代理：
```bash
export GROK_CLI_CHAT_PROXY_BASE_URL="https://grok-proxy.acme.com/v1"
```

**3. 运行 `chaos`。** CLI 通过 `{issuer}/.well-known/openid-configuration` 发现端点，打开 IdP 登录页，并把 token 存到 `~/.grok/auth.json`。OIDC token 以 `Authorization: Bearer` 发给配置好的代理。token 会借助保存下来的 `refresh_token` 静默自动刷新。

**可选字段：**

| 字段 | 默认值 | 说明 |
|-------|---------|-------|
| `scopes` | `["openid", "profile", "email", "offline_access"]` | `offline_access` 开启静默刷新 token；需要时可追加自定义 scope |
| `audience` | None | 某些 IdP（如 Auth0）要求此项 |

### 外部认证 Provider

> **注意：** 本节讲的是上游把认证委托给外部二进制/脚本、由它铸造 xAI 后端所需的会话 token。这条路仍然需要 xAI 账号，Chaos 不登录 xAI 账号，所以在 Chaos 里不适用；下面保留上游内容供对照。

当无法使用浏览器登录时（沙箱虚拟机、CI runner、隔离网络），可以把认证委托给外部二进制或脚本。对于公司自建认证基础设施（SSO、设备码流程、证书认证等）的企业部署，这是推荐做法。

Grok 与具体 Provider 无关——它不关心你的二进制如何认证。它只负责运行命令、从 stdout 读一个 token 并保存下来。你的二进制是个黑盒，整个认证流程由它自己处理。

#### 工作方式

```
┌──────────────┐     sh -c     ┌────────────────────────┐
│     Grok     │──────────────▶│  your auth binary      │
│              │               │                        │
│  reads       │◀── stdout ────│  prints token          │
│  auth.json   │               │                        │
│              │   (stderr)    │  prints status/URLs    │──▶ user's terminal
└──────────────┘               └────────────────────────┘
```

1. Grok 通过 `sh -c "<command>"` 运行你的命令
2. 你的二进制执行它需要的任何认证流程（SSO 登录、设备码、证书交换等）
3. **stderr** → 直接显示给用户（用来输出登录 URL、状态信息、进度）
4. **stdout** → 被 Grok 捕获，作为 access token 存进 `~/.grok/auth.json`
5. exit 0 → 成功；退出码非 0 → Grok 回退到交互式登录

#### stdout / stderr 契约

这一节是最要紧的地方：

| 流 | 打印什么 | 谁看到 |
|--------|---------------|-------------|
| **stdout** | token —— 别的什么都不要 | Grok（解析并存入 `auth.json`） |
| **stderr** | 登录 URL、状态信息、错误、进度 | 用户（显示在其终端里） |

**除了 token，不要往 stdout 打印任何东西。** 没有进度信息、没有调试输出、没有 "Login successful!" 之类的文字。Grok 会逐字读取 stdout 并尝试把它解析成 token。任何多余文字都会破坏解析。

#### stdout 上的 token 格式

stdout 上的 token 可以是下面两种之一：

**1. 裸字符串** —— 就是原始 token，别无其他：
```
eyJhbGciOiJSUzI1NiIs...
```

**2. JSON** —— 可以带上 refresh token 与过期时间：
```json
{"access_token": "eyJhbGciOi...", "refresh_token": "ref-tok", "expires_in": 3600}
```

如果你的 token 会过期、且希望 Grok 在过期前自动重跑该二进制，就用 JSON。`expires_in` 字段（距过期的秒数）告诉 Grok 何时主动刷新。没有它的话，Grok 假定 token 有效期 30 天。

#### 最小示例

```bash
#!/bin/sh
# Print login URL / status to stderr (user sees this)
echo "Authenticating via Acme Corp SSO..." >&2
echo "Visit: https://sso.acme.com/device-login?code=ABCD-1234" >&2

# ... do the auth flow, get a token ...

# Print ONLY the token to stdout (Grok captures this)
echo "eyJhbGciOiJSUzI1NiIs..."
```

#### 配置

```toml
# ~/.chaos/config.toml
[auth]
auth_provider_command = "/usr/local/bin/my-auth-provider"
auth_provider_label = "Acme Corp"   # optional — customizes the TUI login button
auth_token_ttl = 3600               # optional — token lifetime in seconds (see below)
```

```bash
# Or via environment variables
export GROK_AUTH_PROVIDER_COMMAND="/usr/local/bin/my-auth-provider"
export GROK_AUTH_PROVIDER_LABEL="Acme Corp"   # optional
export GROK_AUTH_TOKEN_TTL=3600               # optional
```

如果你的二进制输出的是裸 token 字符串（而不是带 `expires_in` 的 JSON），就把 `auth_token_ttl` 设为该 token 的预期存活秒数。没有它，Grok 无法主动感知过期，只会在收到 401 之后才刷新。

命令会经由平台 shell 运行——macOS/Linux 上是 `sh -c`，Windows 上是 `cmd /C`——所以它可以是二进制路径、脚本或管道。

> **Windows：** 把路径写成 TOML *字面量*字符串（单引号），这样反斜杠才能保留：`auth_provider_command = 'C:\corp\grok-auth.exe'`。在双引号的 TOML 字符串里 `\t`、`\n`、`\r`、`\b`、`\f` 都是转义序列，所以 `"C:\temp\auth.exe"` 会被解析成含制表符的路径，Provider 启动失败——之后 Grok 会回退到浏览器登录，就像这条设置被忽略了一样。

设置 `auth_provider_label` 后，TUI 欢迎界面会显示 **"`Login with Acme Corp`"**，而不是 "`Login with grok.com`"。在无头模式（`chaos -p`）下该标签不起作用——你的二进制的 stderr 会直接打印到终端。

> **企业部署：** 想要一份把外部认证、企业代理和遥测设置组合在一起的完整企业 `config.toml`，见配置章节的[企业部署](#enterprise-deployment)。

#### 示例：设备码流程 Provider

```bash
#!/bin/sh
# 1. Request device code from your IdP
RESP=$(curl -s -X POST https://auth.acme.com/device/code -d "client_id=grok-cli")
CODE=$(echo "$RESP" | jq -r '.user_code')
URL=$(echo "$RESP" | jq -r '.verification_uri')
DEVICE_CODE=$(echo "$RESP" | jq -r '.device_code')

# 2. Show login URL to user (stderr — user sees this in their terminal)
echo "Open $URL and enter code: $CODE" >&2

# 3. Poll until user approves
while true; do
  TOKEN=$(curl -s -X POST https://auth.acme.com/device/token \
    -d "device_code=$DEVICE_CODE&grant_type=urn:ietf:params:oauth:grant-type:device_code" \
    | jq -r 'select(.access_token) | .access_token')
  [ -n "$TOKEN" ] && break
  sleep 5
done

# 4. Print token to stdout — JSON format enables auto-refresh
echo "{\"access_token\": \"$TOKEN\", \"expires_in\": 3600}"
```

#### 示例：支持刷新的认证二进制

Grok 会按两种不同的契约运行你的二进制，`GROK_AUTH_EXPIRED` 就是区分它们的方式：

| | `GROK_AUTH_EXPIRED=1` | 未设置 |
|---|---|---|
| **这是什么** | 一次无头刷新，基于 Grok 手上已有的凭据——临近过期的轮换，或服务端拒绝掉的 token | 一次登录：登录界面（上游为 `grok login`），或某次无头运行无法铸造 token 之后升级而来的登录 |
| **有人在看吗？** | 没有。stdin 是关闭的，也没有什么东西会渲染你的提示 | 有。用户在等，你的 stderr 能送到他那里 |
| **预算** | 几秒（7s），超时后 Grok 会杀掉进程 | 300s —— 足够走一趟浏览器往返或一次设备码 |
| **所以你的二进制应该** | 静默铸造，或者**以非零码退出**。绝不阻塞 | 走完整的 SSO 流程，并且每次都铸造新 token |

```bash
#!/bin/sh
if [ "$GROK_AUTH_EXPIRED" = "1" ]; then
    # Headless: silent refresh only. If that can't work — the SSO session
    # lapsed, say — exit non-zero rather than block. Grok then shows the
    # sign-in screen, which re-runs this binary with the variable unset.
    echo "Refreshing token..." >&2
    TOKEN=$(my-company-auth --refresh --silent) || exit 1
else
    # A user is attached — full interactive SSO flow.
    echo "Authenticating via Acme Corp SSO..." >&2
    TOKEN=$(my-company-auth --login --interactive)
fi

if [ -z "$TOKEN" ]; then
    echo "Authentication failed" >&2
    exit 1
fi

echo "{\"access_token\": \"$TOKEN\", \"expires_in\": 3600}"
```

在 `GROK_AUTH_EXPIRED=1` 时迅速退出，才能让交接到登录界面这一步足够快：如果二进制反过来阻塞在那里，那么每次带着过期 token 启动都要把整个刷新超时耗满。

有一种情况始终有歧义，而且只出现在 **leader 模式**下（`--leader`，或 `[cli] use_leader = true`；默认关闭）：在完全没有凭据时，leader 会在启动后不久在后台多试一次，而那次运行里该变量是未设置的，和登录一样。不需要人工介入就能铸造的二进制（服务账号、keytab、挂载的 token）会在那次尝试中成功，会话自行恢复。必须交互提示的二进制则只是干等，最多等到 300s 的登录上限——没有任何东西在等它，登录界面早已显示，它的 stderr 会写进 `~/.chaos/leader.log`，而不是给用户看。

`GROK_AUTH_EXPIRED` 是可选的——即使你的二进制忽略它，Grok 也能工作。只是登录和刷新会跑同一条流程，而需要交互提示的流程会在无头运行中被杀，来不及完成。

### 自动凭据刷新

> **注意：** 本节讲的自动凭据刷新依赖 xAI 账号的会话凭据，Chaos 不登录 xAI 账号，所以在 Chaos 里不适用；下面保留上游内容供对照。

Grok 支持对外部认证 Provider 与 OIDC 的自动凭据刷新。当 Grok 发现你的 token 已过期（要么根据本地的 `expires_in` 判断，要么因服务端返回 401），它会在重试请求之前自动重跑你的 `auth_provider_command` 以取得新凭据。

这个过程是透明的——你什么都不用做。Grok 会在会话期间于后台处理。

**什么时候会刷新？**

- **过期之前：** 如果你的二进制在 JSON 输出里返回了 `expires_in`，或者你在配置里设了 `auth_token_ttl`，Grok 会在 token 过期前约 5 分钟重跑该二进制，这样你不会看到认证错误。
- **遇到认证错误时：** 如果服务端以 401/403 拒绝请求（例如 token 被吊销或已过期），Grok 会重跑该二进制并把请求重试一次。
- **刷新那一次跑不出 token 时：** 刷新是无头的（没有 stdin、超时很短），所以需要你完成 SSO 流程的二进制在那里无法成功。此时 Grok 不再把已存的凭据视为可用，改为以交互模式运行你的二进制——在启动阶段，这与一台没有任何凭据的机器得到的登录流程相同；在会话中途，该回合会以「需要重新认证」的提示失败，`/login` 会重跑该二进制。
- **OIDC：** 如果你用的是 OIDC 且有 `refresh_token`，Grok 会经由你的 IdP 静默刷新，不会重开浏览器。

**调整刷新提前量：**

```bash
# Grok refreshes tokens 5 minutes before expiry by default.
# Set to 0 to only refresh on 401. Set higher for very short-lived tokens.
export GROK_AUTH_EARLY_INVALIDATION_SECS=300
```

**另外几点：**
- 在上游，使用 `auth_provider_command` 时不需要先运行 `grok login`——首次启动时 Grok 会在真实终端上运行你的二进制（URL 和进度走 stderr），然后直接打开已登录的界面。如果你愿意，也可以运行 `grok login` 提前把 `auth.json` 填好。会话中途的 `/login` 仍然使用 TUI 内的「复制链接」浮层。
- 如果同时配置了 OIDC 和 `auth_provider_command`：在**登录**时，Grok 先试 OIDC 静默刷新（如果存在 `refresh_token`），然后试外部二进制，最后才走浏览器登录。在**会话**期间，配置了哪种方法就只用哪种——设了 `auth_provider_command` 就由它负责所有会话中途的刷新；否则用 OIDC 静默刷新。
- 你的二进制的 stderr 输出会显示给用户，但不支持交互式 stdin。对于二进制展示一个 URL、你在浏览器里完成认证的浏览器型 SSO 流程，这已经够用。

#### 认证排查

打开调试日志来跟踪认证流程：

```bash
chaos --debug-file /tmp/grok-auth.log -p "hello"
tail -f /tmp/grok-auth.log
```

常见的日志消息：

| 日志消息 | 含义 |
|-------------|---------------|
| `auth: running external auth provider (headless refresh)` | 你的二进制正被以 `GROK_AUTH_EXPIRED=1` 调用，只有几秒时间 |
| `auth: running external auth provider (interactive login)` | 你的二进制正被按登录契约调用：没有 `GROK_AUTH_EXPIRED`，stderr 会展示，限时 300s |
| `auth: external auth provider returned fresh token` | 成功——token 已被解析并保存 |
| `auth: external auth provider failed` | 二进制以非零码退出，或退出码为 0 但 stdout 为空/无法解析（细节在 `error` 字段里） |
| `auth: external auth provider timed out (likely needs interactive auth), killing` | 二进制没能在 7s 的无头刷新超时前退出，已被杀掉。在 `GROK_AUTH_EXPIRED=1` 时以非零码退出就可以完全免掉这段等待 |
| `auth: failed to start external auth provider` | 命令无法启动（例如找不到该二进制） |

### 每个模型的 Auth Provider

> **注意：** 命名 auth provider 的凭据助手机制在本分叉照常可用，它就是每个模型 `api_key`/`env_key` 的「token 会轮换」版本。但上面那个 `auth_provider_command` 替换的是 *会话* 认证——铸给 xAI 后端的 token；Chaos 不登录 xAI 账号，因此那一部分在 Chaos 里不适用。

上面的 `auth_provider_command` 替换的是 Grok 的 *会话* 认证：它铸造发给 xAI 后端的 token。如果你想要的是「xAI 模型仍走常规 xAI 登录，而**其他模型**走一个 bearer token 会轮换的网关（LiteLLM、企业代理）」，那就用命名 auth provider——也就是每个模型 `api_key`/`env_key` 的「token 会轮换」版本。

```toml
# ~/.chaos/config.toml
[auth_provider.litellm]
command = "/usr/local/bin/litellm-token"   # run via `sh -c`
token_ttl_secs = 3600                      # optional: see below
timeout_secs = 10                          # optional: command timeout (default 30)

[model.proxied-claude]
model = "claude-sonnet-4-5"
base_url = "https://litellm.corp.example/v1"
context_window = 200000
auth_provider = "litellm"
```

**契约**（stdout 契约与 `auth_provider_command` 相同；`issuer` 字段会被接受，但在这里不起作用；`refresh_token` 若存在，会在刷新时交回给命令）：

- 不带 `args` 时，命令经由 POSIX `sh -c` 运行，所以它可以是二进制路径、脚本或管道。带 `args = ["..."]` 时，命令会直接以这些参数运行、不经过 shell：`command` 是通过 `PATH` 解析的程序名，或一个路径。用 `args` 可以避免 shell 引号问题，在 Windows 上也需要它（那里没有 `sh`）。
- stdout：裸 token，或 JSON `{"access_token": "...", "expires_in": 3600}`。
- stderr：命令失败时会记入日志；退出码 0 = 成功。
- 只要 Grok 是在内存里仍有缓存 token 的情况下重新铸造，就会设置 `GROK_AUTH_EXPIRED=1`，无论起因是临近过期的轮换还是被服务端拒绝。冷缓存下的第一次铸造不带该变量。

**token 生命周期：**

- token 按 Provider 缓存在内存中，并被引用该 Provider 的每个模型共享；不会写入磁盘。命令是凭据助手：持久化存储和 OAuth2 刷新由它自己负责（钥匙串、它自己的点目录等），完全类似 `gcloud auth print-access-token` 或 git 的凭据助手。会话中途重新铸造时，上一份凭据会通过 `GROK_AUTH_PROVIDER_ACCESS_TOKEN` 交回（若存在，还会带 `GROK_AUTH_PROVIDER_REFRESH_TOKEN` / `GROK_AUTH_PROVIDER_EXPIRES_AT`），这样支持 refresh grant 的命令可以刷新而不是重新认证。命令必须非交互且足够快；交互式登录请在带外完成，重启时 Grok 会重跑该命令重新铸造。
- 当 token 缺失、或距过期约一分钟以内时，Grok 会在聊天回合之前运行该命令；服务端拒绝某个 token 之后还会再运行一次。若 token 在取得后 30 秒内就被拒绝，则不会再次获取，因此坏掉的助手只会给出一个清楚的错误，而不是陷入循环。
- token 的存活时间依次取自：命令 JSON 输出里的 `expires_in`、否则 `token_ttl_secs`、再否则 token 自身 JWT 的过期声明。三者都没有时，只有在服务端拒绝某个 token 之后才会替换它。
- 命令受 `timeout_secs` 限制（默认 30，被夹在 1..=600 之间），超时即被杀掉。回合会等待这次运行，所以请让助手既快又非交互。
- 正在运行的会话会在下一次切换模型或新建会话时接收到 Provider 表的修改或删除。一旦接收到，修改会作废缓存的 token，于是被修改的命令会在下次使用时运行；删除则会丢掉缓存的 token。
- 辅助模型（web search、会话摘要、图像描述）只读共享缓存，从不运行该命令；请把它们指向你的聊天模型保持热度的那些 Provider。子代理以其父会话相同的方式刷新 token。

**与其他凭据的关系：** 模型上字面写明的 `api_key`/`env_key` 优先于它的 `auth_provider`。走 Provider 的模型是 BYOK：你的 xAI 会话 token 永远不会发给它们的端点，Provider 命令失败会让请求失败，而不会回退到会话 token。

**安全：** Provider 命令会执行代码，所以只被信任的配置层接受（`~/.chaos/config.toml`、托管配置、requirements）。项目的 `.chaos/config.toml` 永远不能定义它。哪一层设置了模型的 `base_url`，就决定了该模型铸造出的 token 发往哪里；而 `base_url`（与 Provider 表不同）不会被从远端或 campaign 补丁中剥除，静态 `env_key` 也是如此。请把 Provider 表和模型的 `base_url` 放在你信任的层里。命令会继承 Grok 的环境（因此能看到 `PATH`、`HOME` 以及其中的其他秘密），但 Grok 自己的一方凭据（`XAI_API_KEY`、`GROK_DEPLOYMENT_KEY` 及相关 key）会被移除，BYOK 助手绝不会收到它们；写助手时只读它自己需要的东西，并优先使用 `GROK_AUTH_PROVIDER_*` 交回上一份凭据。

### 用 auth.json 访问 API

> **注意：** 本节讲的是用上游 xAI 登录凭据直接调用 `cli-chat-proxy.grok.com`。该能力需要 xAI 账号，Chaos 不登录 xAI 账号，因此在 Chaos 里不适用；下面保留上游内容供对照。`~/.grok/auth.json` 是上游兼容遗留路径，请勿依赖。

如果你已用 `grok login` 完成认证，就可以拿保存下来的凭据直接用 curl 调 CLI chat proxy。该代理要求一组特定的请求头，与 grok CLI 内部发送的一致：

```bash
curl -s -N -X POST "https://cli-chat-proxy.grok.com/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $(jq -r '."https://accounts.x.ai/sign-in".key' ~/.grok/auth.json)" \
  -H "X-XAI-Token-Auth: xai-grok-cli" \
  -H "x-grok-model-override: grok-build" \
  -d '{
    "model": "grok-build",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

**必需的请求头：**

| 请求头                           | 是否必需 | 用途                                                                                                                                                                                   |
| -------------------------------- | -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Authorization: Bearer <token>`  | 是      | `~/.grok/auth.json` 里的会话 token（由 `grok login` 写入）                                                                                                                              |
| `X-XAI-Token-Auth: xai-grok-cli` | 是      | 告诉认证中间件按 CLI 会话 token 校验                                                                                                                              |
| `x-grok-model-override: <model>` | 是\*    | 代理用这个请求头（而不是 JSON body）路由到正确的后端。\*对 `grok-build` 可以省略，它在默认路由上，但带上总是安全的。 |

**流式与非流式：**

代理后面的大多数模型只支持流式。除非你确定该模型支持非流式，否则一律用 `"stream": true`。

| 模型                 | 非流式  | 流式    |
| --------------------- | -------------- | ------------ |
| `grok-build`    | ✅ 支持   | ✅ 支持 |

> **注意：** 上游 `auth.json` 里的 token 7 天后过期，需要通过 `grok login` 刷新；Chaos 不登录 xAI 账号，该流程在 Chaos 中不适用。

---

## 交互式 TUI

TUI（终端用户界面）提供一个完整的交互式编程环境。

### 启动

```bash
chaos [OPTIONS]
```

### 选项

| Flag                       | Description                                                            |
| -------------------------- | ---------------------------------------------------------------------- |
| `--cwd <PATH>`             | 设置工作目录（默认：当前目录）                     |
| `--prompt <TEXT>`          | 启动后立即发送一条初始提示                       |
| `--rules <TEXT>`           | 向系统提示追加自定义规则                               |
| `--always-approve`         | 无需确认即自动批准所有工具执行                  |
| `--sandbox <PROFILE>`      | 操作系统级文件系统/网络护栏（见[沙箱](#sandbox)）       |
| `--light`                  | 使用浅色主题（macOS Basic）而非深色                          |
| `--single-turn`            | 首次回复后退出（需要 `--prompt`）                        |
| `--subagents`              | 启用子代理/task 工具支持（见[子代理](#subagents)）        |
| `--disable-web-search`     | 从代理工具集中移除 web search 工具                          |
| `--agent-profile <PATH>`   | 加载自定义代理定义文件（见[代理配置](#agent-profiles)） |
| `--allow <RULE>`           | 带 glob 模式的权限允许规则（可重复）。见[权限规则](#permission-rules-allow--deny)。 |
| `--deny <RULE>`            | 带 glob 模式的权限拒绝规则（可重复）。见[权限规则](#permission-rules-allow--deny)。 |

### 示例

```bash
# Start in a specific project
chaos --cwd ~/projects/my-app

# Start with an initial task
chaos --prompt "Review this codebase and suggest improvements"

# Add project-specific rules
chaos --rules "Always use TypeScript. Prefer functional components."

# Auto-approve mode for trusted tasks
chaos --always-approve --prompt "Format all files"
```

### 快捷键

| Key                          | Action                          |
| ---------------------------- | ------------------------------- |
| `Enter`                      | 发送消息                    |
| `Shift+Enter` or `Alt+Enter` | 插入换行                  |
| `Ctrl+M`                     | 切换多行输入模式     |
| `Ctrl+C` or `Esc`            | 取消当前操作        |
| `Ctrl+D` or `Ctrl+Q`         | 退出（需确认）        |
| `Ctrl+O`                     | 切换始终批准模式 |
| `Ctrl+T`                     | 切换 TODO/任务面板          |
| `Ctrl+R`                     | 搜索提示历史           |
| `Ctrl+V`                     | 从剪贴板粘贴            |
| `Ctrl+U`                     | 撤销上一次输入改动          |
| `Ctrl+G`                     | 把前台任务移到后台 |
| `Ctrl+P`                     | 切换调试面板              |

### 斜杠命令

在输入框里敲 `/` 即可调用命令：

| Command                            | Alias     | Description                                              |
| ---------------------------------- | --------- | -------------------------------------------------------- |
| `/model <name>`                    | `/m`      | 切换到另一个模型                              |
| `/new`                             |           | 开始新会话（清空上下文）                     |
| `/load [workspace] [session]`      | `/resume` | 载入此前的会话                                  |
| `/rewind <prompt>`                 |           | 回退到之前的某个提示（同时还原文件）             |
| `/compact [context]`               |           | 压缩对话历史                             |
| `/always-approve [on\|off]`        | `/yolo`   | 切换自动批准模式                                 |
| `/multiline`                       | `/ml`     | 切换多行输入模式                              |
| `/memory [workspace\|global] <text>` |         | 把文本追加到记忆文件（需要已启用记忆） |
| `/flush`                           |           | 立即把当前会话的知识写入记忆             |
| `/skills [name]`                   |           | 列出技能，或把某个技能注入上下文               |
| `/plugins [list\|reload\|trust]`   | `/plugin` | 管理插件（列出、重新加载、信任）                     |
| `/hooks-list`                      |           | 显示本次会话加载的钩子                        |
| `/hooks-trust`                     |           | 信任本目录以执行钩子（写入目录信任）        |
| `/hooks-add <path>`                |           | 添加自定义钩子文件或目录                      |
| `/feedback [message]`              |           | 报告问题或发送反馈                         |
| `/exit`                            | `/quit`   | 退出 TUI                                             |

```bash
# Example usage in TUI:
/model grok-build
/new
/rewind
/feedback Something isn't working
```

### 功能

- 代码块的**语法高亮**
- **行内 diff**：在改动被应用之前就能看到文件变化
- **工具执行进度**，带实时输出
- **TODO 面板**，跟踪任务进度
- **会话持久化** —— 对话自动保存，可以继续
- **历史搜索** —— 用 `Ctrl+R` 搜索此前的提示

### 文件引用（`@`）

在提示里用 `@` 操作符把文件内容附到消息上。输入 `@` 再跟文件名或路径，会打开模糊文件选择器，然后按 `Tab` 或 `Enter` 选中。

```
@src/main.rs              # Attach a file
@src/main.rs:10-50        # Attach lines 10–50 of a file
@src/                     # Browse a directory (end with /)
```

**用 `!` 暴露隐藏文件**

默认情况下，`@` 文件选择器遵循 `.gitignore` 规则，并隐藏点文件（以 `.` 开头的文件和目录）。要搜索隐藏文件——例如 `.github/`、`.vscode/`、`.env` 或其他点文件——在查询前加 `!`：

```
@!.github                 # Search for .github/ and other hidden files
@!.vscode/settings.json   # Find .vscode/settings.json
@!.env                    # Attach a .env file
```

`!` 修饰符让你可以附带项目里的任何文件，不受忽略规则限制。

---

## 无头模式

在命令行上以非交互方式运行 Chaos。以下场景适合用无头模式：

- **自动化任务** —— CI/CD 流水线、pre-commit 钩子、cron 任务
- **脚本化工作流** —— 批量处理文件、与其他工具串联
- **构建集成** —— 作为子代理被拉起、嵌入更大的系统
- **以程序方式解析输出** —— JSON 输出供下游处理

无头模式接收一条提示，以完整的工具权限执行它，然后返回结果。

### 基本用法

```bash
chaos -p "Your prompt here"
```

### 选项

| Flag                    | Description                                           |
| ----------------------- | ----------------------------------------------------- |
| `-p, --single <PROMPT>` | 要发送的提示（必需）                         |
| `-m, --model <MODEL>`   | 使用的模型（例如 `grok-build`）               |
| `-s, --session-id <ID>` | 用这个 ID 创建或继续一个无头会话      |
| `-r, --resume <ID_OR_TITLE>` | 按 ID 继续已有会话；对当前目录也可按标题继续，忽略大小写（若只有一条被显式重命名过的标题匹配则它胜出；其余重复项会连同各自的 ID 一起报错；形如 UUID 的值一律按 ID 处理） |
| `-c, --continue`        | 继续当前目录下最近的会话 |
| `--cwd <PATH>`          | 工作目录                                     |
| `--output-format <FMT>` | 输出格式：`plain`、`json`、`streaming-json`      |
| `--always-approve`      | 自动批准工具执行                          |
| `--rules <TEXT>`        | 系统提示的自定义规则                    |
| `--tools <TOOLS>`       | 内置工具的白名单（逗号分隔）。只有列出的工具可用，其余全部移除。仅无头模式。 |
| `--disallowed-tools <TOOLS>` | 要移除的内置工具黑名单（逗号分隔）。列出的工具会从代理工具集中剥除。支持 `Agent` / `Agent(type)` 条目以限制子代理派生（见下）。仅无头模式。 |
| `--max-turns <N>`       | 停止前允许的最大代理回合数       |
| `--reasoning-effort` / `--effort <LEVEL>` | 推理强度（`none`、`minimal`、`low`、`medium`、`high`、`xhigh`、`max`；也接受各模型菜单里的 id，如 `deep`）。TUI 与无头模式都可用。 |
| `--permission-mode <MODE>` | 工具批准的权限模式                 |
| `--allow <RULE>`        | 带 glob 模式的权限允许规则（可重复）。见下。 |
| `--deny <RULE>`         | 带 glob 模式的权限拒绝规则（可重复）。见下。  |

#### 工具过滤（`--tools` / `--disallowed-tools`）

用 `--tools` 把代理限制在一组明确的工具上（白名单），或用 `--disallowed-tools` 从默认集合里去掉指定工具（黑名单）。两者都接受逗号分隔的工具名列表。

工具名对应下面列出的内部工具 ID。速查表：

| Display Name   | 用于 `--tools` / `--disallowed-tools` 的工具 ID |
| -------------- | --------------------------------------------- |
| bash           | `run_terminal_cmd`                            |
| grep           | `grep`                                        |
| read_file      | `read_file`                                   |
| search_replace | `search_replace`                              |
| list_dir       | `list_dir`                                    |
| web_search     | `web_search`                                  |
| web_fetch      | `web_fetch`                                   |
| todo_write     | `todo_write`                                  |
| task           | `task`                                        |

```bash
# Only allow read-only tools
chaos -p "Explain this codebase" --tools "read_file,grep,list_dir"

# Remove web access and file editing
chaos -p "Review this code" --disallowed-tools "web_search,web_fetch,search_replace"

# Remove shell access
chaos -p "Review this code" --disallowed-tools "run_terminal_cmd"
```

`--disallowed-tools` 还支持特殊的 `Agent` 条目来控制子代理派生：

| Entry                          | Effect                                                  |
| ------------------------------ | ------------------------------------------------------- |
| `Agent`                        | 阻止**所有**子代理派生                         |
| `Agent(explore)`               | 只阻止 `explore` 类型的子代理                  |
| `Agent(explore, plan)`         | 阻止多个指定类型                           |

```bash
# Allow tools but prevent the agent from spawning any subagents
chaos -p "Fix this bug" --disallowed-tools "Agent"

# Block only the explore subagent
chaos -p "Refactor this module" --disallowed-tools "Agent(explore)"
```

设置了 `--tools` 后，只有列出的工具可用，默认的工具注入会被关闭。两个标志同时出现时，`--disallowed-tools` 在 `--tools` 之后生效——可以用它先取一个白名单，再去掉其中特定条目。

> **注意：** `--tools`、`--disallowed-tools` 和 `--max-turns` 只在无头模式（`-p`）下受支持。在交互式 TUI 里使用会打印一条警告并忽略该标志。`--reasoning-effort`/`--effort` 与 `--permission-mode` 在两种模式下都可用。

#### 权限规则（`--allow` / `--deny`）

权限规则决定某些具体的工具调用是自动批准、拒绝，还是需要确认。与 `--disallowed-tools`（把工具从代理工具集中彻底移除）不同，权限规则让工具保持可用，只是在执行前设卡。

规则使用 `ToolPrefix(glob_pattern)` 语法。支持的前缀：

| Prefix        | 控制的内容                   |
| ------------- | ---------------------------------- |
| `Bash(...)`   | Shell 命令执行            |
| `Edit(...)`   | 文件编辑（路径 glob）           |
| `Write(...)`  | 文件写入（路径 glob）           |
| `Read(...)`   | 文件读取（路径 glob）           |
| `Grep(...)`   | 搜索操作（路径 glob）      |
| `WebFetch(...)` | URL 抓取（glob 或 `domain:host`） |
| `MCPTool(...)` | MCP 工具调用              |

glob 模式支持 `*`（单层通配）和 `**`（递归）。不带括号的裸前缀匹配该类型的所有调用。Claude Code 的 `Bash(cmd:*)` 规则也被接受，它等价于对 `cmd` 做前缀匹配。

```bash
# Deny all shell commands matching "rm*"
chaos -p "Clean up this project" --deny "Bash(rm*)"

# Allow npm commands, deny everything else dangerous
chaos -p "Set up the project" --allow "Bash(npm*)" --deny "Bash(sudo*)"

# Deny edits outside src/
chaos -p "Refactor the code" --deny "Edit(/etc/**)"

# Allow all bash commands (auto-approve without prompting)
chaos -p "Build the project" --allow "Bash"

# Combine: allow fetching docs sites, deny other URLs
chaos --allow "WebFetch(domain:docs.rs)" --deny "WebFetch(*)"
```

`--allow` 与 `--deny` 可以重复使用以追加多条规则。拒绝规则优先于允许规则。这些标志在 TUI 和无头模式下都可用。

### 示例

```bash
# Simple question
chaos -p "What does this project do?"

# Use a specific model
chaos -p "Optimize this function" -m grok-build

# Get JSON output for parsing
chaos -p "List all TODO comments in the codebase" --output-format json

# Streaming JSON for real-time processing
chaos -p "Explain the architecture" --output-format streaming-json

# Multi-turn conversation (session ID is returned in JSON output)
chaos -p "Remember: the secret number is 42" --output-format json
chaos -p "What's the secret number?" --resume <sessionId>

# Resume most recent session
chaos -p "Continue where we left off" -c

# Run in a different directory
chaos -p "Run the tests" --cwd ~/projects/other-app --always-approve
```

### 用具名会话写脚本

面向 CI 和自动化，`-s/--session-id` 让你自选会话 ID：

```bash
# Start a session namespaced to a PR
chaos -p "Review the changes in this PR" -s "critique-myrepo-pr-123"

# Continue in the same session
chaos -p "Now check for security issues" -s "critique-myrepo-pr-123"
```

如果该会话已存在，就从你上次停下的地方继续；不存在则新建一个。
这一点与 `--resume` 不同，后者在会话不存在时会报错。

> **注意：** `-s/--session-id` 只适用于无头模式（`-p/--single`）。
> 在交互式 TUI 里请用 `/load` 或 `--resume`。

### 输出格式

**plain**（默认）—— 人类可读的文本：

```
Here's a summary of the codebase...
```

**json** —— 完成后输出单个 JSON 对象：

```json
{
  "text": "Here's a summary of the codebase...",
  "stopReason": "EndTurn",
  "sessionId": "abc123",
  "requestId": "xyz789"
}
```

**streaming-json** —— 换行分隔的 JSON 事件：

```json
{"type":"text","data":"Here's"}
{"type":"text","data":" a summary"}
{"type":"thought","data":"Analyzing the directory structure..."}
{"type":"end","stopReason":"EndTurn","sessionId":"abc123","requestId":"xyz789"}
```

### 脚本示例

```bash
# Pipe output to a file
chaos -p "Generate a README" > README.md

# Parse JSON output with jq
chaos -p "List files" --output-format json | jq -r '.text'

# CI/CD: automated code review
chaos -p "Review changes for bugs and security issues." \
  --output-format json --always-approve | jq -r '.text' > review.md

# Pipeline: chain with other tools
git diff --staged | chaos -p "Write a concise commit message for these changes"

# Batch: process multiple files
for file in src/*.js; do
  chaos -p "Migrate $file from CommonJS to ES modules." --always-approve
done

# Pre-commit hook
chaos -p "Review staged changes for obvious bugs. Reply OK if fine, or list issues." \
  --always-approve --output-format json | jq -r '.text' | grep -q "^OK" || exit 1
```

> **注意：** 无头模式默认每次新建会话。用 `-s <id>` 在多次调用之间保持上下文。

---

## Agent 模式

把 Chaos 作为 ACP（Agent Client Protocol）代理运行，用于与 IDE、编辑器及自定义工具集成。

### stdio 传输

直接与 ACP 客户端集成时使用：

```bash
chaos agent stdio
```

通信通过 stdin/stdout 上的 JSON-RPC 进行。使用这种模式的有：

- IDE 扩展（Zed、Neovim、Emacs 等）
- 自定义自动化工具
- ACP 客户端库

### 选项

| Flag                  | Description                                                                         |
| --------------------- | ----------------------------------------------------------------------------------- |
| `-m, --model <MODEL>` | 覆盖默认模型 ID（例如 `grok-build`）                           |
| `--always-approve`    | 以始终批准模式启动（无需确认即自动批准所有工具执行） |
| `--reauth`            | 强制走重新认证流程                                                        |

<details>
<summary><strong>Advanced: WebSocket Relay</strong></summary>

要把代理暴露到公网（而不是局域网）上，可以运行一个 WebSocket 中继服务器，让代理连上去：

```bash
chaos agent headless --grok-ws-url wss://your-relay.example.com/ws
```

代理向外连接到你的中继，你的网页客户端也连到同一个中继。用于构建浏览器无法拉起本地进程的 Web UI。

</details>

---

## SSH 透传（`chaos ssh`）

在那些对 OSC 52 本地剪贴板拦截缺乏原生支持的终端（例如 Apple Terminal）里连接远端主机时，用 `chaos ssh` 代替普通的 `ssh`。

```bash
# Basic usage (same args as ssh)
chaos ssh user@host

# With SSH flags
chaos ssh -t user@host
chaos ssh -L 8080:localhost:8080 user@host

# With remote command
chaos ssh user@host -- tmux attach
```

在 macOS 上，如果终端不原生处理 OSC 52，`chaos ssh` 会在一个本地 PTY 里运行 SSH，由该 PTY 拦截剪贴板序列并写入 `pbcopy`。普通的 OSC 52 与 tmux DCS 透传都会被处理。原生支持 OSC 52 的终端（iTerm2、Ghostty、Kitty、WezTerm、Alacritty）则直接以普通 `ssh` 执行，不加包装。

这一切都在本地完成。

---

## 用 Chaos 构建

Chaos 可以当作 OpenAI 兼容的 chat completion 后端来用。两种集成方式二选一：

| Mode         | Use Case                                                           |
| ------------ | ------------------------------------------------------------------ |
| **Headless** | 简单的 chat API、脚本、自动化、OpenAI SDK 直接替换           |
| **ACP SDK**  | IDE 集成、工具可见性、思考流、权限 UI |

---

### 无头模式（简单的 chat completion）

简单集成用无头模式。它会拉起 `chaos -p` 并解析 JSON 输出。

#### Python —— 无头模式

```python
import asyncio
import json
import os

class GrokChat:
    """Simple OpenAI-compatible wrapper using headless mode."""

    def __init__(self, cwd="."):
        self.cwd = cwd
        self.env = {**os.environ}

    def _build_cmd(self, prompt, model, stream):
        return ["chaos", "-p", prompt, "-m", model, "--cwd", self.cwd,
                "--output-format", "streaming-json" if stream else "json", "--always-approve"]

    async def create(self, messages, model="grok-build", stream=False):
        prompt = messages[-1]["content"] if len(messages) == 1 else "\n".join(
            f"{m['role']}: {m['content']}" for m in messages
        )
        cmd = self._build_cmd(prompt, model, stream)

        if stream:
            return self._stream(cmd)

        proc = await asyncio.create_subprocess_exec(
            *cmd, env=self.env, stdout=asyncio.subprocess.PIPE
        )
        stdout, _ = await proc.communicate()
        data = json.loads(stdout.decode()) if stdout else {"text": ""}
        return {
            "choices": [{
                "message": {"role": "assistant", "content": data.get("text", "")},
                "finish_reason": "stop"
            }]
        }

    async def _stream(self, cmd):
        proc = await asyncio.create_subprocess_exec(
            *cmd, env=self.env, stdout=asyncio.subprocess.PIPE
        )
        async for line in proc.stdout:
            if not line.strip():
                continue
            event = json.loads(line)
            if event.get("type") == "text":
                yield {"choices": [{"delta": {"content": event["data"]}}]}
            elif event.get("type") == "end":
                yield {"choices": [{"delta": {}, "finish_reason": "stop"}]}


# Usage
async def main():
    client = GrokChat(cwd=".")

    # Non-streaming
    response = await client.create([{"role": "user", "content": "What files are here?"}])
    print(response["choices"][0]["message"]["content"])

    # Streaming
    async for chunk in await client.create(
        [{"role": "user", "content": "List files"}], stream=True
    ):
        print(chunk["choices"][0]["delta"].get("content", ""), end="", flush=True)

asyncio.run(main())
```

#### TypeScript —— 无头模式

```typescript
import { execa } from "execa";

class GrokChat {
  constructor(private cwd = ".") {}

  private buildArgs(prompt: string, model: string, stream: boolean) {
    return [
      "-p",
      prompt,
      "-m",
      model,
      "--cwd",
      this.cwd,
      "--output-format",
      stream ? "streaming-json" : "json",
      "--always-approve",
    ];
  }

  async create(
    messages: { role: string; content: string }[],
    { model = "grok-build", stream = false } = {},
  ) {
    const prompt =
      messages.length === 1
        ? messages[0].content
        : messages.map((m) => `${m.role}: ${m.content}`).join("\n");

    if (stream) return this.streamResponse(prompt, model);

    const { stdout } = await execa(
      "chaos",
      this.buildArgs(prompt, model, false),
    );
    const data = JSON.parse(stdout || '{"text":""}');
    return {
      choices: [
        {
          message: { role: "assistant", content: data.text || "" },
          finish_reason: "stop",
        },
      ],
    };
  }

  async *streamResponse(prompt: string, model: string) {
    const proc = execa("chaos", this.buildArgs(prompt, model, true));
    for await (const chunk of proc.stdout!) {
      for (const line of chunk.toString().split("\n").filter(Boolean)) {
        const event = JSON.parse(line);
        if (event.type === "text") {
          yield { choices: [{ delta: { content: event.data } }] };
        } else if (event.type === "end") {
          yield { choices: [{ delta: {}, finish_reason: "stop" }] };
        }
      }
    }
  }
}

// Usage
const client = new GrokChat(".");

// Non-streaming
const response = await client.create([
  { role: "user", content: "What files are here?" },
]);
console.log(response.choices[0].message.content);

// Streaming
for await (const chunk of await client.create(
  [{ role: "user", content: "List files" }],
  { stream: true },
)) {
  process.stdout.write(chunk.choices[0].delta?.content || "");
}
```

---

### ACP SDK（丰富的代理集成）

使用 Agent Client Protocol 可以完整访问工具调用、思考、计划与权限。

#### Python —— ACP SDK

```python
import asyncio
import json

class GrokACPChat:
    """Rich OpenAI-compatible wrapper using ACP protocol."""

    def __init__(self, cwd="."):
        self.cwd = cwd
        self.proc = None
        self.session_id = None

    async def init(self):
        self.proc = await asyncio.create_subprocess_exec(
            "chaos", "agent", "stdio",
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE
        )

        # Initialize
        await self._request("initialize", {
            "protocolVersion": "1",
            "clientCapabilities": {
                "fs": {"readTextFile": True, "writeTextFile": True},
                "terminal": True
            }
        })

        # Create session
        result = await self._request("session/new", {
            "cwd": self.cwd,
            "mcpServers": []
        })
        self.session_id = result["sessionId"]
        return self

    async def _request(self, method, params):
        msg = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
        self.proc.stdin.write(f"{msg}\n".encode())
        await self.proc.stdin.drain()

        line = await self.proc.stdout.readline()
        return json.loads(line).get("result", {})

    async def create(self, messages, model="grok-build", stream=False):
        prompt = [{"type": "text", "text": m["content"]} for m in messages]

        # For streaming, yield chunks as they arrive
        if stream:
            return self._stream(prompt)

        result = await self._request("session/prompt", {
            "sessionId": self.session_id,
            "prompt": prompt
        })
        return {
            "choices": [{
                "message": {"role": "assistant", "content": result.get("text", "")},
                "finish_reason": result.get("stopReason", "stop").lower()
            }]
        }

    async def _stream(self, prompt):
        # Send prompt request
        msg = json.dumps({
            "jsonrpc": "2.0", "id": 1,
            "method": "session/prompt",
            "params": {"sessionId": self.session_id, "prompt": prompt}
        })
        self.proc.stdin.write(f"{msg}\n".encode())
        await self.proc.stdin.drain()

        # Read streaming updates
        while True:
            line = await self.proc.stdout.readline()
            if not line:
                break

            data = json.loads(line)

            # Handle notifications
            if data.get("method") == "session/update":
                update = data["params"]["update"]
                session_update = update.get("sessionUpdate")

                if session_update == "agent_message_chunk":
                    yield {"choices": [{"delta": {"content": update["content"]["text"]}}]}
                elif session_update == "agent_thought_chunk":
                    yield {"choices": [{"delta": {"thought": update["content"]["text"]}}]}
                elif session_update == "tool_call":
                    yield {"choices": [{"delta": {"tool_call": {
                        "name": update["tool"],
                        "status": "pending"
                    }}}]}
                elif session_update == "plan":
                    yield {"choices": [{"delta": {"plan": update["entries"]}}]}

            # Handle final response
            elif "result" in data:
                yield {"choices": [{"delta": {}, "finish_reason": "stop"}]}
                break


# Usage
async def main():
    client = await GrokACPChat(cwd=".").init()

    # Streaming with rich updates
    async for chunk in await client.create(
        [{"role": "user", "content": "Refactor the main function"}],
        stream=True
    ):
        delta = chunk["choices"][0]["delta"]
        if "content" in delta:
            print(delta["content"], end="", flush=True)
        if "thought" in delta:
            print(f"\n[Thinking: {delta['thought']}]", end="")
        if "tool_call" in delta:
            print(f"\n[Tool: {delta['tool_call']}]")
        if "plan" in delta:
            print(f"\n[Plan: {delta['plan']}]")

asyncio.run(main())
```

#### TypeScript —— ACP SDK

```typescript
import { spawn, ChildProcess } from "child_process";
import * as readline from "readline";

class GrokACPChat {
  private proc!: ChildProcess;
  private sessionId!: string;
  private rl!: readline.Interface;

  constructor(private cwd = ".") {}

  async init() {
    this.proc = spawn("chaos", ["agent", "stdio"]);
    this.rl = readline.createInterface({ input: this.proc.stdout! });

    // Initialize
    await this.request("initialize", {
      protocolVersion: "1",
      clientCapabilities: {
        fs: { readTextFile: true, writeTextFile: true },
        terminal: true,
      },
    });

    // Create session
    const { sessionId } = await this.request("session/new", {
      cwd: this.cwd,
      mcpServers: [],
    });
    this.sessionId = sessionId;
    return this;
  }

  private async request(method: string, params: any): Promise<any> {
    return new Promise((resolve) => {
      const msg = JSON.stringify({ jsonrpc: "2.0", id: 1, method, params });
      this.proc.stdin!.write(msg + "\n");

      this.rl.once("line", (line) => {
        resolve(JSON.parse(line).result || {});
      });
    });
  }

  async create(
    messages: { role: string; content: string }[],
    { model = "grok-build", stream = false } = {},
  ) {
    const prompt = messages.map((m) => ({ type: "text", text: m.content }));

    if (stream) return this.streamResponse(prompt);

    const result = await this.request("session/prompt", {
      sessionId: this.sessionId,
      prompt,
    });

    return {
      choices: [
        {
          message: { role: "assistant", content: result.text || "" },
          finish_reason: result.stopReason?.toLowerCase() || "stop",
        },
      ],
    };
  }

  async *streamResponse(prompt: { type: string; text: string }[]) {
    const msg = JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      method: "session/prompt",
      params: { sessionId: this.sessionId, prompt },
    });
    this.proc.stdin!.write(msg + "\n");

    for await (const line of this.rl) {
      const data = JSON.parse(line);

      if (data.method === "session/update") {
        const update = data.params.update;
        switch (update.sessionUpdate) {
          case "agent_message_chunk":
            yield { choices: [{ delta: { content: update.content?.text } }] };
            break;
          case "agent_thought_chunk":
            yield { choices: [{ delta: { thought: update.content?.text } }] };
            break;
          case "tool_call":
            yield {
              choices: [
                {
                  delta: {
                    tool_call: {
                      name: update.tool,
                      args: update.arguments,
                      status: "pending",
                    },
                  },
                },
              ],
            };
            break;
          case "plan":
            yield { choices: [{ delta: { plan: update.entries } }] };
            break;
        }
      } else if (data.result) {
        yield { choices: [{ delta: {}, finish_reason: "stop" }] };
        break;
      }
    }
  }
}

// Usage
const client = await new GrokACPChat(".").init();

// Streaming with rich updates
for await (const chunk of await client.create(
  [{ role: "user", content: "Refactor main" }],
  { stream: true },
)) {
  const delta = chunk.choices[0].delta;
  if (delta.content) process.stdout.write(delta.content);
  if (delta.thought) console.log(`\n[Thinking: ${delta.thought}]`);
  if (delta.tool_call)
    console.log(`\n[Tool: ${JSON.stringify(delta.tool_call)}]`);
  if (delta.plan) console.log(`\n[Plan: ${JSON.stringify(delta.plan)}]`);
}
```

---

### ACP 协议参考

Chaos 实现了 [Agent Client Protocol (ACP)](https://agentclientprotocol.com)，一个 AI 代理通信标准。

#### 架构

```
┌─────────────────────────────────────────┐
│           ACP Client                    │
│  (IDE, Editor, Custom Application)      │
└──────────────────┬──────────────────────┘
                   │ JSON-RPC over stdio
┌──────────────────▼──────────────────────┐
│           chaos agent stdio             │
│                                         │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  │
│  │ Session │  │  Tools  │  │   MCP   │  │
│  │ Manager │  │ Registry│  │ Servers │  │
│  └─────────┘  └─────────┘  └─────────┘  │
└─────────────────────────────────────────┘
```

#### SDK

| Language   | Package                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------- |
| TypeScript | [`@agentclientprotocol/sdk`](https://www.npmjs.com/package/@agentclientprotocol/sdk)     |
| Rust       | [`agent-client-protocol`](https://crates.io/crates/agent-client-protocol)                |
| Python     | [`agent-client-protocol-python`](https://github.com/PsiACE/agent-client-protocol-python) |
| Go         | [`acp-go-sdk`](https://github.com/coder/acp-go-sdk)                                      |
| Kotlin     | [`acp`](https://github.com/agentclientprotocol/kotlin-sdk)                               |

#### 资源

- [ACP Specification](https://agentclientprotocol.com/protocol/prompt-turn)
- [Protocol Introduction](https://agentclientprotocol.com/overview/introduction)

#### 兼容的客户端

| Client                                                   | Status      |
| -------------------------------------------------------- | ----------- |
| [Zed](https://zed.dev/docs/ai/external-agents)           | ✓ 已支持 |
| [Neovim](https://neovim.io) (CodeCompanion, avante.nvim) | ✓ 已支持 |
| [Emacs](https://github.com/xenodium/agent-shell)         | ✓ 已支持 |
| [marimo notebook](https://github.com/marimo-team/marimo) | ✓ 已支持 |
| JetBrains                                                | 即将支持 |

---

## 配置

Chaos 从 `~/.chaos/config.toml` 读取配置。如果该文件不存在，Chaos 会使用合理的默认值。你只需要写出自己想要覆盖的项。

> 配置根按 `$CHAOS_HOME` → `$GROK_HOME` → 已有 `~/.chaos` → 已有 `~/.grok` → 默认 `~/.chaos` 的顺序解析；旧用户可继续使用 `~/.grok/config.toml`（兼容读取）。项目级同样双读 `.chaos/` 与 `.grok/`，同名时 Chaos 优先。权威表述见仓库根目录的 `CHAOS.md`。

下面每个功能小节都记录了自己的配置。本节讲的是那些没有独立顶层小节的通用设置。

### 通用设置

```toml
[cli]
auto_update = true                     # check for updates on launch

[models]
default = "grok-4.6"                   # model used for new sessions
web_search = "grok-4.6"                # model used by the web_search tool

[ui]
max_thoughts_width = 120               # max column width for reasoning display

[features]
support_permission = false             # prompt before tool execution
telemetry = false                      # anonymous usage telemetry (env: GROK_TELEMETRY_ENABLED)
feedback = false                       # feedback system (env: GROK_FEEDBACK_ENABLED)
lsp_tools = false                      # expose the lsp tool (see LSP Servers below)
codebase_indexing = true               # code graph indexing (true, false, or glob patterns)

[session]
auto_compact_threshold_percent = 85    # auto-compact at this % of context window
load_envrc = true                      # load .envrc environment variables into bash commands

[tools]
respect_gitignore = true               # filter gitignored files from tools (env: GROK_RESPECT_GITIGNORE)

[toolset.bash]
timeout_secs = 120.0                   # command timeout in seconds
output_byte_limit = 65536              # max output size (64KB)

[toolset.web_fetch]
proxy_endpoint = "https://proxy.example.com"   # egress proxy URL (all requests routed through it)
allowed_domains = ["docs.rs", "x.ai"]           # override the built-in ~84-domain allowlist
```

### 遥测

配置遥测目标与凭据。空值会禁用对应的接收端。环境变量优先于配置值。从公开源码树构建的版本不带任何遥测默认值：`events_url`、`events_api_key` 和 `mixpanel_token` 都是未设置的，`mixpanel_enabled` 为 `false`，所以除非你在这里或通过环境变量提供值，否则什么都不会发送。

```toml
[telemetry]
events_url = "https://example.com/events"  # env: GROK_TELEMETRY_EVENTS_URL
events_api_key = "..."                      # env: GROK_TELEMETRY_EVENTS_API_KEY
mixpanel_token = "..."                      # env: GROK_TELEMETRY_MIXPANEL_TOKEN
mixpanel_enabled = true                     # env: GROK_TELEMETRY_MIXPANEL_ENABLED
trace_upload = true                         # env: GROK_TELEMETRY_TRACE_UPLOAD
```

从源码构建时，也可以在构建环境里设置 `GROK_TELEMETRY_BUILD_EVENTS_URL`、`GROK_TELEMETRY_BUILD_EVENTS_API_KEY` 和 `GROK_TELEMETRY_BUILD_MIXPANEL_TOKEN`，把默认值在编译期烙进二进制（用这种方式提供 Mixpanel token 也会默认启用 Mixpanel）。配置文件与运行时的环境变量值会覆盖构建期的默认值。

### LSP 服务器

Chaos 可以连接到配置在 JSON 文件里的 Language Server Protocol (LSP) 服务器。LSP 集成让 Chaos 在你的仓库里工作时具备语言感知的代码智能。

LSP 支持有两种用法：

- **被动诊断** —— 编辑之后，Chaos 可以呈现语言服务器给出的错误、警告等诊断信息。
- **`lsp` 工具** —— Chaos 可以主动向语言服务器查询 `goToDefinition`、`findReferences`、`hover`、`goToImplementation`、`documentSymbol` 和 `workspaceSymbol`。

参考：[Language Server Protocol](https://microsoft.github.io/language-server-protocol/)

#### 配置位置

Chaos 在下面这些位置查找服务器定义：

- 项目配置：`<repo>/.chaos/lsp.json`
- 用户配置：`~/.chaos/lsp.json`

如果同一个服务器名在两处都出现，项目配置胜出。

#### 工具启用

只要有一个 `lsp.json` 文件，被动诊断就能工作。模型可见的 `lsp` 工具会在下面两条都成立时暴露出来：

- LSP 工具已启用（`GROK_LSP_TOOLS=1` 或 `[features] lsp_tools = true`）
- 合并后的 LSP 配置非空

为单次运行启用该工具：

```bash
GROK_LSP_TOOLS=1 chaos
```

或者在配置里启用：

```toml
[features]
lsp_tools = true
```

如果启用了 LSP 工具但找不到可用的服务器配置，Chaos 会在日志里给出非致命警告，然后不带 `lsp` 工具继续运行。如果配置存在但每个服务器都启动失败，该工具可能仍然存在，并在首次使用时以启动错误失败。

#### 示例 `lsp.json`

```json
{
  "typescript": {
    "command": "typescript-language-server",
    "args": ["--stdio"],
    "extensionToLanguage": {
      ".ts": "typescript",
      ".tsx": "typescriptreact"
    },
    "startupTimeout": 30000
  }
}
```

#### 必需字段

| 字段 | 说明 |
|-------|-------------|
| `command` | 要执行的服务器二进制。对 `stdio` 而言，它必须在 `PATH` 里，或是一个绝对路径。 |
| `extensionToLanguage` | 把文件扩展名映射到 LSP 语言 ID。 |

#### 可选字段

| 字段 | 说明 |
|-------|-------------|
| `args` | 传给服务器进程的命令行参数。 |
| `transport` | `stdio`（默认）或 `socket`。 |
| `env` | 服务器进程的额外环境变量。 |
| `initializationOptions` | LSP initialize 期间传入的 JSON。 |
| `settings` | 通过 workspace settings 更新发送的配置。 |
| `workspaceFolder` | 覆盖发给服务器的工作区目录路径。 |
| `workspaceOpen` | 要加载的解决方案或项目，适用于必须被明确告知的服务器（见下）。 |
| `startupTimeout` | 启动等待上限（毫秒），超过即视为启动失败。 |
| `shutdownTimeout` | 优雅关闭等待上限（毫秒）。 |
| `restartOnCrash` | 崩溃后是否重启该服务器。 |
| `maxRestarts` | 放弃前的最大重启尝试次数。 |

#### 告诉服务器加载哪个解决方案（`workspaceOpen`）

大多数服务器会自己从工作区目录推断要分析什么。少数服务器不行，
它们通过协议扩展来加载自己的工作区。C# 服务器
（`Microsoft.CodeAnalysis.LanguageServer`，"Roslyn"）是最典型的一个：单独跑时它
把每个文件都当作散落的 "miscellaneous file"，完全不报项目级诊断，
直到被告知要打开某个解决方案或一组项目。

```json
{
  "csharp": {
    "command": "dotnet",
    "args": [
      "/path/to/Microsoft.CodeAnalysis.LanguageServer.dll",
      "--stdio",
      "--logLevel", "Warning",
      "--extensionLogDirectory", "/tmp/roslyn-logs"
    ],
    "extensionToLanguage": { ".cs": "csharp" },
    "workspaceOpen": { "solution": "MyApp.sln" },
    "startupTimeout": 60000
  }
}
```

没有解决方案文件时，用 `"projects": ["src/App/App.csproj", "src/Lib/Lib.csproj"]`
代替 `"solution"`。路径可以是绝对路径，也可以是相对于工作区根的路径。
像 `roslyn-language-server` 这类包装器自己就会发送这些通知，
那种情况下可以省略 `workspaceOpen`。

注意该服务器要求必须给出 `--logLevel`，而且任何比 `Warning` 更啰嗦的级别
都会让它把每一条内部日志行都流式推给客户端。

#### 安装语言服务器

Chaos 不捆绑语言服务器二进制。你需要自己安装服务器，并确认配置的 `command` 在你的机器上能跑起来。

示例：

| Language | Server | Install example |
|----------|--------|-----------------|
| TypeScript | `typescript-language-server` | `npm install -g typescript-language-server typescript` |
| Python | `pyright` | `npm install -g pyright` 或 `pip install pyright` |
| Rust | `rust-analyzer` | 按你所使用平台推荐的方式安装 `rust-analyzer` |

#### 说明

- 被动诊断**不**需要 `GROK_LSP_TOOLS=1`；只要配置了适用的服务器并成功启动，它就会运行。
- 被动诊断目前由 `search_replace` 的编辑驱动；它们不是针对工作区中任意 shell 或 git 变更的通用监视器。
- 当 `lsp` 工具被禁用或未配置时，它会被刻意隐藏，以免模型围绕不可用的能力做规划。
- 同一工作区内的子代理会复用父会话已有的 LSP 运行时，而不是再起一套重复的服务器池。
- 正因为复用，子代理会继承父会话在该共享工作区上的 LSP 服务器集合；在复用运行时的路径下，子代理本地的 LSP 配置差异不会被加载。

### 企业部署

一份完整的企业部署 `config.toml` 示例：带外部认证、企业代理，并关闭遥测：

```toml
[cli]
auto_update = false

[auth]
auth_provider_command = "/usr/local/bin/my-company-auth-provider"
auth_provider_label = "Acme Corp"
auth_token_ttl = 3600               # if your provider outputs bare tokens

[models]
default = "company-grok"

[model.company-grok]
model = "grok-build"
base_url = "https://grok-proxy.acme.com/"
name = "Grok Build Latest (Proxy)"
context_window = 256000

[features]
support_permission = false
telemetry = false

[toolset.bash]
timeout_secs = 120.0
```

> **注意：** 上面 `[auth]` 段里的外部认证 Provider 铸造的是 xAI 会话 token，需要 xAI 账号，而 Chaos 不登录 xAI 账号，所以这一段在 Chaos 里不适用。要在 Chaos 里接入企业网关，请改用每个模型的 `base_url` 配上 `env_key`/`api_key`，见上文「自定义模型」一节。
>
> 上游行为：有了这份配置，`grok` 会运行你的认证二进制、保存 token，并把推理路由经过你的企业代理。完整的认证设置细节见[认证](#authentication)。

---

## AGENTS.md

创建一个代理规则文件（例如 `AGENTS.md`）就能加入项目专属的指令。Chaos 会读取这些文件，并把它们的内容追加到系统提示中。

Chaos 按这个顺序扫描代理规则：

1. `~/.chaos/`（全局规则）
2. 如果位于 git 仓库内：从仓库根目录 → 当前工作目录（含两端）的每一级目录
3. 如果**不**在 git 仓库内：只看当前工作目录

在每一级目录里，Chaos 会检查这些文件名：

- `Agents.md`、`Claude.md`、`AGENT.md`、`AGENTS.md`

顺序很重要：越晚被找到的文件（更深的目录）排在越后，因此当指令冲突时，它们实际上更有优先权。被 gitignore 忽略的文件会被跳过。每个文件上限 10,000 字符（超出会截断并给出警告）。

> **注意：** `--rules` 标志会在发现的所有代理文件之上再追加 _额外的_ 规则，所以你可以把两者结合起来做会话级定制。

---

## 技能

技能是可复用的提示包，用专门的工作流、领域知识和工具集成来扩展 Chaos。可以用它们把那些每次会话都得重新解释一遍的可重复流程固化下来。

### 技能位置

Chaos 从下面这些目录发现技能（按优先级排列）：

| Location                    | Scope | Priority |
| --------------------------- | ----- | -------- |
| `./.chaos/skills/`           | Local | Highest  |
| `<repo_root>/.chaos/skills/` | Repo  | Medium   |
| `~/.chaos/skills/`           | User  | Lowest   |
| `~/.claude/skills/`         | User  | Lowest   |

同名技能会去重——高优先级位置覆盖低优先级位置。

仓库范围的技能（Local 与 Repo）遵循 `.gitignore`，被忽略的会被过滤掉。用户范围的技能（`~/.chaos/skills/`）在仓库之外，永远不会被过滤。

### 配置

通过 config.toml 里的 `[skills]` 追加技能目录或排除路径：

```toml
[skills]
paths = ["~/my-team-skills"]          # additional directories to scan
ignore = ["~/my-team-skills/wip"]     # paths to exclude
```

### 创建技能

每个技能都放在自己的目录里，包含一个 `SKILL.md` 文件：

```
~/.chaos/skills/
└── commit/
    └── SKILL.md
```

**SKILL.md 的格式：**

```markdown
---
name: commit
description: Create well-formatted git commits following conventional commit standards. Use when the user wants to commit changes or asks for /commit.
---

# Git Commit Skill

Review staged changes and create a commit with a clear, conventional message.

## Steps

1. Run `git diff --staged` to see changes
2. Summarize what changed and why
3. Create commit message following conventional commits format
4. Run `git commit -m "..."` with the message
```

**必需的 frontmatter 字段：**

| Field         | Description                                                                  |
| ------------- | ---------------------------------------------------------------------------- |
| `name`        | 技能标识符（小写、连字符、最长 64 个字符）                          |
| `description` | 该技能做什么、何时使用——Chaos 就是据此决定是否调用它 |

### 使用技能

**在 TUI 里：**

```bash
/skills              # List available skills
/skills commit       # Inject the "commit" skill into context
```

**模型也可以在识别到相关任务时自动调用技能。** 何时触发由技能的 `description` 字段决定。

**斜杠命令简写：**

用户可以把技能写作 `/skill-name`（例如 `/commit`）。看到这种写法时，Chaos 会调用对应的技能。

> **提示：** `description` 字段至关重要——它决定了 Chaos 何时自动调用该技能。请把触发短语和适用场景写具体。

---

## 代理配置

代理配置控制会话的系统提示、工具集与行为。一个代理配置是一份带 YAML frontmatter 的 `.md` 文件，也可以是从磁盘上发现的具名代理。

Chaos 从 `.chaos/agents/`（项目）、`~/.chaos/agents/`（用户）以及内置代理中发现代理定义。优先级（高者胜出）：

1. `--agent-profile <PATH>` 命令行标志
2. `config.toml` 里的 `[agent]` 段
3. `GROK_AGENT` 环境变量
4. 默认的 `grok-build` 代理

```toml
# ~/.chaos/config.toml
[agent]
name = "my-custom-agent"             # Discovered by name
# definition = "/path/to/agent.md"   # OR: explicit path
```

```bash
chaos --agent-profile ./my-agent.md
# or
export GROK_AGENT="my-custom-agent"
```

---

## 子代理

子代理会派生出独立的子会话并行处理任务。每个子会话有自己的上下文窗口，也可以选择继承父会话的对话历史。默认启用。

### 禁用

```bash
export GROK_SUBAGENTS=0              # Environment variable
```

```toml
# ~/.chaos/config.toml
[subagents]
enabled = false
```

### 开关与模型覆盖

可以在保持整套机制启用的前提下禁用特定子代理类型，或把它们路由到不同的模型：

```toml
[subagents.toggle]
explore = true                       # default — omitted agents are enabled
plan = false                         # disable plan subagent

[subagents.models]
explore = "grok-build"              # route explore to a lighter model
```

默认情况下子代理继承父会话的模型。只有显式的按代理指定才会覆盖它：
`[subagents.models].<agent>`（优先级最高），其次是代理定义里的 `model`。
这两种指定都无条件生效，无论父会话当下用的是哪个模型。

### 角色与人格

角色（role）定义可复用的能力/模型默认值。人格（persona）在子代理的提示上叠加语气与行为指令。

```toml
[subagents.roles.researcher]
description = "Deep research agent"
default_capability_mode = "read-only"
model = "grok-build"
prompt_file = ".chaos/prompts/researcher.md"

[subagents.personas.concise]
instructions = "Be extremely concise. No filler words."
# instructions_file = ".chaos/personas/concise.md"  # or load from file
```

两者也分别从 `.chaos/roles/*.toml` 和 `.chaos/personas/*.toml` 文件里发现。如果请求的人格找不到，派生会失败（fail-closed）。

---

## 插件

插件用来自外部包的工具、技能和 MCP 服务器扩展 Chaos。

### 插件位置

| Location                    | Scope   |
| --------------------------- | ------- |
| `.chaos/plugins/`            | Project |
| `~/.chaos/plugins/`          | User    |
| `--plugin-dir <PATH>` (CLI) | Session |

### 配置

```toml
# ~/.chaos/config.toml
[plugins]
paths = ["~/my-plugins/custom-tools"]       # additional plugin directories
disabled = ["user/a1b2c3d4/noisy-plugin"]   # plugin IDs to skip
```

运行时用 `/plugins list`、`/plugins reload` 或 `/plugins trust <path>` 管理插件。

---

## 钩子

钩子会在工具与会话的生命周期事件上（工具使用前后、会话开始/结束）运行项目脚本。项目必须先被显式信任，其钩子才会执行。

Chaos 从项目目录下的 `.chaos/hooks/` 中发现钩子。用下面这些命令管理它们：

```
/hooks-list              # show hooks loaded in this session
/hooks-trust             # trust this project for hook execution
/hooks-add <path>        # add a custom hook file or directory
```

### 配置文件里的钩子

钩子也可以直接定义在各配置层里，这样就能随其他配置一起分发，而不必写成单独的 JSON 文件。在 `config.toml`（你自己的那份）、`managed_config.toml` 或
`requirements.toml` 里加一张 `[[hooks.<Event>]]` 表：

```toml
[[hooks.PreToolUse]]
matcher = "Bash|Write|Edit"
  [[hooks.PreToolUse.hooks]]
  type = "command"
  command = "/opt/guard/pretooluse.sh"   # use an absolute path
  timeout = 10
```

其 schema 与钩子文件里使用的 JSON `hooks` 对象一致。钩子会从每一层读取，并以叠加的方式合并：低优先级的层可以新增钩子，但永远不能删除或替换另一层的块。每个钩子在 `/hooks-list` 里的名字都会带上它来自的层作为前缀（例如 `managed:` 或
`requirements/user:`）。

来自**由 root 拥有的**那些层的钩子（系统目录里的 `requirements.toml`，例如
`/etc/grok/requirements.toml`，或 `/etc/grok/managed_config.toml`）是被强制实施的：
无法从钩子面板、启用/禁用 API 或 `disabled-hooks` 文件里禁用它们，低优先级层里一份逐字节相同的副本也不能夺走它们的来源归属。这种强制依赖操作系统的文件属主——请把这些文件以 root 属主部署（或通过 MDM 分发）；没有签名校验。`$CHAOS_HOME` 各层（`requirements.toml`、`managed_config.toml`、`config.toml`）里的钩子仍然只是便利分发，不是强制边界：那个目录归用户所有，用户可以编辑或改指别处。

---

## 自定义模型

添加自定义模型端点，以使用其他 Provider 或自托管模型。你也可以用自定义设置覆盖内置模型。

### 模型配置

TOML 表头里的名字（`[model.my-model]` 中的 `my-model`）就是模型选择器里显示的名字。`model` 字段是发给 API 的标识符。如果省略 `model`，表头名会直接发给 API。

```toml
[model.my-model]
model = "model-id"                    # Model identifier sent to API
base_url = "https://api.example.com/v1"  # OpenAI-compatible endpoint
name = "Display Name"                 # Shown in model picker
description = "Model description"     # Optional description
api_key = "sk-..."                    # API key for this provider (optional)
env_key = "OPENAI_API_KEY"            # Env var(s) holding the API key (string or array; first set wins)
auth_provider = "corp-gateway"        # Named credential helper for rotating tokens (optional)
temperature = 0.7                     # Sampling temperature (0.0-2.0)
top_p = 0.95                          # Nucleus sampling parameter
max_completion_tokens = 8192          # Max tokens per response
context_window = 256000               # Total context window in tokens (for auto-compact)
```

**凭据解析顺序：** `api_key` → `env_key` → 缓存的 `auth_provider` token（终态：缓存未命中就解析为没有凭据，绝不会用会话 token）→ 会话 token → `XAI_API_KEY`。见[每个模型的 Auth Provider](#per-model-auth-providers)。

`context_window` 参数用于计算自动压缩何时触发。没有指定时，Chaos 对已知模型回退到内置默认值。

### 行内 `<think>` 推理

有些 OpenAI 兼容的推理模型会把思考文本放在 `content` 里以 `<think>...</think>` 的形式返回，而不是用 `reasoning_content`。开启按模型提取，就能把这些片段引导到可折叠的推理通道：

```toml
[model.deepseek-r1]
model = "deepseek-reasoner"
base_url = "https://api.example.com/v1"
api_backend = "chat_completions"
context_window = 128000
extract_inline_thinking = true
```

该选项默认关闭，且只影响该模型的 Chat Completions。标签可能跨越流式的多个分片；如果某个 `<think>` 块被截断而没有 `</think>`，它余下的文本仍然算推理内容。为与远端模型元数据保持一致，`extractInlineThinking` 也被接受，不过在 `config.toml` 里以蛇形命名为准。

### 国内 Provider

pager 的 `/provider add` 流程为常见的国产 Provider（DeepSeek、Qwen、智谱、Moonshot、火山方舟）内置了预设——在那里选一个就能省去手工配置。如果你更愿意直接编辑 `config.toml`，下面这些就是标准写法：

```toml
# DeepSeek (deepseek-reasoner / deepseek-chat)
[model_providers.deepseek]
base_url = "https://api.deepseek.com/v1"
api_backend = "chat_completions"
env_key = "DEEPSEEK_API_KEY"

[model.deepseek-r1]
model = "deepseek-reasoner"
model_provider = "deepseek"
context_window = 64000
extract_inline_thinking = true   # deepseek-reasoner emits <think> in content
```

```toml
# Qwen / 通义千问 (DashScope compatible-mode)
[model_providers.qwen]
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
api_backend = "chat_completions"
env_key = "DASHSCOPE_API_KEY"

[model.qwen3]
model = "qwen3-max"
model_provider = "qwen"
context_window = 32768
# qwen3-thinking already streams a structured `reasoning_content`; do NOT
# also set extract_inline_thinking=true, or duplicate/conflicting reasoning
# is emitted.
```

```toml
# Zhipu / 智谱 GLM
[model_providers.zhipu]
base_url = "https://open.bigmodel.cn/api/paas/v4"
api_backend = "chat_completions"
env_key = "ZHIPUAI_API_KEY"

[model.glm4]
model = "glm-4"
model_provider = "zhipu"
context_window = 128000
```

```toml
# Moonshot / 月之暗面 (Kimi)
[model_providers.moonshot]
base_url = "https://api.moonshot.cn/v1"
api_backend = "chat_completions"
env_key = "MOONSHOT_API_KEY"

[model.kimi]
model = "moonshot-v1-8k"
model_provider = "moonshot"
context_window = 8192
```

```toml
# Volcengine / 火山方舟 (Doubao)
[model_providers.volcengine]
base_url = "https://ark.cn-beijing.volces.com/api/v3"
api_backend = "chat_completions"
env_key = "ARK_API_KEY"

[model.doubao]
model = "doubao-1-5-pro-32k"
model_provider = "volcengine"
context_window = 32768
```

> 国产 Provider 的错误被划分为与重试相关的几类：key 写错或过期（`Auth`）以及余额/额度用尽（`Billing`）**不会**重试——它们立即暴露出来，让你去充值或检查凭据，而不是看起来像卡住了。限流和瞬时的服务端故障会以有上限的退避重试。

### 覆盖内置模型

你可以只覆盖内置模型的某些字段，而不必把整个定义重写一遍。只写你想改的字段：

```toml
# Override just the API key for a default model
[model.grok-build]
api_key = "my-api-key"

# Override temperature and add a custom API key
[model.grok-4.20-0309-reasoning]
temperature = 0.5
api_key = "sk-custom"
```

**工作原理：** 当你覆盖某个内置模型时，Chaos 先取默认配置（包括由你的 `[endpoints]` 设置得到的正确 `base_url`），然后只应用你写明的字段。未指定的字段从默认值继承。

**优先级顺序：**
1. 你的配置（`[model.*]`）—— 优先级最高
2. 从远端 `/v1/models` 预取的模型
3. 硬编码默认值 —— 优先级最低

**web search 用的模型：** 设置 `[models] web_search`、`GROK_WEB_SEARCH_MODEL` 或 `--web-search-model`，把 `web_search` 工具指向另一个模型。目标端点必须支持 Responses API 和 web search。

> **用自定义模型做覆盖：** 如果该模型还不在目录里（内置默认或
> `chaos models` 的输出中都没有），只设置 `[models] web_search` 是
> 不够的。你还需要一条 `[model.*]` 条目，Chaos 才知道怎么访问它。
> 两者缺一，web search 会被静默禁用。
>
> ```toml
> [models]
> web_search = "my-custom-model"       # 1. tell web search which model to use
>
> [model.my-custom-model]              # 2. tell Grok how to reach it
> model = "my-custom-model"
> api_backend = "responses"            # required — web search uses the Responses API
> # base_url, api_key, env_key optional — defaults to cli-chat-proxy
> ```

### 示例

**OpenAI 兼容端点：**

```toml
[model.local-llama]
model = "llama-3.1-70b"
base_url = "http://localhost:8080/v1"
name = "Local Llama"
temperature = 0.8
```

**Ollama：**

```toml
[model.ollama-codellama]
model = "codellama"
base_url = "http://localhost:11434/v1"
name = "CodeLlama (Ollama)"
```

**Together AI：**

```toml
[model.together-mixtral]
model = "mistralai/Mixtral-8x7B-Instruct-v0.1"
base_url = "https://api.together.xyz/v1"
name = "Mixtral 8x7B"
env_key = "TOGETHER_API_KEY"
```

**OpenAI：**

```toml
[model.gpt-4o]
model = "gpt-4o"
base_url = "https://api.openai.com/v1"
name = "GPT-4o"
env_key = "OPENAI_API_KEY"
```

### 使用自定义模型

```bash
# List available models (including custom)
chaos models

# Use in TUI via slash command
/model my-model

# Use in headless mode
chaos -p "Hello" -m my-model

# Set as default
# In config.toml:
[models]
default = "my-model"
```

### 自定义模型端点

把 Chaos 指向一个自定义的 OpenAI 兼容 `/v1/models` 端点，而不是默认的 cli-chat-proxy。当模型由企业网关或自托管推理栈提供时很有用。

**环境变量：**

| Variable | Required | Description |
|----------|----------|-------------|
| `GROK_MODELS_BASE_URL` | Yes | 推理 / chat completions 的基础 URL（例如 `https://api.acme.com/v1`）。模型列表会自动从 `{base_url}/models` 获取 |
| `XAI_API_KEY` | Yes | 作为 `Authorization: Bearer` 发给自定义端点的 API key |
| `GROK_MODELS_LIST_URL` | No | 当模型列表 URL 与 `{base_url}/models` 不同时，用它覆盖 |

**配置：**

```bash
export GROK_MODELS_BASE_URL="https://api.acme.com/v1"
export XAI_API_KEY="xai-..."
chaos
```

Chaos 会在启动时从 `{GROK_MODELS_BASE_URL}/models` 拉取模型列表，并把推理请求发到 `GROK_MODELS_BASE_URL`。这遵循 OpenAI、Anthropic、OpenRouter、Groq、Together.ai 等使用的标准 OpenAI 兼容约定。

如果你的模型列表端点与 `{base_url}/models` 不同，请显式设置 `GROK_MODELS_LIST_URL`。

**与 `[endpoints]` 配置组合：** 你也可以在 `~/.chaos/config.toml` 里设置端点：

```toml
[endpoints]
models_base_url = "https://api.acme.com/v1"

# Override just the API key for a specific model
[model.grok-build]
api_key = "my-api-key"
```

在使用 `[endpoints]` 加部分模型覆盖时，`base_url` 会从 endpoints 配置继承——你不需要在每个 `[model.*]` 段里再写一遍。

**认证行为：** 设置了 `models_base_url` 后，Chaos 使用 API key 认证（`Authorization: Bearer`）而不是会话认证。不需要 `chaos login`，只要有 API key。

---

## MCP 服务器

用 [Model Context Protocol](https://modelcontextprotocol.io) 服务器扩展 Chaos 的能力。

### 配置

MCP 服务器配置在 `~/.chaos/config.toml` 里：

```toml
[mcp_servers.<name>]
command = "/path/to/server"           # Server executable
args = ["--flag", "value"]            # Command arguments
env = { VAR = "value" }               # Environment variables
headers = { "X-Header" = "value" }    # Optional HTTP headers (Streamable HTTP)
enabled = true                        # Enable/disable (default: true)
startup_timeout_sec = 30              # Init timeout (default: 30)
tool_timeout_sec = 60                 # Tool call timeout (default: 60)
tool_timeouts = { create_issue = 120, search = 30 }  # Per-tool timeout overrides (seconds)
```

### 项目范围的 MCP 服务器

MCP 服务器也可以按项目配置在 `.chaos/config.toml` 里。Chaos 会从当前目录一路向上走到 git 仓库根，在每一级加载 `.chaos/config.toml`：

| Location                        | Scope             | Priority |
| ------------------------------- | ----------------- | -------- |
| `~/.chaos/config.toml`           | All projects      | Lowest   |
| `<repo-root>/.chaos/config.toml` | This repository   | ↑        |
| `<cwd>/.chaos/config.toml`       | Current directory | Highest  |

如果某个项目定义的服务器与全局同名，项目版本会**整体替换**它（字段不做合并——未写的字段取默认值，而不是全局的值）。只在全局配置里定义的服务器不受影响。

**示例：** 在你的仓库里提交一份 `.chaos/config.toml`，就能把 MCP 服务器分享给整个团队：

```
my-project/
├── .chaos/
│   └── config.toml
├── src/
└── ...
```

```toml
# .chaos/config.toml
[mcp_servers.linear]
command = "npx"
args = ["-y", "mcp-remote", "https://mcp.linear.app/mcp"]
```

如果你的 `~/.chaos/config.toml` 里也有一个 `linear` 服务器，项目版本会整体替换它。

> **注意：** 项目级 `.chaos/config.toml` 只支持 `[mcp_servers]`。其他配置段（模型等）只从 `~/.chaos/config.toml` 读取。

### 工具命名

MCP 工具以服务器名做命名空间：

- 服务器 `filesystem` 的工具 `read_file` → `filesystem__read_file`
- 服务器 `github` 的工具 `create_issue` → `github__create_issue`

### 示例服务器

**文件系统访问：**

```toml
[mcp_servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/allowed/directory"]
```

**GitHub 集成：**

```toml
[mcp_servers.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
env = { GITHUB_PERSONAL_ACCESS_TOKEN = "ghp_xxxxxxxxxxxx" }
```

**Postgres 数据库：**

```toml
[mcp_servers.postgres]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-postgres", "postgresql://user:pass@localhost/db"]
```

**自定义服务器：**

```toml
[mcp_servers.my-tools]
command = "/usr/local/bin/my-mcp-server"
args = ["--config", "/etc/my-mcp.json"]
startup_timeout_sec = 30
tool_timeout_sec = 120
```

**带 session id 请求头的 Streamable HTTP：**

```toml
[mcp_servers.my-http-mcp]
url = "http://localhost:5000/api/mcp"
headers = { "x-session-id" = "{{session_id}}" }
```

### 可用的 MCP 服务器

社区服务器见 [MCP Server Registry](https://github.com/modelcontextprotocol/servers)：

- Filesystem、Git、GitHub、GitLab
- PostgreSQL、SQLite、Redis
- Slack、Discord、Linear
- Puppeteer、Playwright
- 以及更多

---

## 记忆

> **实验性功能：** 用 `GROK_MEMORY=1`、`[memory] enabled = true`，或托管的远端设置来启用。

跨会话记忆让 Chaos 能在同一项目的不同会话之间记住事实、决策、代码模式和调试流程。

### 工作原理

记忆以 Markdown 文件的形式存放在 `~/.chaos/memory/` 下：
- **全局**（`~/.chaos/memory/MEMORY.md`）—— 适用于你所有项目的事实
- **工作区**（`~/.chaos/memory/<project-slug>-<hash8>/MEMORY.md`）—— 项目专属的约定与上下文
- **会话日志**（`~/.chaos/memory/<project-slug>-<hash8>/sessions/`）—— 每个会话的摘要

工作区目录带一段短哈希作为后缀以保证唯一（例如 `xai-a3f7b2c9/`）。该哈希由 git remote URL 派生，因此同一仓库的所有克隆与工作树共享同一个记忆目录。

一个 SQLite 索引让所有记忆文件都能被快速混合检索（FTS5 关键词 + 可选向量 KNN）。

### 启用记忆

```bash
# Environment variable (persists for the shell session)
export GROK_MEMORY=1
chaos

# Config file (persists permanently)
# ~/.chaos/config.toml
[memory]
enabled = true
```

### 自动保存了什么

每个会话结束时，Chaos 会把一份**结构化的元数据摘要**写进当天的会话日志：
- 消息条数（用户 / 助手 / 工具）
- 话题 —— 该会话中最初几条真实的用户提示
- 工具使用分布（例如 `read_file: 4, search_replace: 3`）
- 被读取或被编辑过的文件路径
- 日期与会话 ID

shell 命令被有意**排除**在自动保存之外——命令字符串里常常嵌着秘密（token、API key、DSN），而自动保存是静默发生的。
需要命令历史时请用 `/flush`，它由用户主动触发，产出的是 LLM 生成的摘要，而不是原始的逐字输出。

这份摘要在后续会话中可被检索，但**不**记录完整内容或推理过程。

### 用 `/flush` 记录更丰富的知识

要更丰富地记录——决策、模式、调试流程、API 发现——在 TUI 里用 `/flush`。它会触发一次由 LLM 生成的当前会话要点摘要，并写入 `~/.chaos/memory/<project-slug>-<hash8>/sessions/` 下一份带日期的会话日志，之后会被索引，在后续会话中可检索。

想在压缩之前保住重要上下文，或者在一次高产出会话的任何时刻，都可以用 `/flush`。

```
/flush
```

### 手动追加记忆

你也可以直接在 TUI 里追加事实，无需离开会话：

```
/memory workspace Use Rust for all backend services.
/memory global Prefer 2-space indentation in TypeScript.
/memory global Preferred editor: VS Code with Vim keybindings.
```

省略 `workspace` 或 `global` 时默认作用于工作区范围。

### 检索记忆

Chaos 会在每个会话的第一个回合以及压缩之后自动检索记忆。首次注入可以在 `[memory.initial_injection]` 下关闭，或给它单独设定分数阈值。你也可以通过模型提示直接调用 `memory_search` 和 `memory_get`：

```
Search memory for "auth middleware patterns"
Read my workspace MEMORY.md
```

### CLI 命令

```bash
# Open workspace MEMORY.md in $EDITOR / $VISUAL
chaos memory edit

# Open global MEMORY.md
chaos memory edit --global

# Show memory statistics: file count, chunk count, and index size
chaos memory stats
```

### 配置参考

`~/.chaos/config.toml` 里 `[memory]` 下的主要选项：

| Key | Default | Description |
|-----|---------|-------------|
| `enabled` | `false` | 启用记忆（也可通过 CLI 标志或环境变量设置） |
| `session.save_on_end` | `true` | 在会话结束时写入轻量元数据摘要 |
| `watcher.enabled` | `true` | 监视 `~/.chaos/memory/` 的外部改动，并在检索时重建索引 |
| `search.max_results` | `6` | 默认返回的记忆结果条数 |
| `search.min_score` | `0.35` | 显式记忆检索与恢复路径的最低相关性分数阈值 |
| `initial_injection.enabled` | `true` | 启用首回合自动注入记忆 |
| `initial_injection.min_score` | `0.0` | 覆盖首回合注入的分数阈值（`0.0` 保留历史上不过滤的行为） |
| `embedding.model` | *(unset)* | 向量检索用的嵌入模型；未设置则禁用嵌入 |
| `embedding.dimensions` | `1024` | 嵌入向量维度 |

### 可观测性

首回合记忆注入运行时，Chaos 会发出 `grok-shell-memory_injection`
遥测事件。它包含：
- 是否走了问候语回退查询路径
- 结果条数与最高分
- 通过 `configured_min_score` 得到的已配置首回合阈值

---

## 沙箱

Chaos 可以使用操作系统级的内核原语（Linux 上是 Landlock，macOS 上是
Seatbelt）限制代理进程及其派生的命令在你的文件系统和网络上能访问什么。该功能默认关闭。

### 快速开始

```bash
# Run with workspace sandbox (read everywhere, write only to CWD + /tmp)
chaos --sandbox workspace

# Read-only mode (agent can read but not write anything)
chaos --sandbox read-only

# Maximum isolation (read/write CWD only, no child network)
chaos --sandbox strict
```

### 内置 Profile

| Profile         | FS Read            | FS Write                  | Child Network | Use Case                 |
| --------------- | ------------------ | ------------------------- | ------------- | ------------------------ |
| `off` (default) | 不受限       | 不受限              | 不受限  | 无沙箱               |
| `workspace`     | 所有位置         | CWD + `/tmp` + `~/.chaos/` | 允许       | 日常开发       |
| `read-only`     | 所有位置         | 仅 `~/.chaos/`           | 阻止         | 探索、代码评审 |
| `strict`        | CWD + 系统路径 | CWD + `/tmp` + `~/.chaos/` | 阻止         | 不受信任的代码           |

敏感路径（`~/.ssh/`、`~/.aws/`、`~/.gnupg/`、`~/.chaos/auth/`）无论用哪个 profile
都始终禁止写入。

### 自定义 Profile

创建 `~/.chaos/sandbox.toml`（全局）或 `.chaos/sandbox.toml`（按项目）：

```toml
[profiles.devbox]
# Start from a built-in profile, then add overrides
extends = "workspace"
restrict_network = true

# Paths the agent can read but NOT write/delete
read_only = ["/data"]

# Additional writable paths (literal directory grants — no globs;
# trailing /** is treated as the parent directory)
read_write = ["/tmp/scratch"]

# Paths denied entirely
deny = ["/data/shared-secrets"]
```

使用它：

```bash
chaos --sandbox devbox
```

### 工作原理

沙箱在启动时就作用于**整个 chaos 进程**，用的是内核原语——而不是逐条命令包一层。
这意味着所有工具操作都被覆盖：

- `read_file`、`search_replace`、`list_dir` —— 由进程内的 Landlock/Seatbelt 限制
- `bash` 命令、`grep`（rg）—— 子进程自动继承文件系统限制
- 网络 —— 子进程可通过 seccomp 阻止（Linux）

沙箱一旦应用就**不可逆**。这是一项安全设计——模型无法在运行时说服代理放宽限制。

### 当前的限制

- **平台支持**：沙箱强制在 Linux 上使用 Landlock（内核 ≥ 5.13），
  在 macOS 上使用 Seatbelt。如果沙箱无法应用（例如内核不支持、
  缺少 entitlement），Chaos 会记一条警告，然后在没有强制的情况下继续。

- **网络限制是部分的**：带 `restrict_network` 的 profile 会通过 seccomp 阻止
  **子进程**（bash 命令、脚本）的网络，但那些在进程内发起 HTTP 请求的内置工具
  （web search、LLM API）不受影响。代理本身需要网络才能工作，所以进程级的
  网络无法被阻止。

### 事件日志

沙箱事件（profile 已应用、违规）会记录到 `~/.chaos/sandbox-events.jsonl`，
用于遥测与调试。

---

## 自省

用 `chaos inspect` 查看 Chaos 在当前目录中发现的一切：

```bash
chaos inspect          # human-readable output
chaos inspect --json   # machine-readable JSON
```

输出会按类型组织，展示所有已加载的配置：

- **项目指令** —— AGENTS.md / CLAUDE.md 文件及其 token 计数
- **技能** —— 来自 `.chaos/skills/`、`~/.chaos/skills/`、插件和配置路径
- **代理** —— 内置的、用户自定义的、以及插件提供的子代理
- **插件** —— 发现到的插件及其各自提供的东西（技能、代理、钩子、MCP）
- **MCP 服务器** —— 来自 `config.toml`、插件、`~/.claude.json` 和 `.mcp.json`
- **LSP 服务器** —— 来自 `lsp.json` 和插件的语言服务器
- **钩子** —— 项目钩子与插件钩子
- **权限、配置来源** —— 哪些配置文件的生效

插件提供的组件会带着 `[plugin: name]` 标记出现在各自对应的小节里，一眼就能看出每个技能、MCP 服务器或代理来自哪里。

---

## Claude Code 兼容性

Chaos 会在原生 `.chaos/` 路径之外，自动发现 Claude Code 目录中的配置。不需要额外设置。

### 会被拾取的内容

| Component         | Claude Code 位置                                 | Chaos 如何使用                 |
| ----------------- | ---------------------------------------------------- | -------------------------------- |
| **Skills**        | `.claude/skills/`, `~/.claude/skills/`               | 作为技能加载（与 `.chaos/skills/` 相同） |
| **Agents**        | `.claude/agents/`, `~/.claude/agents/`               | 作为子代理加载              |
| **Plugins**       | `.claude/plugins/`, `~/.claude/plugins/`             | 连同其所有组件一起被发现   |
| **Installed plugins** | `~/.claude/plugins/installed_plugins.json`        | 加载其中的每个 `installPath`     |
| **Marketplaces**  | `~/.claude/plugins/known_marketplaces.json`          | 采用 `installLocation` 里的插件目录 |
| **MCP servers**   | `~/.claude.json`, `.mcp.json`                        | 与 `config.toml` 一起加载   |
| **Project rules** | `CLAUDE.md`, `.claude/CLAUDE.md`                     | 作为项目指令加载   |
| **Permissions**   | `.claude/settings.json`, `.claude/settings.local.json` | 没有 TOML 配置时的回退   |

### 插件组件

Claude Code 插件可以提供技能（`skills/`）、命令（`commands/`）、代理（`agents/`）、钩子（`hooks/hooks.json`）、MCP 服务器（`.mcp.json`）和 LSP 服务器（`.lsp.json`）。所有这些组件类型都会在运行时被 Chaos 发现并使用。

---

## 内置工具

Chaos 默认包含这些工具：

| Tool             | Description                                                    |
| ---------------- | -------------------------------------------------------------- |
| `read_file`      | 读取文件内容，带行号                           |
| `search_replace` | 对文件做精确编辑                                    |
| `grep_search`    | 用正则模式搜索（ripgrep）                           |
| `list_dir`       | 列出目录内容                                        |
| `bash`           | 执行 shell 命令                                         |
| `web_search`     | 在网上搜索最新信息                      |
| `web_fetch`      | 抓取指定 URL 并以 markdown 返回其内容        |
| `todo_write`     | 创建和管理任务列表                                   |
| `task`           | 启动子代理会话（需要 `--subagents`）              |
| `kill_task`      | 终止正在运行的后台任务或子代理                |
| `get_task_output` | 获取后台任务或子代理的输出与状态      |
| `memory_search`  | 检索跨会话记忆（需要已启用记忆） |
| `memory_get`     | 按路径读取记忆文件                                     |
| `search_tool`    | 发现可用的集成工具（MCP）                     |
| `use_tool`       | 调用通过 `search_tool` 发现的集成工具           |
| `lsp`            | 通过语言服务器提供代码智能（需要 `lsp_tools`）  |

### 控制可用的工具

在无头模式里，你可以用 `--tools`（白名单）和 `--disallowed-tools`（黑名单）标志限制或移除工具。细节与示例见[无头模式](#headless-mode)。

在代理配置里，使用 `tools` 和 `disallowedTools` frontmatter 字段：

```yaml
---
tools:
  - read_file
  - grep_search
  - list_dir
disallowedTools:
  - web_search
  - Agent(explore)
---
```

### `web_fetch`

抓取指定 URL 并以 markdown 返回其内容。**默认禁用** —— 用 `GROK_WEB_FETCH=1` 启用。 

没有设置自定义 `allowed_domains` 时，该工具允许一份默认白名单，里面是有用的文档站点（SpaceXAI、语言文档、框架、云厂商、数据库等）。不在白名单上的域名会向用户请求批准；`--always-approve` 则全部自动批准。域名匹配不区分大小写，会去掉 `www.` 前缀，并支持带路径范围的条目（例如 `x.ai/company`）。

---

## 会话持久化

Chaos 会自动把对话持久化到磁盘。这在所有模式下都有效：TUI、无头模式、agent stdio。

### 存储布局

会话存放在 `~/.chaos/sessions/` 下，按 URL 编码后的工作目录组织：

```
~/.chaos/sessions/<encoded-cwd>/<session-id>/
  summary.json            # metadata: title, timestamps, model, message count
  updates.jsonl           # ACP session update stream (conversation + tool calls)
  chat_history.jsonl      # raw chat messages sent to the model
  plan.json               # TODO/task list state
  rewind_points.jsonl     # file snapshots for /rewind undo
  signals.json            # session signals (turn count, token usage)
  feedback.jsonl          # user feedback and ratings
  compaction_checkpoints/ # saved state from auto-compact
  subagents/              # child session directories (when subagents are enabled)
```

`summary.json` 是索引条目——它包含会话标题、模型 ID、创建/更新时间戳，以及（对恢复出来的会话而言）父会话引用。`updates.jsonl` 是权威的对话日志，`/load` 与会话恢复都以它为准。

### TUI

聊天过程中会话会自动保存。要重新开始：

```
/new
```

除非你继续某个此前的会话，否则每次启动 TUI 都会新建一个会话。

### 无头模式

用标志控制会话行为：

```bash
# New session each time (default)
chaos -p "Hello"

# Create or resume a named session
chaos -p "Remember: X=42" -s my-session
chaos -p "What is X?" -s my-session

# Resume existing session (errors if not found)
chaos -p "Continue" -r my-session

# Continue most recent session in current directory
chaos -p "What were we doing?" -c
```

会话 ID 会在 JSON 输出里返回：

```bash
chaos -p "Hello" --output-format json | jq -r '.sessionId'
```

### Agent stdio（ACP）

用 ACP 构建时，会话通过协议方法管理：

```typescript
// Create new session
const { sessionId } = await connection.request("session/new", {
  cwd: "/path/to/project",
  mcpServers: [],
});

// Load existing session
await connection.request("session/load", {
  sessionId: "existing-session-id",
  cwd: "/path/to/project",
  mcpServers: [],
});
```

代理会自动持久化所有会话更新。客户端可以重新连接，并按 ID 载入此前的会话。

---

## 文件位置

| Path                  | Description                                         |
| --------------------- | --------------------------------------------------- |
| `~/.chaos/config.toml` | 配置文件                                  |
| `~/.chaos/sessions/`   | 已持久化的会话（按工作目录组织） |
| `~/.grok/auth.json`   | 上游兼容遗留的 xAI 账号凭据文件（由上游自动管理；Chaos 不写入也不读取）           |
| `~/.chaos/memory/`     | 跨会话记忆文件与索引                |
| `~/.chaos/skills/`     | 用户范围的技能定义                       |
| `~/.chaos/plugins/`    | 用户范围的插件                                 |
| `~/.chaos/agents/`     | 用户范围的代理定义                       |
| `.chaos/config.toml`   | 项目范围的配置（MCP 服务器）                 |
| `.chaos/skills/`       | 项目范围的技能定义                    |
| `.chaos/plugins/`      | 项目范围的插件                              |
| `.chaos/agents/`       | 项目范围的代理定义                    |
| `.chaos/hooks/`        | 项目范围的钩子                                |
| `.chaos/lsp.json`      | LSP 服务器配置                            |
| `~/.claude/skills/`   | 用户范围的技能（Claude Code 兼容）             |
| `~/.claude/plugins/`  | 用户范围的插件（Claude Code 兼容）            |
| `~/.claude.json`      | MCP 服务器（Claude Code 兼容）                    |
| `.mcp.json`           | 项目范围的 MCP 服务器（Claude Code 兼容）     |

---

## 环境变量

| Variable                         | Description                                                                                              |
| -------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `XAI_API_KEY`         | 来自 [console.x.ai](https://console.x.ai) 的 API key。用于自定义端点认证与 API key 登录      |
| `GROK_CLI_CHAT_PROXY_BASE_URL`  | 覆盖 cli-chat-proxy 的 URL（默认：`https://cli-chat-proxy.grok.com/v1`）                          |
| `GROK_MODELS_BASE_URL`          | 推理的自定义基础 URL。模型列表自动从 `{base_url}/models` 获取（见[自定义模型端点](#custom-models-endpoint)） |
| `GROK_MODELS_LIST_URL`          | 当模型列表 URL 与 `{GROK_MODELS_BASE_URL}/models` 不同时，用它覆盖                                              |
| `GROK_AUTH_PROVIDER_COMMAND`     | 外部认证二进制（配置文件之外的替代方式）。见[外部认证 Provider](#external-auth-provider) |
| `GROK_AUTH_TOKEN_TTL`            | 供只输出裸 token 的外部认证 Provider 使用的 token 存活秒数。见[外部认证 Provider](#external-auth-provider) |
| `GROK_AUTH_EARLY_INVALIDATION_SECS` | 距 `expires_at` 还有多少秒就把 token 视为过期（默认 `300`）。见[自动凭据刷新](#automatic-credential-refresh) |
| `GROK_OIDC_ISSUER`              | OIDC issuer URL（配置文件之外的替代方式）。见 [OIDC](#oidc-customer-sso)                             |
| `GROK_OIDC_CLIENT_ID`           | OIDC client ID（配置文件之外的替代方式）。见 [OIDC](#oidc-customer-sso)                              |
| `CHAOS_HOME`                    | 覆盖配置目录（默认：`~/.chaos`）。兼容读取旧的 `GROK_HOME`                                                           |
| `GROK_SUBAGENTS`                | 启用（`1`）或禁用（`0`）子代理/task 工具支持                                                 |
| `GROK_MEMORY`                   | 启用（`1`）或禁用（`0`）跨会话记忆                                                       |
| `GROK_AGENT`                    | 自定义代理定义的路径或名字（见[代理配置](#agent-profiles)）                             |
| `GROK_WEB_FETCH`                | 启用（`1`）或禁用（`0`）`web_fetch` 工具                                                       |
| `GROK_WEB_FETCH_PROXY`          | `web_fetch` 请求的出口代理 URL（会被 `[toolset.web_fetch] proxy_endpoint` 覆盖）           |
| `GROK_RESPECT_GITIGNORE`        | 设为 `0` 时禁用工具中的 `.gitignore` 过滤                                                  |
| `GROK_FEEDBACK_ENABLED`         | 独立于遥测启用（`1`）或禁用（`0`）反馈系统                               |
| `GROK_DEPLOYMENT_KEY`           | 企业部署用的管理 API key                                                            |
| `GROK_LOG_FILE`                 | 提供一个文件路径即可启用文件日志（该值会逐字用作路径）                    |
| `GROK_DEBUG_LOG`                | 调试全量日志（由 `--debug` 设置）：真值会把每会话日志路由到 `~/.chaos/debug/<sessionId>.txt`，给出路径则写入那一个文件 |
| `RUST_LOG`                      | stderr 的日志过滤（无头 `-p` 默认 `off`，其他非 TUI 模式默认 `error`；TUI 会捕获 stderr），也用于 `GROK_LOG_FILE` 日志；`--debug` 的全量日志不受它影响 |

---

## Shell 补全

为你的 shell 生成补全脚本并安装，就能对 `chaos` 的命令和标志做 Tab 补全。

**注意：** 下面的路径是推荐默认值。有些环境不会自动加载标准位置——你可能需要按自己的 shell 框架或发行版约定调整。

### Bash

生成并安装：

```bash
mkdir -p ~/.local/share/bash-completion/completions
chaos completions bash > ~/.local/share/bash-completion/completions/chaos
```

重新加载你的 shell，或运行 `source ~/.bashrc`。

另一种方式（Chaos 管理的目录）：

```bash
mkdir -p ~/.chaos/completions/bash
chaos completions bash > ~/.chaos/completions/bash/chaos.bash
```

加到 `~/.bashrc`：

```bash
[[ -r "$HOME/.chaos/completions/bash/chaos.bash" ]] && source "$HOME/.chaos/completions/bash/chaos.bash"
```

### Zsh

生成并安装：

```bash
mkdir -p ~/.zsh/completions
chaos completions zsh > ~/.zsh/completions/_chaos
```

加到 `~/.zshrc`：

```zsh
fpath=(~/.zsh/completions $fpath)
autoload -Uz compinit
compinit
```

另一种方式（Chaos 管理的目录）：

```bash
mkdir -p ~/.chaos/completions/zsh
chaos completions zsh > ~/.chaos/completions/zsh/_chaos
```

加到 `~/.zshrc`：

```zsh
fpath=("$HOME/.chaos/completions/zsh" $fpath)
autoload -Uz compinit
compinit
```

### 升级之后

升级 `chaos` 之后请重新生成补全脚本——脚本反映的是所装版本 CLI 的样子。

---

## 故障排查

### 调试日志

把日志写进文件便于调试。TUI 会捕获 stderr，所以单靠 `RUST_LOG` 在生产里看不到输出——请改用 `chaos --debug` 或 `GROK_LOG_FILE`：

```bash
# Per-session debug log (~/.chaos/debug/<sessionId>.txt)
chaos --debug

# Log to a custom path
GROK_LOG_FILE=/tmp/grok-debug.log chaos

# Tail the most-recently-opened session's log in another terminal (Unix symlink)
tail -f ~/.chaos/debug/latest.txt
```

`--debug` 的全量日志使用固定过滤器（一方 crate 为 `debug`），不受 `RUST_LOG` 收窄。`GROK_LOG_FILE` 日志默认 `debug`，并遵循 `RUST_LOG`，所以你可以设置模块级过滤器做定向调试：

```bash
# Debug auth, info for everything else
GROK_LOG_FILE=/tmp/grok-debug.log RUST_LOG="info,xai_grok_shell::auth=debug" chaos
```

### 认证失败

Chaos 是 BYOK：没有账号登录流程，也就没有「清掉凭据再重新登录」这一步。遇到认证失败，请检查 Provider 配置：编辑 `~/.chaos/config.toml`，或在 TUI 里用 `/provider` 打开配置面板。

```bash
# Debug auth issues — check the log for "auth:" entries
chaos --debug-file /tmp/grok-auth.log -p "hello"
grep "auth:" /tmp/grok-auth.log
```

### 找不到模型

```bash
# List available models
chaos models

# Check config.toml for typos in [model.*] sections
```

### MCP 服务器起不来

```bash
# Test the server command manually
npx -y @modelcontextprotocol/server-filesystem /path

# Increase startup timeout in config
[mcp_servers.filesystem]
startup_timeout_sec = 30
```

### 命令超时

```toml
# Increase bash timeout in config.toml
[toolset.bash]
timeout_secs = 300.0
```

### 检查会话数据

会话文件就是普通的 JSON/JSONL，可以直接查看：

```bash
# Find sessions for the current directory
ls ~/.chaos/sessions/

# Read session metadata
cat ~/.chaos/sessions/<encoded-cwd>/<session-id>/summary.json | jq .

# View conversation history
cat ~/.chaos/sessions/<encoded-cwd>/<session-id>/updates.jsonl | head -20

# Count turns in a session
wc -l ~/.chaos/sessions/<encoded-cwd>/<session-id>/chat_history.jsonl
```

### 上下文窗口已满

如果自动压缩触发得太频繁，就把阈值调低，让它更早压缩、留出更多余量：

```toml
[session]
auto_compact_threshold_percent = 70    # default is 85
```

---

## 许可证

基于 Apache License, Version 2.0 授权。见仓库根目录的
`LICENSE` 文件。
