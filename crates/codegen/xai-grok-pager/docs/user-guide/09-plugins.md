# 插件

插件把技能、斜杠命令、代理、钩子和 MCP 服务器打包成一个可安装的单元。你从插件市场获取插件，安装想要的那些，Chaos 会加载它们带来的内容。要构建并分享你自己的插件市场，见[创建你自己的插件市场](#创建你自己的插件市场)。

---

## 插件市场如何工作

插件市场是某人已发布并分享的一组插件目录。使用它分两步，就像添加应用商店：添加插件市场后可以浏览其中的插件，然后由你选择安装哪些。

1. **添加插件市场**，让 Chaos 能展示它提供的内容。此时不会安装任何东西。
2. **安装你想要的插件**，一次一个。

插件在安装并启用之前保持关闭，插件的钩子和 MCP 服务器在你[信任](#信任与安全)它之前保持未激活。

---

## 添加插件市场

插件市场源可以是 GitHub 仓库、任意主机上的 git URL，或本地文件夹。从命令行添加一个：

```bash
chaos plugin marketplace add my-org/team-plugins                  # GitHub shorthand (owner/repo)
chaos plugin marketplace add https://gitlab.com/acme/plugins.git  # any git host, include https:// and .git
chaos plugin marketplace add ./my-marketplace                     # a local folder
```

用 `chaos plugin marketplace list` 列出源，用 `chaos plugin marketplace update [<name>]` 刷新源，用 `chaos plugin marketplace remove <url>` 移除源。

你也可以在配置中声明源，让它们始终存在。

### 在 config.toml 中

每个源需要一个 `name`，以及一个 `git` URL（可选 `branch`）或本地 `path`：

```toml
[[marketplace.sources]]
name = "My Team Plugins"
git = "https://github.com/my-org/plugins.git"

[[marketplace.sources]]
name = "Local Dev"
path = "~/dev/my-plugins"
```

### 在 settings.json 中

在 `extraKnownMarketplaces` 下按名称添加源。每个条目的 `source` 是 `git`（带 `url`）、`github`（带 `repo`）或 `local`（带 `path`）之一：

```json
{
  "extraKnownMarketplaces": {
    "my-marketplace": {
      "source": { "source": "git", "url": "git@github.com:my-org/plugins.git" }
    }
  }
}
```

把这个文件放在 `~/.chaos/settings.json` 或 `~/.claude/settings.json`。

---

## 安装并使用插件

添加插件市场后，按名称安装插件。你也可以直接从仓库或本地路径安装：

```bash
chaos plugin install deploy-tools --trust
```

你要安装的源支持几种形式：

- `owner/repo`（GitHub 简写）、`owner/repo@v1.0`（一个 ref）、`owner/repo@<commit-sha>`（一个确切提交，拉取后校验）或 `owner/repo#subdir`
- 完整的 git URL（`https://github.com/user/repo.git`）或 SSH（`git@github.com:user/repo.git`）
- 本地路径（`./local-dir` 或 `/absolute/path`）

不带 `--trust` 运行 `chaos plugin install <source>` 时，Chaos 会显示该源，警告安装会激活插件的钩子、MCP 服务器和技能，然后停下。加上 `--trust` 才会继续。只从你信任的源安装插件（见[信任与安全](#信任与安全)）。

插件的技能会出现在斜杠菜单里。当技能名称有歧义时，Chaos 会显示带插件名前缀的限定形式，例如 `/deploy-tools:release`。要加载新安装的插件，在插件标签页按 `r` 或开始新会话。

---

## 管理插件

### 从命令行

```bash
chaos plugin list [--json] [--available]   # installed plugins (--available requires --json)
chaos plugin uninstall <name> [--confirm] [--keep-data]   # aliases: rm, remove
chaos plugin update [<name>]               # omit the name to update every plugin
chaos plugin enable <name>
chaos plugin disable <name>
chaos plugin details <name>                # show the plugin's component inventory
```

### 在终端 UI 中

用 `Ctrl+L`（VS Code 系终端之外）或 `/plugins`（任意终端，且 VS Code 系终端上必须用它）打开插件模态。它有六个标签页：**Hooks**、**插件**、**市场**、**Skills**、**工作流**、**MCP 服务器**；用 `Tab` / `Shift+Tab` 切换。`/hooks`、`/marketplace`、`/skills`、`/workflows`、`/mcps` 命令会打开模态并停在对应的标签页。

在**插件**标签页中，按 `Enter` 展开一个插件，查看其名称、版本、作用域（`cli`、`project`、`user`、`custom path` 或插件市场源名称）、技能、代理、钩子、MCP 服务器（插件未受信任时显示为 `blocked`）、描述和路径。然后：

| 键 | 操作 |
|-----|--------|
| `r` | 重新加载所有插件 |
| `a` | 从 `owner/repo`、URL 或本地路径添加插件 |
| `Space` | 启用或禁用选中的插件 |
| `x` | 卸载选中的插件 |
| `f` | 按状态过滤（全部、已启用或已禁用） |
| `/` | 按名称搜索 |

在**市场**标签页中，从你的源浏览并安装：

| 键 | 操作 |
|-----|--------|
| `i` | 安装选中的插件 |
| `d` | 卸载选中的插件 |
| `a` | 添加插件市场源 |
| `x` | 移除选中的源及其插件 |
| `r` | 刷新来源 |
| `u` | 更新选中的插件 |

市场标签页里的组件摘要只为发布了 [`plugin-index.json`](#添加目录可选) 目录的市场显示。破坏性操作会请求确认：按小写 `y` 确认，按其它任意键（包括 `Esc`）取消。

在**工作流**标签页中（直接用 `/workflows` 打开，或在上面的命令处按 `Tab`），浏览 Chaos 找到的已保存工作流：内置的、项目 `.chaos/workflows/` 和用户 `~/.chaos/workflows/` 下的。每行显示工作流的名称、来源和描述；按 `Enter` 展开其路径和何时使用的说明，`r` 重新加载列表，`/` 搜索。各行仅供浏览——用 `/workflow <name>` 或它自己的斜杠命令来运行。

### 在配置中打开或关闭插件

在 `~/.chaos/config.toml` 中设置：

```toml
[plugins]
paths = ["~/my-plugins/custom-tools"]        # extra plugin directories
disabled = ["user/a1b2c3d4/noisy-plugin"]    # names or IDs to skip
enabled = ["project/9f8e7d6c/team-tools"]    # names or IDs to force on
```

插件默认是关闭的：在 `enabled` 里列出某个插件即打开它，在 `disabled` 里列出则发现它但跳过加载。每个条目是纯插件名（来自 `chaos plugin list`）或完整 ID（`<scope>/<hash>/<name>`）。

要完全隐藏插件与钩子界面，在 `~/.chaos/pager.toml` 中设置 `disable_plugins = true`。

---

## 信任与安全

插件以你的权限运行，所以要像对待任何你安装的软件一样对待它们：只添加你信任的插件市场，只从你信任的源安装插件。

已启用的插件需要信任才能加载技能、命令、钩子、MCP 服务器和 LSP 服务器。未受信任的插件代理仍会列出，但只有 frontmatter。Chaos 自动信任 `~/.chaos/plugins/` 中的插件；`.chaos/plugins/` 中的项目插件需要信任。安装时加 `--trust` 即授予：

```bash
chaos plugin install <source> --trust
```

受信任插件的 `.mcp.json` 服务器像其它 MCP 配置一样附加到会话，子代理会继承它们。插件代理（`plugin-name:agent-name`）默认使用父会话的 MCP 服务器，与 `~/.chaos/agents/` 下的用户代理相同；用 `mcpInheritance` frontmatter 可以限制这一点（见[子代理](16-subagents.md#mcp-继承)）。为安全起见，插件代理的 frontmatter 不能声明 `mcpServers` 或钩子，也不能设置 `permissionMode: bypassPermissions`。

---

## 创建你自己的插件市场

插件市场是一个列出一组插件的 git 仓库（或本地文件夹）。添加它就像添加应用商店：让人们浏览你的插件，由他们选择安装哪些。发布自己的插件市场是团队或组织从一处分享其技能、命令、代理、钩子和 MCP 服务器的方式。

你需要三样东西：一个 git 仓库、每个插件一个文件夹，以及一个列出它们的索引文件。

### 搭建仓库

1. **创建一个 git 仓库。** 私有仓库即可；访问使用每个人自己的 git 凭据。
2. **把每个插件添加为一个文件夹。** 插件文件夹可包含 `skills/`、`commands/`、`agents/`、`hooks/hooks.json`、`.mcp.json` 中的任意内容，以及可选的 `plugin.json` 清单（见[插件包含什么](#插件包含什么)）。
3. **在 `.grok-plugin/marketplace.json` 中列出插件。** 这是 Chaos 读取的索引。
4. **推送仓库。**

典型布局：

```
my-org-plugins/
  .grok-plugin/
    marketplace.json      # the index Grok reads (required)
    plugin-index.json     # optional catalog for richer browsing
  plugins/
    gdrive/
      plugin.json         # optional manifest
      skills/gdrive/SKILL.md
      .mcp.json           # MCP servers this plugin adds
```

Chaos 从 `.grok-plugin/marketplace.json` 读取索引。它也接受 `.grok-plugin/plugin.json` 以及 `.claude-plugin/` 的等价形式。

### 编写索引

`marketplace.json` 为插件市场命名并列出每个插件：

```json
{
  "name": "My Org Plugins",
  "description": "Internal skills and tools",
  "owner": { "name": "Platform Team", "email": "platform@example.com" },
  "plugins": [
    {
      "name": "gdrive",
      "description": "Search and edit Google Drive, Docs, Sheets, and Slides",
      "category": "productivity",
      "source": { "type": "local", "path": "./plugins/gdrive" }
    }
  ]
}
```

每个插件的 `source` 以以下两种方式之一指向其文件：

- **在本仓库中**：`{ "type": "local", "path": "./plugins/gdrive" }`。纯字符串 `"./plugins/gdrive"` 也可以。
- **在单独的仓库中**：`{ "source": "url", "url": "https://github.com/my-org/gdrive.git", "sha": "<full commit sha>" }`。固定一个 `sha`，安装才可复现（当你[要求固定版本](#要求固定版本)时必需）。

可选的每插件字段：`version`、`author`、`homepage`、`tags`、`keywords`。

### 添加目录（可选）

`plugin-index.json` 目录让市场浏览器在任何人安装之前就能展示每个插件的技能、命令、钩子和代理。它只用于展示，没有它安装也能工作，团队通常在 CI 里生成它：

```json
{
  "version": 1,
  "plugins": {
    "gdrive": {
      "components": {
        "skills": [{ "name": "gdrive", "description": "Google Drive access" }]
      }
    }
  }
}
```

### 检查并分享它

发布前用 `chaos plugin validate [<path>]` 校验插件，用 `chaos plugin tag [<path>] [--push]` 从清单版本打一个发布标签。然后让人们访问该仓库。他们添加一次，再安装想要的插件：

```bash
chaos plugin marketplace add my-org/my-org-plugins   # GitHub shorthand, a git URL, or a local path
chaos plugin install gdrive --trust
```

要为所有人自动安装而不是逐人安装，见[跨组织分发](#跨组织分发)。

---

## 跨组织分发

管理员通过 Chaos 的 TOML 分层外加一个可选的 Claude 策略文件来控制插件、插件市场和 MCP 服务器：

- **`managed_config.toml` / `requirements.toml`**（以及 macOS MDM）是**原生**策略。当要求 Chaos 在每个服务器和插件市场上强制执行允许列表、拒绝列表和固定（pin）时——包括用户自己的配置或插件定义的那些——就写在这里。`requirements.toml` / MDM 是防篡改层；用户可写的 `~/.chaos` 副本只是自我约束。
- **Claude 的 `managed-settings.json`** 是**劝告性**的。它的 MCP 与市场限制只约束**外部**主体——项目文件（`.chaos/config.toml`、`.mcp.json`）、导入的 Claude 配置、CLI 覆盖和客户端注入的服务器。它们从不约束 Chaos 原生的主体（用户/系统 `config.toml`、插件提供的定义、管理员的 pin）。**添加**插件市场或安装新源始终被视为外部行为，所以劝告性的严格列表仍会拒绝未列出的 `marketplace add` / `plugin install` 源。

各层按**最严者胜**合并：任何拒绝都获胜，每个受限源都必须放行，布尔 pin 只收紧（`false` 固定后，之后的 `true` 不能解绑）。TOML 里 CamelCase 的 Claude 键与 snake_case 的 Chaos 键都接受。

`chaos inspect`（以及 `chaos inspect --json`）显示已加载的 MCP/市场列表、`allowManagedMcpServersOnly` 处于 `off` / `advisory` / `enforced` 哪种状态、额外的市场 pin，以及 **Enforced by policy**（由策略强制执行）之下只收紧的 pin。

### 把插件市场铺开给所有人

在 `managed_config.toml` 中添加源，并打开你想要的插件：

```toml
[[marketplace.sources]]
name = "My Org Plugins"
git = "https://github.com/my-org/my-org-plugins.git"

# Plugins stay off until enabled. List plugin names (from `chaos plugin list`)
# or full IDs (`<scope>/<hash>/<name>`).
[plugins]
enabled = ["gdrive"]
```

要实现无需每人动手的无人值守安装，还可以把插件的文件放在 Chaos 会自动发现并信任的位置：`~/.chaos/plugins/`，或你的设备管理工具管理、并用 `[plugins].paths` 指向的目录。然后用 `[plugins].enabled` 启用它们。

受管的工作区也可以不经过插件，直接向用户同步技能。同步来的技能带 `server` 作用域，由工作区管理；用户同名的自有技能会遮蔽同步来的那个。见[技能](08-skills.md)。

### 限制可以添加哪些插件市场

列出人们仅可添加的 git 源。任何其它 git URL 都会被拒绝。被采纳的条目是 `{ "source": "git", "url": "…" }` 和 `{ "source": "github", "repo": "owner/repo" }`（规范化为 `https://github.com/owner/repo.git`）。可选的 `ref` / `branch` 存在额外的 pin 上；它不是允许列表身份的一部分。严格列表中的 `local` 条目会被丢弃并给出警告；它们不允许任何东西。

键**存在**本身就是限制：空列表（`strict_known_marketplaces = []`）、每个条目都不受支持的列表，或类型错误的键都是完全锁定，在修复之前拒绝一切添加和安装。不写这个键，插件市场就不受限制。

在存在生效严格列表时添加**本地路径**会被拒绝（路径永远不会匹配 git-URL 允许列表；fail closed），除非**管理员**的 `extraKnownMarketplaces` pin 恰好指名该路径。用户可写的 `~/.chaos` 层的 pin 不能开出这个例外。未通过列表的既有 git 源在加载时被丢弃（`Marketplace source blocked by allowlist`）。

```toml
# /etc/grok/requirements.toml  (native: binds every marketplace)
[[strict_known_marketplaces]]
source = "git"
url = "git@github.enterprise.example:ACME/my-org-plugins.git"

[[strict_known_marketplaces]]
source = "github"
repo = "ACME/more-plugins"
```

同样的列表也能用于 Claude 的 `managed-settings.json`（对已配置的 Chaos 原生源是劝告性的）。URL 比较只在 **scheme 和 host** 上折叠大小写，并恰好剥掉末尾的一个 `.git`（`repo.git.git` 是另一个仓库）。用 `chaos inspect` 查看已加载的允许列表。

用 `extraKnownMarketplaces` / `extra_known_marketplaces` 从策略分发额外源。名字由最先 pin 它的那一层赢得；已持有该名字但 URL 不同的已配置源不会被覆盖（记录日志）。额外 pin 上的 `autoUpdate = false` 会关掉**全局**的会话启动插件自动更新（没有按市场划分的 Chaos 等价键）。

```toml
[extra_known_marketplaces.acme]
source = { source = "git", url = "https://github.com/ACME/my-org-plugins.git", ref = "main" }
```

### 限制可以运行哪些 MCP 服务器

Chaos 会在每个原生 TOML 策略层以及 Claude 的 `managed-settings.json`（劝告性；见上文）上强制执行 MCP 允许/拒绝列表。`chaos inspect` 打印合并后的列表。

每条允许或拒绝条目是以下之一：

| 字段 | 匹配 |
| --- | --- |
| `serverUrl` / `server_url` | HTTP/SSE 服务器 URL。两份列表上 host 和 path 都遵循 Claude 的 `serverUrl` 规则：`*` 通配符分别匹配 host 和 path（`https://*.example.com/*` 不能匹配另一个 host 上的相似路径）；不带 path 的模式（`https://mcp.example.com`，或末尾只有裸 `/`）匹配该 host 上的每条路径；带 path 的模式只匹配那条路径，所以用 `/mcp/*` 来限定授权范围。**允许条目**在 scheme 和端口上比 Claude 更严。scheme 必须是字面量或裸 `*`（`*` 匹配受支持的远程 scheme，即 http 和 https，仅此而已）；不带 scheme 的 `*.example.com/*` 或部分 scheme 通配（如 Claude 的 `http*://`）永远不会匹配，并在启动时记录一条警告。端口保持字面量（https 上显式的 `:443` 与不带端口是同一目标）；通配端口（如 Claude 的 `http://localhost:*/*`）永远不会匹配，并在启动时记录一条警告——请把每个端口都列出来。**拒绝条目**按 host 和 path 匹配、跨所有 scheme 和端口：`mcp.untrusted.example/*` 与 `http://mcp.untrusted.example:*/*` 都会封禁该 host 的任意 scheme 和端口，且不给出警告。 |
| `command` | stdio 可执行文件名，与配置的 command 精确匹配（不含 argv 其余部分）。 |
| `serverCommand` / `server_command` | stdio argv，与 `[command, args…]` 精确匹配。不完整的数组（非字符串或为空）会匹配错误的命令：在允许列表上它什么也不放行；在拒绝列表上它会把该源锁死（见下文）。 |
| `serverName` / `server_name` | 任意传输上的配置名。比较在空格变为 `_` 之后不区分大小写；运行时名字上的 `grok_com_` 前缀会被剥掉。 |

**拒绝获胜。** 匹配 `deniedMcpServers` 的服务器即使同时匹配 `allowedMcpServers` 也会被封禁。若 `allowedMcpServers` 存在，则每个未列出的服务器都被封禁；存在但为空的列表（`allowed_mcp_servers = []`）会封禁该文件约束的每一个服务器，所以不要把它当脚手架发布。只有拒绝的文件封禁列出的服务器、放行其余（空拒绝列表无害）。跨层看，一个服务器必须通过**每一个**受限源。

**配置错误是锁死而不是放行。** 类型错误的策略键（本应是列表的地方写了表或字符串）、同一键两种拼写且值不同、每个条目都不受支持的允许列表，或无法执行的拒绝条目（未知字段、不完整的 `serverCommand`、永远匹配不到的 `serverUrl`）都会把该文件的 MCP 策略锁死：它约束的每个服务器都被封禁，理由是 `locked down by policy (<file>)`，直到该文件被修复。启动日志会指名该文件和有问题的键。不可用的**允许**条目只是什么也不放行。

`allowManagedMcpServersOnly = true`（或 `allow_managed_mcp_servers_only`）是一道锁死：即使允许列表为空，也要求允许条目正向匹配。原生 TOML 在 inspect 里显示为 `enforced`；只来自 Claude 的显示为 `advisory`（Chaos 原生的服务器豁免）。

`enableAllProjectMcpServers = false` 会丢弃项目作用域的 MCP，除非该服务器同时匹配某条允许条目。

```toml
# /etc/grok/requirements.toml
allow_managed_mcp_servers_only = true
enable_all_project_mcp_servers = false

[[allowed_mcp_servers]]
server_url = "https://*.example.com/*"

[[allowed_mcp_servers]]
command = "npx"

[[allowed_mcp_servers]]
server_command = ["npx", "@corp/mcp"]

[[allowed_mcp_servers]]
server_name = "linear"

[[denied_mcp_servers]]
command = "node"

[[denied_mcp_servers]]
server_url = "https://mcp.untrusted.example/*"
```

这些列表在 Chaos 合并用户、项目、插件和导入的 MCP 配置之后应用。被封禁的服务器会从会话中丢弃（记录为 `MCP server blocked by managed settings policy`），理由是匹配了 `deniedMcpServers`、不在 `allowedMcpServers` 中、被策略锁死或项目-MCP pin，外加策略文件路径（inspect/JSON/日志保留完整路径；面向用户的拒绝只显示文件名）。

部署也可以直接向用户下发 MCP 服务器。原生允许列表仍然约束任何配置——受管的或个人的——所允许运行的内容。

### 关闭会话启动时的插件自动更新

`plugin_auto_update = false` / `pluginAutoUpdate = false` 只收紧。这个全局 pin 是 Chaos 自己的键，没有 Claude 对应物。固定后，会话启动不再扫描插件市场，也不再逐插件分发更新（没有 toast）。手动 `chaos plugin update` 仍然有效。Claude 按市场的 `extraKnownMarketplaces.<name>.autoUpdate: false` 固定的是同一个全局开关。

### 要求固定版本

拒绝任何未固定到完整 commit sha 的远程插件安装或更新：

```toml
[marketplace]
require_sha = true
```

你也可以设置 `GROK_MARKETPLACE_REQUIRE_SHA=1`。两者都只收紧策略；都不能把它关回去。在你的插件市场的 `plugin-index.json` 里发布 `sha` 值，从它安装就能满足这条规则。直接内嵌在插件市场仓库里的插件是从该仓库的检出复制的，所以要用同样的方式固定：在 `plugin-index.json` 里写 `sha` 值。

### 关闭插件界面

要隐藏插件与钩子界面，在 `pager.toml` 中设置：

```toml
disable_plugins = true
```

### 这不涵盖什么

插件市场分发的是 Chaos 内容：技能、命令、代理、钩子和 MCP 服务器配置。它们不会在机器上安装程序。运行辅助二进制（例如自定义登录工具）的技能或 MCP 服务器仍需单独交付该二进制——随你的部署打包，或通过你的设备管理工具推送。

---

## 故障排查

**你安装的插件没有出现。** 插件在启用之前是关闭的。查看 `chaos plugin list`，然后把插件的名字或 ID 加进 `[plugins].enabled`，或在插件标签页对它按 `Space`。在插件标签页按 `r` 重新加载，或开始一个新会话。

**插件的技能、钩子或 MCP 服务器没有加载。** 在插件受信任之前它们保持未激活。加 `--trust` 重新安装，或把插件放到 `~/.chaos/plugins/` 下（自动信任）。见[信任与安全](#信任与安全)。

**插件市场里的某个技能或 MCP 服务器缺失。** 用 `chaos plugin marketplace update` 刷新源，确认插件已安装并启用；如果你的组织限制源，再确认该插件市场仍被允许（见[跨组织分发](#跨组织分发)）。有些 MCP 服务器需要登录，认证之前不会出现。

**配置了 MCP 服务器但它从不启动。** 可能是组织策略封禁了它。`chaos inspect` 列出 `allowedMcpServers` / `deniedMcpServers`、`mcpManagedServersOnly`、所有被锁死的策略文件，以及每个服务器的来源。拒绝匹配、未放行该服务器的允许列表/锁定、被锁死的策略文件，或项目作用域服务器上的 `enableAllProjectMcpServers = false`，都会在启动前把它丢弃。见[限制可以运行哪些 MCP 服务器](#限制可以运行哪些-mcp-服务器)。

**添加插件市场被拒绝。** 有 `strictKnownMarketplaces` 列表在生效。只有列出的 git / GitHub URL 能添加；本地路径的添加被拒绝，除非管理员的 `extraKnownMarketplaces` pin 恰好指名该路径。如果 `chaos inspect` 显示该列表已被锁死，说明键存在但为空、格式错误或只指名了不受支持的源，修复之前什么都添加不了。

**安装因未固定而被拒绝。** 你的部署要求固定 commit。安装一个确切的提交（`owner/repo@<sha>`），或使用在 `plugin-index.json` 里发布 `sha` 值的插件市场。见[要求固定版本](#要求固定版本)。

**查看确切加载了什么。** 运行 `chaos inspect`（加 `--json` 得到机器可读输出），列出每个发现的插件及其提供的技能、代理、钩子和 MCP 服务器，每项都带 `plugin: <name>` 来源标签。

---

## 参考

### 插件包含什么

一个插件是一个目录，可包含以下内容的任意组合：

- **技能**：一个由 SKILL.md 文件组成的 `skills/` 目录
- **斜杠命令**：一个 `commands/` 目录
- **代理**：一个 `agents/` 目录
- **钩子**：一个 `hooks/hooks.json` 文件
- **MCP 服务器**：一个 `.mcp.json` 文件
- **LSP 服务器**：一个 `.lsp.json` 文件

可选的 `plugin.json` 清单可以覆盖路径或添加元数据；没有它时，Chaos 会从这些标准目录发现组件。例如，一个 `team-tools` 插件可能打包一个部署技能、一个代码评审代理、pre-commit 钩子和一个 Linear MCP 服务器，一步安装到位。

技能或命令可以在其 SKILL.md 旁附带一个**辅助脚本**（例如它调用的一个 Python 文件）。把脚本放进插件，让技能按相对路径运行它；它会随插件一起复制到机器上。脚本的运行时及其导入的任何包必须已经存在，插件交付的是文件，不是运行时或原生二进制（见[这不涵盖什么](#这不涵盖什么)）。

### Chaos 在哪里查找插件

Chaos 按优先级顺序从以下位置发现插件。`.claude/plugins/` 的等价形式也可用；两个插件同名时，优先级高的获胜：

| 位置 | 作用域 | 信任 |
|----------|-------|-------|
| `_meta.pluginDirs` (`session/new` / `session/load`) | 会话，仅该会话 | 自动信任 |
| `--plugin-dir`（`chaos agent … stdio` 的标志） | 进程，仅该代理进程 | 自动信任 |
| `.chaos/plugins/` | 项目，通过版本控制共享 | 需要信任 |
| `~/.chaos/plugins/` | 用户，所有项目 | 自动信任 |
| `[plugins].paths`（配置） | 你自行添加的目录 | 取决于位置 |

`session/new` 和 `session/load` 请求上的 `_meta.pluginDirs` 字段为单个会话加载插件；因为目录由调用方提供，这些插件自动受信任，且在会话结束后不保留。`--plugin-dir` 是专用 `chaos agent … stdio` 进程的进程级等价物，可重复（`chaos agent --no-leader --plugin-dir A --plugin-dir B stdio`），在 leader 模式下被忽略——由共享的 leader 发现它自己的插件。

### 插件钩子中的环境变量

插件钩子在标准钩子环境之外还接收两个变量：

| 变量 | 说明 |
|----------|-------------|
| `GROK_PLUGIN_ROOT` | 插件安装目录的绝对路径。 |
| `GROK_PLUGIN_DATA` | 插件可写数据目录的绝对路径，用于状态、缓存和日志。 |

Chaos 会设置这两个变量，并覆盖钩子 `env` 映射里任何同名值（`CLAUDE_PLUGIN_ROOT` 和 `CLAUDE_PLUGIN_DATA` 别名也会设置）。传给钩子的每个变量见[钩子指南](10-hooks.md)。

### 键盘快捷键

这些键在插件模态的每个标签页都有效：

| 键 | 操作 |
|-----|--------|
| `Tab` / `Shift+Tab` | 下一个 / 上一个标签页 |
| `j` / `k` 或方向键 | 移动选择 |
| `Enter` | 展开或折叠选中的条目 |
| `/` | 在当前标签页按名称搜索 |
| `Esc` | 清除搜索，或关闭模态 |
