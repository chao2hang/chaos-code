# 状态栏

分页器底部的一个可选行——全屏时位于快捷键栏上方，最小模式下位于提示框信息行下方——默认关闭。它显示实时会话上下文，例如模型、上下文窗口占用、开销、目录和 git 工作树，或你配置的任意脚本的输出。在 `~/.chaos/config.toml`（或兼容的 `~/.grok/config.toml`）里设置 `[ui.status_line]` 即可启用。

## 设置

### 内置

```toml
[ui.status_line]
type = "builtin"
items = ["cwd", "model", "context"]   # default when omitted
```

例如它会渲染成 `grok-shell-status-line │ Grok 4.5 │ 12% ctx`。条目按你列出的顺序出现，过长的会以 `…` 省略：目录和会话名在 40 列处截断，模型在 30 列处截断。

| 条目 | 显示 |
| --- | --- |
| `cwd` | 当前目录（basename） |
| `model` | 模型显示名 |
| `context` | 上下文窗口百分比；到达自动压缩阈值时呈琥珀色，代理未报告阈值时则在 80% 处呈琥珀色 |
| `cost` | 会话开销；低于 $0.005 时隐藏，因此绝不会显示有误导性的 `$0.00` |
| `turn-timer` | 正在进行的回合的已用时间，从一秒起显示 |
| `session-name` | 会话名（已设置时） |

### 命令

把 `command` 指向一个脚本。Chaos 通过 stdin 把 [JSON](#可用数据) 管道给它，并显示其 stdout。`~/` 前缀会展开为你的主目录。

```toml
[ui.status_line]
type = "command"
command = "~/.chaos/statusline.sh"
```

字段名与嵌套遵循通用的状态栏约定，因此移植的脚本通常只需小改而非重写。下表未列出的内容一律不发送。

### 禁用

`type = "disabled"`（默认）不显示任何内容；`off`、`none` 和 `hidden` 都被接受为 `disabled` 的写法。

### 选项

| 键 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `type` | string | `disabled` | `builtin`、`command` 或 `disabled`。 |
| `items` | array | `["cwd", "model", "context"]` | 内置条目，按此顺序。 |
| `command` | string | none | `type = "command"` 用的脚本。 |
| `padding` | integer | `0` | 水平留白，每侧字符数，上限 16。若留白大到一列都不剩，则保留该行但不在其中绘制任何内容。 |
| `refresh_interval` | integer | unset | 仅用于 `command` 行，单位秒，取值 1 到 86,400。即使没有任何变化，也按此间隔重新运行脚本，这样闲置的会话仍能反映出变化——一次事故页面、一个 CI 状态。不设置则该行保持事件驱动。它调度的那次运行携带 `"trigger": "refresh_interval"`，其失败时保留上一次输出而不绘制错误（见[定时刷新](#定时刷新)）。调用网络的脚本应选更长的间隔，并在 `state` 运行时读取缓存。 |

## 工作原理

- **刷新。** 会话状态变化时（会话启动、回合结束、模型或努力级别切换、HEAD 移动、压缩、客户端接入）该行会更新，回合进行期间持续更新，而不是靠定时器。闲置的会话不会重新运行你的脚本，因此其中的时钟不会自己走——除非你设置 `refresh_interval`，它在上述一切之上再加一个定时器。这些更新以固定的 300 ms 去抖，因此繁忙的回合无法每一帧都运行你的脚本；必须立即反映的变化（一次缩放、一个新快照、切换代理）只等待 100 ms。已经在进行的运行绝不会被取消：下一个变化等它结束。Chaos 在启动时读取 `[ui.status_line]`，对它的修改要到下次启动才生效。
- **输出。** 你打印的每一行成为该行的一行，最多五行，每行在 1024 个字符处截断，ANSI 转义序列本身也计入，因此颜色繁多的行留给文本的空间更少。终端太矮时行数更少，多余的自底部丢弃。支持 ANSI 颜色；其余所有转义（光标移动、行清除、回车覆写）一律丢弃。OSC 8 超链接对 `http`、`https` 和 `mailto` 目标有效，其他目标渲染为纯文本。stdout 超过 64 KiB 会被截断并停止脚本。成功但什么都没打印的脚本会拿走该行，而不是回退到内置条目，因此只在某些时候打印的脚本会在出现和消失之间让转录区移动一行。
- **尺寸。** Chaos 把 `COLUMNS` 和 `LINES` 设为你的输出所填充的行，而不是窗口大小：面板内边距和你的 `padding` 已经扣除。`tput` 报告的也是这些值，因为它在 stdout 不是终端时读取它们。`LINES` 是该行当前填充的行数而不是它可能增长到的行数，因此在你打印更多内容之前它一直是 `1`；无论它显示什么，上限都是五。在该行首次绘制之前，以及在没有空间容纳它的帧上，尺寸是该行上次绘制时的值；从未绘制过则为 80x1。
- **Shell。** `command` 是一条 shell 命令行，因此 `jq -r '…'` 和管道按原样工作；路径在指向可执行文件时直接运行，否则通过 `sh -c` 运行——后者正是 `#!` 行缺失或写错时脚本的运行方式。包含空格的路径要像在提示符下那样加引号。每次运行都是全新进程，因此对脚本文件的修改在下次运行时生效。
- **后台工作不会存活。** 运行结束时，脚本留下的任何仍在运行的进程都会被杀死，每条路径都是如此：正常退出、超时、输出过多。运行在脚本退出时即告结束，因此后台作业在那之后打印的任何内容都会丢失。
- **环境。** 脚本依次在会话的工作目录、仓库根目录、分页器自身目录中运行，以第一个是本地路径的为准，超时 10 秒，超时后该行显示 `[status line: timed out]`。`COLUMNS` 和 `LINES` 描述的是脚本填充的行，不是窗口。不运行任何 shell rc 文件（`BASH_ENV` 和 `ENV` 已清空），且 `GIT_OPTIONAL_LOCKS=0`。分页器和编辑器的中和方式与 Chaos 其余部分相同，因此脚本里的 `git` 或 `gh` 调用不会阻塞等待它们。
- **输入。** JSON 负载写入 stdin 时带一个尾随换行，因此 `read -r line` 和 `input=$(cat)` 都可用。

## 定时刷新

在 `command` 行上设置 `refresh_interval` 后，脚本还会按定时器重新运行，这样事故页面或 CI 状态就能在会话闲置期间到达该行：

```toml
[ui.status_line]
type = "command"
command = "~/.chaos/statusline.sh"
refresh_interval = 300   # seconds
```

- **负载会说明脚本为何运行。** 响应定时器的那次运行携带 `"trigger": "refresh_interval"`——定时器尚未触发期间到来的状态变化会搭乘这次运行——不欠触发的运行则携带 `"trigger": "state"`。在 `refresh_interval` 时访问网络、在 `state` 时读缓存，否则繁忙回合——它会持续重新运行脚本——会对脚本调用的对象形成请求风暴。
- **负载是 Chaos 发送的最后一份。** 定时器运行用上一次状态变化时的负载重新运行你的脚本，因此其中的会话数字——开销、上下文、token——停留在那次变化时，而不是触发时。只有脚本自己取到的数据是新的。
- **刷新失败保留上一次输出。** 一旦你的脚本给出过应答——打印了一行，或刻意什么都不打印——失败或超时的定时器运行会让该行保持原样，无论那是上一次输出还是一次状态运行已绘制的失败，并把失败写入 `~/.chaos/logs/unified.jsonl`，这样不稳定的端点不会在安静的夜里画上一个错误。连续三次刷新失败意味着脚本本身坏了，错误终究会显示出来；在脚本尚未应答任何东西之前的刷新失败——新会话，或刚切换代理之后——也会立即绘制，因为没有什么可保留。由会话状态触发的运行照旧立即报告其失败。
- **错过的触发会合并。** 当该行隐藏（全屏子代理视图、欢迎界面）或一次运行已占用槽位时，触发会等待，该行在可以运行时补一次运行——绝不会为挂起或长回合跳过的那些触发补一串。无论脚本运行多久，定时器保持自己的节奏：运行尚未结束时到期的触发会顺延到下一次运行，而不是在其后堆积。
- **定时器属于运行脚本的模式。** `builtin` 下的 `refresh_interval` 不调度任何东西，`chaos inspect` 会报告这一点；`disabled` 下它随其他一切一起关闭。

## 可用数据

移植脚本时请仔细阅读这些说明。`workspace.repo_root` 是仓库根目录，没有 `project_dir`——那个名字在别处指启动目录。`context_window.session_usage` 和 `session_*` 的 token 计数是整个会话的累计值，不是单次调用的，而实时窗口是 `context_window.context_tokens`。没有额外会话目录的列表，因为 Chaos 没有这个概念。`transcript_path` 指的是 Chaos 自己的更新流，不是其他工具格式的转录，`prompt_id` 只在回合进行期间出现。以上每种情况里，移植的脚本读到的是空而不是错误答案，所以要对你用到的字段做好保护。

下表之外的内容一律不发送。移植的脚本若去读代理改动行数的计数、速率限制摘要、编辑器模式、思考或快速模式标志、输出风格、拉取请求、额外会话目录、或工作树的创建来源目录，都会发现它们不存在：每一项要么是 Chaos 没有的功能，要么是它无法如实提供数字的量。

| 字段 | 说明 |
| --- | --- |
| `cwd`, `session_id` | 工作目录和唯一会话 id |
| `session_name` | 会话的标签页名，由客户端填写。出现在 `command` 的 stdin 中，`SessionStatus` 通知里没有 |
| `prompt_id` | 正在处理的提示的 UUID。仅在回合进行期间出现 |
| `transcript_path` | 会话的 `updates.jsonl` 的路径。该文件是 Chaos 自己的更新流，因此解析其他工具转录格式的脚本读不了它 |
| `model.id`, `model.display_name` | 模型标识符和显示名。代理无法读取会话的模型时省略 |
| `workspace.current_dir` | 当前目录 |
| `workspace.repo_root` | 仓库根目录，仓库之外省略。不是 `project_dir`——那个名字在别处指启动目录 |
| `workspace.branch` | 检出的分支，任意仓库中都有。分离 HEAD 时省略 |
| `workspace.git_worktree` | 工作树名，位于链接工作树内时 |
| `workspace.repo.{host,owner,name}` | 在 git 仓库内从 `origin` 远端解析。没有 owner 段的远端会省略 `owner` |
| `schema_version` | 负载结构的修订号。新增字段不会提升它；删除或改变字段类型会。用 `>=` 测试它，并依据它而非 `version` 分支 |
| `version` | Chaos 版本，用于显示 |
| `cost.total_duration_ms` | 本进程接入会话以来的毫秒数。恢复的会话从恢复时刻起算，其开销亦然 |
| `cost.total_cost_usd`, `cost.total_api_duration_ms` | 会话开销和 API 等待毫秒数。会话中尚无任何带价格的内容时开销缺失，用量账本不可读时也缺失，因此把缺失的开销当作未知而不是零 |
| `context_window.context_window_size` | 最大上下文尺寸，单位 token。在模型窗口已知之前省略 |
| `context_window.context_tokens` | 对话当前占用的 token，只计输入，因此压缩后会下降。代理无法读取计数时省略，所以 `0` 永远意味着空上下文 |
| `context_window.session_input_tokens`, `.session_output_tokens` | 按整个会话计费，因此只增不减。以会话命名是因为它们计的就是会话：`total_*` 在别处指当前窗口内的内容，在这里即 `context_tokens`。用它们除以 `context_window_size` 会超过 100% 并继续增长。用量账本不可读时省略 |
| `context_window.used_percentage`, `.remaining_percentage` | 窗口当前的填充程度，0 到 100 的整数。与 `context_window_size` 或 `context_tokens` 一起省略，因为未知窗口的百分比不是数字 |
| `context_window.session_usage.{input_tokens,output_tokens,cache_creation_input_tokens,cache_read_input_tokens}` | `input_tokens`、`cache_creation_input_tokens` 和 `cache_read_input_tokens`（三者之和回到 `session_input_tokens`），外加 `output_tokens`。整个会话的累计值，不是单个回合的。首次调用之前缺失 |
| `context_window.auto_compact_threshold_percent` | 会话自动压缩的位置。代理未报告时省略 |
| `effort.level` | 推理努力级别，模型支持时才有 |
| `turn.started_at_ms` | 当前回合开始的 Unix 毫秒数，回合之间缺失。用你自己的时钟减去它即得已用时间 |
| `worktree.{name,path,branch,main_worktree_root}` | 当前工作树，位于链接工作树内时。位于文件系统根的工作树会省略 `name`，`main_worktree_root` 是该工作树的分叉来源 |
| `trigger` | 这次运行为何被调用：定时器请求的运行为 `refresh_interval`，否则为 `state`。出现在 command 行的 stdin 中，`SessionStatus` 通知里没有——后者描述的是会话而非一次运行 |

Chaos 无法如实提供的数据一律省略而不是以占位符发送，因此该行绝不会显示编造的值。务必对它们做好保护：`jq -r` 对缺失的键会打印字面文本 `null`，所以在 jq 里写 `// 0` 或 `// "?"`，在 JavaScript 里写 `?.`。

## 示例

保存一个脚本（例如 `~/.chaos/statusline.sh`），用 `chmod +x` 赋予可执行权限，然后把它设为 `command`。这个例子用了 [`jq`](https://jqlang.org/)；Python 和 Node.js 原生就能解析 JSON。负载里不带脏文件计数，所以脚本为此调用 `git`。

```bash
#!/bin/bash
input=$(cat)
DIR=$(echo "$input" | jq -r '.workspace.current_dir')
MODEL=$(echo "$input" | jq -r '.model.display_name // "?"')
PCT=$(echo "$input" | jq -r '.context_window.used_percentage // 0')
BRANCH=$(echo "$input" | jq -r '.workspace.branch // "detached"')
DIRTY=$(git diff --numstat 2>/dev/null | wc -l | tr -d ' ')
printf '%b\n' "${DIR##*/} │ $MODEL │ ${PCT}% ctx │ \033[32m$BRANCH\033[0m ~$DIRTY"
```

## 提示

- 用模拟输入测试：`echo '{"session_id":"t","workspace":{"current_dir":"/tmp/demo"},"model":{"display_name":"Grok 4.5"},"context_window":{"used_percentage":25}}' | ./statusline.sh`
- 把 `git status` 这类慢命令缓存到以 `session_id` 为键的临时文件，每隔几秒刷新。`session_id` 在单个会话内稳定，且跨会话唯一。
- 用 `printf '%b'` 而不是 `echo -e`，转义更可靠。

## 故障排查

- **什么都不显示。** Chaos 在启动时读取 `[ui.status_line]`，因此编辑 `config.toml` 后要重启。重启就足够了：新客户端接入时，代理会为仍在运行的会话打开该行。该行只在代理视图激活后才渲染，因此在欢迎界面上、或全屏子代理视图打开时不渲染。检查 `type` 不是 `disabled`，且 command 脚本可执行并写入 stdout。
- **行内出现一条消息。** 以 `[ui.status_line]` 开头的行意味着 Chaos 无法按所写内容使用该节：它要么指出读不了的键，要么指出你选的模式还缺什么。`chaos inspect` 列出同样的问题，包括这个版本不认识的键——该行被关闭时先看这里。它能读的部分仍然生效，且 Chaos 保留你写的原样，不会重写一个它读不了的节。设置 `type = "disabled"` 可移除该行和这条消息。
- **永远空着的行。** 代理没有发送状态更新，这通常意味着某个 `chaos` 或 leader 进程比这个客户端旧。重启 leader 或更新 Chaos。
- **只有你自己的配置能设置它。** `command` 行会运行一个程序，因此它只从你的 `~/.chaos/config.toml` 和管理员管理的配置中读取。仓库无法设置它：仓库本地的 `.chaos/config.toml` 只为 MCP 服务器读取，`[ui.status_line]` 不在任何项目级配置层能提供的键之列，因此克隆仓库无法让 Chaos 运行其中的脚本。
- **推送的配置没有生效。** `[ui.status_line]` 会从 campaign 和 version-override 补丁中剥离，因为状态栏可以指定一条会在你机器上运行的命令。请在自己的 `config.toml` 里设置。
- **错误。** 脚本打印的任何内容都会显示，即使它以非零退出，因此 `printf …; [[ -n $dirty ]]` 的行为和预期一致。什么都不打印且失败的脚本显示 `[status line: exit N]`，并保持到下一次运行成功为止——会话状态触发的运行会立即报告其失败；定时器运行的失败则保留上一次输出（见[定时刷新](#定时刷新)）。脚本的 stderr 永远不会画到行上，因此用于调试的 `echo` 不会打扰该行；带 `--debug` 运行 Chaos 即可读到它。Chaos 完全无法启动的脚本显示 `[status line: could not start the script: …]`，没有可执行位的文件正是这种结果；被系统杀死的脚本显示 `[status line: killed by signal]`。`#!` 行指定的解释器不存在时会改用 `sh` 重试，因此显示的是退出码。
