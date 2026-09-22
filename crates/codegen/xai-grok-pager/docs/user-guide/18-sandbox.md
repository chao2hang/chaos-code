# 沙箱模式

沙箱模式用操作系统内核原语（Linux 上是 Landlock，macOS 上是 Seatbelt）限制代理进程及其派生的命令能访问哪些文件和网络。这些限制由内核在整个进程生命周期内强制执行。

沙箱模式默认关闭。

---

## 快速上手

```bash
# Run with workspace sandbox (read everywhere, write to CWD + temp dirs + ~/.chaos/)
chaos --sandbox workspace

# Read-only mode (read everywhere, write only to ~/.chaos/ + temp dirs)
chaos --sandbox read-only

# Most restrictive profile (read CWD + system paths + ~/.chaos, write CWD + ~/.chaos/sessions + temp dirs, no child network)
chaos --sandbox strict
```

---

## 内置配置档

| Profile               | 文件读                        | 文件写                                       | 子网络 | 适用场景                          |
| --------------------- | ------------------------------ | ---------------------------------------------- | ------------- | --------------------------------- |
| `off`（默认）       | 不受限                   | 不受限                                   | 不受限  | 无沙箱                        |
| `workspace`           | 所有位置                     | CWD + `~/.chaos/` + `/tmp` + `/var/tmp`         | 允许       | 日常开发                |
| `devbox`              | 所有位置                     | 除 `/data` 外的所有顶层目录                     | 允许       | 用完即弃的开发虚拟机                |
| `read-only`           | 所有位置                     | `~/.chaos/` + `/tmp` + `/var/tmp`               | 已阻止¹      | 探索代码、代码审阅          |
| `strict`              | CWD + 系统路径 + `~/.chaos` | CWD + `~/.chaos/sessions` + `/tmp` + `/var/tmp` | 已阻止¹      | 不受信任的代码                    |

¹ 子进程网络阻断只在 **Linux** 上生效（通过 seccomp）——在 macOS 上它是空操作，这些配置档不会限制那里的子进程网络。

要在配置档之上再拦住特定文件（例如 `.env` 或凭据路径），可以定义一个带 `deny` 列表的[自定义配置档](#自定义配置档)——它由内核强制执行（读 + 写/改名），并支持 `**/*.pem` 这类 glob 模式。

### 各配置档详解

**workspace** —— 日常开发推荐用它。代理可以读系统上的任何文件（便于理解依赖、系统库等），但只能写入当前工作目录、`~/.chaos/` 和临时目录（`/tmp`、`/var/tmp`，以及 macOS 的临时目录）。`web_search` 这类工具与 MCP 服务器允许联网。

**devbox** —— 为用完即弃的开发虚拟机预留的内置配置档。代理可以读所有位置，也可以写入除 `/data` 和虚拟文件系统（`/proc`、`/sys`、`/dev`）之外的每个顶层目录，包括主目录。允许联网。`--sandbox devbox` 运行的是内置配置档，它会盖过你在 `sandbox.toml` 里定义的任何 `[profiles.devbox]`。

**read-only** —— 想让代理分析代码、不改动项目文件时用它。代理可以读所有内容，但只能写入 `~/.chaos/`（会话持久化需要）和临时目录。子进程网络访问在 Linux 上被阻断（macOS 上是空操作）。

**strict** —— 限制最严的配置档，用于审阅不受信任的代码。代理可以读当前工作目录、必要的系统路径和 `~/.chaos`。写入限于 CWD、`~/.chaos/sessions` 和临时目录——不是整棵 `~/.chaos` 树。子进程网络访问在 Linux 上被阻断（macOS 上是空操作）。

### 对全局直接写入路径的保护

在 `workspace`、`read-only`、`strict` 下（以及以它们为基础扩展出来的自定义配置档），内核会对 Chaos 自有的那些直接磁盘路径**拒绝写入**：它们被用作用户级钩子来源，此外还有配置与信任文件（被授予读取权限时它们仍可读）。内置的 `strict` 可以读 `~/.chaos`（这些文件保持可读）；写入限于 CWD + `~/.chaos/sessions` + 临时目录，不是整棵树。即使配置档授予了写权限，拒写依然生效：

- `~/.chaos/hooks/`（钩子目录）
- `~/.chaos/hooks-paths`（注册表文件；它本身不作为钩子 JSON 加载——被加载的只是其中列出的绝对目标）
- `hooks-paths` 里列出的绝对目标（相对路径的行会被忽略；目标缺失时拒绝启动沙箱）
- `~/.chaos/config.toml`、`~/.chaos/trusted_folders.toml`、`~/.chaos/managed_config.toml`、`~/.chaos/requirements.toml`、`~/.chaos/sandbox.toml`（设置、目录信任、受管策略、要求项和沙箱配置档）

由于这些文件在上述配置档下是只读的，本该保存到它们的改动只对当前会话生效。接受目录信任提示、用 `/model` 换模型、修改权限模式（`/auto` 或 Shift+Tab），都只对当前会话生效，不会被保存。要保存目录信任，请在启动沙箱前于该目录下运行 `chaos --trust`。要改默认模型或权限模式，请直接编辑 `~/.chaos/config.toml`。

在这些配置档下首次启动时，若 `hooks/` 目录与 `hooks-paths` 文件缺失，Chaos 会创建真实的空目录与空文件（绝不会用符号链接，也不会创建成错误的类型）。Claude/Cursor 的全局设置**不在**这条拒写规则覆盖范围内；这些厂商配置的发现仍由兼容性设置单独把关。

`$CHAOS_HOME` 本身是符号链接、或 `hooks-paths` 的某一项带有符号链接成分时，沙箱启动会被拒绝（防止目标被改写）。受保护路径已存在的父目录会被固定，使其无法在拒绝规则之下被改名（同级目录仍可写）。在 Linux 上，bubblewrap 内部的嵌套用户命名空间被禁用，因此挂载绑定无法被重排。项目钩子仍由目录信任把关。`devbox` 配置档不施加这项保护（用完即弃的虚拟机）。需要这项保护的配置档在内核策略无法生效时会拒绝启动（包括无法验证只读挂载的 Linux）。

---

## 自定义配置档

自定义沙箱配置档写在 `~/.chaos/sandbox.toml`（全局）或 `.chaos/sandbox.toml`（按项目）：

```toml
[profiles.project]
# Start from a built-in profile, then add overrides
extends = "workspace"
restrict_network = true

# Paths the agent can read but NOT write/delete
read_only = ["/data"]

# Additional writable paths
read_write = ["/tmp/scratch"]

# Paths or globs to kernel-deny (read + write/rename, enforced; see notes below)
deny = ["/data/shared-secrets", "**/.env", "**/*.pem"]
```

使用这个自定义配置档：

```bash
chaos --sandbox project
```

自定义配置档不能复用内置名称。`--sandbox devbox` 始终运行内置的 `devbox` 配置档，盖过你定义的任何 `[profiles.devbox]`。

如果用户级与项目级文件对同一个自定义配置档定义不一致，Chaos 采用用户级的那个，并在启动时给出告警。运行 `/doctor` 可以看到两个文件的位置以及如何解决冲突。定义完全相同时不会告警。

### 自定义配置档的字段

| 字段              | 类型     | 说明                                          |
| ------------------ | -------- | ---------------------------------------------------- |
| `extends`          | String   | 要继承的内置配置档（`workspace`、`devbox`、`read-only`、`strict`）。省略时默认 `workspace` |
| `restrict_network` | Boolean  | 阻断子进程的网络访问 |
| `read_only`        | String[] | 额外的只读路径 |
| `read_write`       | String[] | 额外的可读写路径 |
| `deny`             | String[] | 要由内核拒绝读取的路径或 glob（读 + 写/改名；见下方说明）。含 `*`、`?` 或 `[` 的条目按 glob 处理 |

> **关于 `read_only` / `read_write`：** 这两项是**字面目录授权**，不是 glob。
> 末尾的 `/**`（或 `/*`）会被当作父目录，所以 `…/cache/**` 授权的是
> `…/cache`（单独一个 `/**` 授权的则是 `/`）。在那之后仍含 `*`、`?` 或 `[`
> 的条目（例如 `/home/**/cache`，或名字里真的带 `dir[1]` 的目录）会被跳过，
> 并给出告警——请直接列出你需要的具体目录，或把 glob 放到 `deny` 里。
> 首尾带空白的条目同样会被跳过并告警：字面路径里的空白是有意义的，所以
> 请改正条目本身，不要指望它会被自动裁剪。

> **关于 `deny`：** 非空的 `deny` 列表由**内核强制执行**。被拒绝的路径在
> macOS 上通过 Seatbelt、在 Linux 上通过 bwrap 的 bind-over 实现
> **拒绝读取、拒绝写入/改名**，因此被拒绝的路径既读不到（无论用 `bash`、
> `grep` 还是子代理），也无法移出拒绝集合再到别处读取（`mv secret x && cat x`
> 这条绕过路径已被封堵）。在 **Linux** 上，拒绝读取依赖 `bubblewrap`：若它
> 缺失（或任意一个拒绝路径无法被绑定），Chaos 会拒绝启动，而不是把被拒绝的
> 路径暴露在外运行（`devbox` 只对 `/data` 拒写，仍会退回 Landlock）。
> 对**不在** `deny` 中的路径的写入，由你在 `read_write` 里授予的权限决定。

> **`deny` 里的 glob：** 条目只要含 `*`、`?` 或 `[` 就是 **glob**。这些字符
> **永远**表示 glob——要拒绝某个名字里含这些字符的字面文件，请改为指定它的
> 父目录。支持的 gitignore 风格子集是：
>
> - `*` —— 单个路径段内的任意字符（遇到 `/` 停止）
> - `?` —— 单个路径段内恰好一个字符
> - `**` —— 跨目录（须作为完整的路径段，例如 `**/`、`a/**`）；`**/`
>   也匹配零个目录，所以 `**/.env` 匹配 `.env` 和 `sub/.env`
> - `[abc]` / `[a-z]` —— 字符类；开头的 `!` **或** `^` 表示取反
>   （`[!a]` 和 `[^a]` 都是「不是 `a`」）
>
> 花括号展开（`{a,b}`）、反斜杠转义、空路径段（重复的 `//` 或末尾的 `/`）、
> `.` 或 `..` 段，以及两种少见的字符类写法 `[]…]`（`]` 写在最前）与 POSIX
> `[[:…:]]` 都**不支持**，这样两个平台就绝不可能对同一个 glob 作出不同解释。
> 使用不受支持元字符的 glob、或写法有误的 glob，会让 Chaos 在**两个平台上
> 都拒绝启动**（fail closed）——请把 `*.pem` 和 `*.key` 写成两条，而不要写成
> `*.{pem,key}`。
>
> 相对 glob 以工作区为锚点，绝对 glob（例如 `/home/**/.ssh`）以它的字面前缀
> 为锚点。非 glob 条目仍按精确路径匹配。相对 glob **只在工作区内**匹配。要拒绝
> 其它位置的文件，请把条目写成绝对路径。除此之外，执行方式因平台而异：
>
> - **macOS 上是严密的：** 每个 glob 会在运行时变成一条 Seatbelt 正则，所以
>   匹配的文件**即使在 Chaos 启动之后才创建**也会被拒绝。
> - **Linux 上是尽力而为：** 挂载命名空间无法在运行时做 glob，所以每个 glob
>   会展开成**启动时已存在**的文件，再绑定覆盖上去。**之后**才创建、且匹配该
>   glob 的文件**不在**覆盖范围内——在 Linux 上必须严密拦截的内容请写成精确
>   路径。被匹配的符号链接会连同它解析后的目标一起屏蔽。若某个 glob 匹配到太
>   多文件，或它所在的目录树过深过广而无法扫描，Chaos 会**拒绝启动**，而不是
>   降低强度；错误信息会指出是哪些 glob、扫描停在了哪个目录。启动扫描从每个
>   glob 的字面前缀开始，并包含被 gitignore 忽略的文件和隐藏文件，所以在很大
>   的工作区里，请优先用带锚点的 glob（`certs/**/*.pem` 只扫描 `certs/`），
>   而不要用裸 `**` 模式。

---

## 工作原理

沙箱在启动时通过内核原语施加到**整个 Chaos 进程**上——不是逐条命令地包装。因此所有工具操作都在覆盖范围内：

- `read_file`、`search_replace`、`list_dir` —— 由进程内的 Landlock/Seatbelt 限制
- `bash` 命令、`grep`（rg）—— 子进程自动继承文件系统限制
- 网络 —— 在 Linux 上可用 seccomp 阻断子进程；在 macOS 上是空操作

当**请求**了非 `off` 的沙箱配置档时（来自 CLI、`GROK_SANDBOX`、配置或受管要求项）：

- 代理**在进程内**运行，不走共享 leader，这样在配置档被强制执行时，工具调用留在这个进程里。若本来会启用 leader 模式，启动时会有一行提示说明这一点
- 内置配置档施加失败时，Chaos 会告警并在没有强制执行的情况下继续（见[平台支持](#平台支持)），但仍会拒绝 leader，以免工具被委派到别处
- `chaos workspace start`、`restart`、`resume` 不可用；`pause`、`stop`、`status` 仍可用

要使用被拒绝的那些命令，请在选中该配置档的源头把它关掉。

沙箱一旦施加就**不可撤销**。代理无法在运行时放宽限制。

---

## 恢复会话

会话启动时所用的配置档会随会话一起保存，并且在**会话存续期间固定不变**。
恢复会话时（`chaos --resume <id>`、`chaos --continue` 或 `chaos -r`），Chaos 会
自动恢复同一个配置档——因此用 `--sandbox workspace` 启动的会话不会悄悄回到更
严格的默认值上，把原本能跑的命令弄坏。

恢复**不会**改变会话的沙箱：

- 恢复时不加 `--sandbox`，沿用会话保存的配置档。
- `--sandbox <profile>` 与保存的配置档**相同**时允许。
- `--sandbox <profile>` 与保存的配置档**不同**时**报错拒绝**——改动已恢复会话的
  沙箱是个安全陷阱（它可能放宽本应被限制的访问范围，也可能弄坏依赖更宽权限的
  会话）。要换配置档请新建会话。

**新**会话的配置档解析顺序：

1. 显式的 `--sandbox <profile>` 标志，或 `GROK_SANDBOX` 环境变量
2. 配置里的 `[sandbox] profile`
3. `off`（无沙箱）

---

## 平台支持

| 平台 | 机制 | 最低版本        |
| -------- | --------- | ---------------------- |
| Linux    | Landlock  | 内核 5.13 或更高   |
| macOS    | Seatbelt  | macOS（所有版本）   |

沙箱无法施加时（例如内核不支持、缺少 entitlements），Chaos 会记录一条告警，并在没有强制执行的情况下继续。例外是显式请求的**自定义配置档**：在 **macOS 和 Linux 上都是**，若它无法施加（配置档不存在、`sandbox.toml` 格式有误，或在 Linux 上非空 `deny` 所需的 `bubblewrap` 不可用），Chaos 会拒绝启动，而不是把被拒绝的路径暴露在外运行。

---

## 网络限制

在 Linux 上，带 `restrict_network` 的配置档通过 seccomp 阻断**子进程**（bash 命令、脚本）的网络访问。在 macOS 上，网络阻断是空操作。在进程内发起 HTTP 请求的内置工具（网页搜索、LLM API 调用）从不受影响——代理需要网络才能工作。

实际效果上，在 Linux 上这意味着：

- `web_search`、`web_fetch` 和 LLM API 始终可以联网
- `curl`、`wget`、`npm install` 这类 `bash` 命令在启用 `restrict_network` 时被阻断

---

## Shell 环境策略

沙箱控制的是子进程能触达哪些文件和网络。顶层的 `[shell_environment_policy]` 表控制的则是它继承哪些环境变量，这样模型运行的某个工具命令就读不到你 shell 环境里恰好躺着的密钥。

```toml
[shell_environment_policy]
inherit = "core"                 # all (default) | core | none
ignore_default_excludes = false  # also drop *KEY* / *SECRET* / *TOKEN*
exclude = ["ACME_*", "CI_*"]     # drop these names
include_only = ["PATH", "HOME"]  # if set, keep only these names
set = { MY_FLAG = "1" }          # force these values
```

Chaos 按顺序构造子进程环境：先从 `inherit` 开始（`all` 保留全部，`core` 保留一小撮平台相关变量如 `PATH` 和 `HOME`，`none` 从空环境开始）；再丢掉内置的密钥模式 `*KEY*`、`*SECRET*`、`*TOKEN*`，除非 `ignore_default_excludes = true`；再丢掉所有匹配 `exclude` 的名字；然后应用 `set`；最后在 `include_only` 非空时只保留匹配的名字。模式是不区分大小写的 glob（`*`、`?`）。

默认值（`inherit = "all"`、`ignore_default_excludes = true`）不改动环境，所以在你配置策略之前什么都不会变。在非持久化后端上，该策略还会过滤从你登录 shell 捕获的变量，因此 `.rc` 文件里的 export 无法把密钥偷偷带过 `exclude` 或 `include_only`。持久化 shell 是一个例外：策略会作用于它的基础环境，但 `.rc` 文件在登录期间导出的变量是从快照回放的，不会重新过滤——所以在这种 shell 下不要把密钥放进启动文件。在 macOS、Linux 和 Windows 上，强制范围覆盖 bash 工具与各终端。

---

## 事件日志

沙箱事件会记录到 `~/.chaos/sessions` 便于排查。事件包括：

- 已施加的配置档（哪个配置档、时间戳）
- 违规（尝试访问被拒绝的路径）

---

## 何时使用沙箱模式

**在以下情况使用 `workspace`：**

- 做自己的项目，只想要基本的写入保护
- 在共享环境里运行，想限制改动的范围

**在以下情况用 `deny` 列表定义自定义配置档：**

- 需要在某个基础配置档之上再拦住特定文件（例如 `.env` 或凭据路径）
- 需要的内核级强制要覆盖 `bash`、`grep` 和子代理——而不只是 `read_file` 工具

**在以下情况使用 `read-only`：**

- 审阅你不信任的代码
- 探索一个代码库，但不想冒误改的风险
- 做代码分析或审计

**在以下情况使用 `strict`：**

- 分析不受信任的或第三方的代码
- 在安全敏感的环境里运行
- 想要最大程度的隔离

**在以下情况不用沙箱：**

- 代理需要安装依赖（`npm install`、`pip install`）
- 代理需要改动工作目录之外的文件
- 你在可信环境里工作，想要最大的灵活性

---

## 取舍

| 方面      | 无沙箱            | 有沙箱                    |
| ----------- | -------------------------- | ------------------------------- |
| 安全      | 代理拥有完整的系统访问权限 | 代理受配置档规则限制 |
| 能力  | 什么都能做            | 受配置档限制              |
| 性能 | 无额外开销                | 开销可忽略             |
| 恢复    | 只能信任代理       | 由内核强制边界      |

沙箱在操作系统层面施加限制——Linux 上通过 Landlock 或挂载命名空间，macOS 上通过 Seatbelt——而不是另起一个虚拟机。
