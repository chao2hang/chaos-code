# 后台任务与监控

Chaos 在不阻塞对话的前提下运行长时进程。本文覆盖后台命令、`/loop` 命令、`monitor` 工具与调度器。

---

## 后台命令

在 `run_terminal_command` 工具上设置 `background: true`，即可让命令在后台运行。它会立即返回一个任务 ID；用 `get_command_or_subagent_output` 取回输出。

### 工作原理

1. 代理以 `background: true` 调用 `run_terminal_command`。
2. 命令在后台启动。
3. 代理收到一个 `task_id`，供之后引用。
4. 命令完成时，对话中出现一条通知。

### 获取输出

用 `get_command_or_subagent_output` 检查后台命令或子代理。`task_ids` 以列表传入（单个 id 就是单元素数组；最多 20 个）：

- 省略 `timeout_ms`，或传 `0`，得到非阻塞快照。
- 正的 `timeout_ms` 会等待完成。多个 id 会等到**全部**完成。

正的 `timeout_ms` 会被截断到 **1 小时**（`3600000` 毫秒）以内。传输层截止时间更短的主机可设置 `GROK_MAX_WAIT_BLOCK_MS`（纯毫秒数；无法解析的值保持默认）。

若等待返回时子进程仍在运行，别去动它：既不要杀死它，也不要让它停止。完成时会自动唤醒父进程。只有在还需要一份新快照时才再次轮询。

### 终止后台任务

用 `kill_command_or_subagent(task_id)` 终止正在运行的后台任务或子代理。该工具对 shell 进程先发 SIGTERM、再发 SIGKILL，对子代理发送 Cancel 和 Shutdown。任务被杀死或早已退出时都报告成功。

### 常见用法

- **开发服务器**：启动开发服务器，继续写代码
- **测试套件**：在后台跑测试，同时修复问题
- **构建流程**：先启动构建，稍后查看结果
- **长编译**：先启动编译，同时做其他事

---

## 把运行中的任务转到后台

在交互式 TUI 里，按 `Ctrl+B` 可把正在前台运行的命令转到后台。这是唯一的后台化快捷键，不过命令执行中途发送新消息也会把该命令转入后台而不是杀死它。以下情况适合这么做：

- 命令耗时超出预期。
- 你想在命令运行期间问代理别的事。
- 进程启动后你才发现它是长时任务。

任务会继续运行，完成时你会收到通知。

---

## /loop 命令

`/loop` 按固定间隔重复运行一个提示。它适合轮询任务、周期检查和持续监控。

### 语法

```
/loop [interval] <prompt>
```

支持的间隔格式：

| 格式  | 示例    | 说明               |
| ------ | ------- | ------------------ |
| `Ns`   | `60s`   | 每 N 秒（最小 60）|
| `Nm`   | `5m`    | 每 N 分钟          |
| `Nh`   | `2h`    | 每 N 小时          |
| `Nd`   | `1d`    | 每 N 天            |

### 示例

```
/loop 5m Check if the test suite passes and report any failures
/loop 2h Summarize new commits since the last check
/loop 60s Check if the dev server at localhost:3000 is responding
```

### 行为

- 提示在创建时立即触发一次，之后按指定间隔重复
- 每次触发都在一个分离的后台子代理中运行，而不是作为你对话中的一个回合。触发看不到对话内容，所以存储的提示必须自含；只有结果会传回
- 周期任务 7 天后自动过期
- 最多可同时有 50 个活跃的定时任务

---

## monitor 工具

`monitor` 工具从长时运行的脚本流式读取事件。输出的每一行都成为对话中的一条通知。`monitor` 工具是 `/loop` 的流式对应物：周期检查用 `/loop`，实时事件流用 `monitor`。

### 工作原理

1. 你提供一个 shell 命令（`command`）和一条简短的 `description`，后者出现在每条通知里。
2. Chaos 把命令的 stdout 与 stderr 合并进同一个输出文件。
3. 该文件中的每个新行都成为一条通知，送达到对话。
4. 监视器一直运行，直到命令退出或你停止它。

### 脚本编写准则

- **管道里一定用 `grep --line-buffered`。** 不用的话，管道缓冲会把事件延迟数分钟。
- **轮询循环里处理瞬时失败**（`curl ... || true`）。一次失败的请求不应让监视器停止。
- **使用有选择性的过滤器。** 每一行都会成为一条消息，所以绝不要直接管道原始日志。
- **轮询间隔要与来源匹配。** 远程 API 用 30 秒或更长以尊重速率限制，本地检查用 0.5 到 1 秒。
- **stdout 与 stderr 都会产生事件。** 把不想作为事件的输出重定向掉——例如追加 `2>/dev/null`——或把它过滤掉。

### 示例

```bash
# Watch for errors in a log file
tail -f /var/log/app.log | grep --line-buffered "ERROR"

# Monitor file changes in a directory
inotifywait -m --format '%e %f' /watched/dir

# Poll GitHub for new PR comments
last=$(date -u +%Y-%m-%dT%H:%M:%SZ)
while true; do
  now=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  gh api "repos/owner/repo/issues/123/comments?since=$last" \
    --jq '.[] | "\(.user.login): \(.body)"'
  last=$now; sleep 30
done
```

### 持久监视器

对应当在整个会话期间存活的监视器，设置 `persistent: true`：

- PR 监控
- 日志跟踪
- CI 状态观察

用 `kill_command_or_subagent(task_id)` 停止持久监视器。

### 事件量控制

若某个监视器产生的事件过多，Chaos 会自动停止它。此时请用更紧的过滤器重启监视器。优先用 `grep --line-buffered`、`awk`，或只输出你关心事件的包装脚本。

---

## 调度器

调度器提供了创建周期任务的更底层 API。`/loop` 是对调度器的便捷封装。

### scheduler_create

创建一个定时任务：

| 参数             | 说明                                                       |
| --------------- | --------------------------------------------------------- |
| `interval`      | 运行频率：`"5m"`、`"2h"`、`"1d"`、`"60s"`                   |
| `prompt`        | 每次触发要执行的提示文本                                    |
| `fire_immediately`| 除间隔外，创建时也立即触发一次（默认：`false`）              |
| `recurring`     | 重复运行（默认：`true`）或只触发一次（`false`）             |
| `durable`       | 跨会话持久保留（默认：`false`）                             |

每次触发都在一个分离的后台子代理中运行；没有让它作为对话中回合运行的选项。

### scheduler_list

列出所有活跃的定时任务，含其 ID、提示、间隔和下次触发时间。

### scheduler_delete

按 ID 取消一个定时任务。任务被找到并移除时返回成功。

---

## 任务面板

在交互式 TUI 里，按 `Ctrl+G` 可切换任务面板。该面板在一个视图中列出：

- 运行中的子代理及其进度
- 活跃的后台任务及其状态
- 监视器与 `/loop` 任务，各带一个实时行数徽标
- 每个条目的任务 ID

若要切换的是提示队列，按 `Ctrl+;`。

---

## 仍在运行状态栏

当代理看似空闲而后台工作仍在运行时——回合之间，或某个回合正阻塞在可被用户打断的等待上——提示框上方会出现一条常驻状态栏：

```
◎ 1 command · 2 monitors · 1 loop · 1 subagent still running
```

它统计正在运行的后台命令、监视器、`/loop` 定时任务和后台子代理，并随每项完成实时更新。其中任何一项都能唤醒代理开启新回合（命令与子代理在完成时，监视器在事件时，loop 在计时器到点时），所以这条提示会一直挂着，直到什么都不剩。运行计数只存在于这条状态栏上：完成在转录里只是一枚单独的“Task completed”徽片，而“Worked for”标记保持朴素——转录从不重复或复述运行计数。

当某个回合正等待后台工作（阻塞在 `get_command_or_subagent_output`）时，状态栏会加一条提示：输入会立即接管：

```
◎ 1 command still running · send a message to interrupt
```

代理在等待某个没有实时计数器的东西（一次 sleep，或已经完成的工作）时，同样的提示显示为 `◎ waiting · send a message to interrupt`。发送消息会打断等待，立即运行你的消息。转录始终保持惯常形态：回合结束时一枚“Worked for”标记。当一次完成唤醒代理并让它回复时，那条回复有它自己的“Worked for”标记；代理静默应答的唤醒在转录里不留痕迹——除非它失败，那样即使静默唤醒也会出现一行“Turn failed”，所以一条常驻指令永远不会在无形中停止执行。

---

## 用法与模式

### 开发服务器 + 编码

在后台启动开发服务器，继续写代码：

```
Start the dev server with `npm run dev` in the background, then implement the login form.
```

代理以 `background: true` 运行开发服务器，并继续写代码。服务器启动时，你会看到一条通知。

### 持续测试监控

```
/loop 5m Run the test suite and report any new failures since the last run
```

每 5 分钟，代理运行一次测试，只报告新增的失败。

### 日志监控

用 `monitor` 观察特定事件：

```
Monitor the application log for ERROR and WARN entries. Use:
tail -f /var/log/app.log | grep --line-buffered -E "ERROR|WARN"
```

每个错误或警告都会作为一条通知出现在对话中。

### CI 流水线观察

```
/loop 2m Check the status of the GitHub Actions run for this PR. Report when it completes.
```

---

## 最佳实践

- **一次性长命令用 `background`**（构建、测试套件、服务器启动）
- **周期检查用 `/loop`**（CI 状态、测试运行、健康检查）
- **实时事件流用 `monitor`**（日志跟踪、文件监视）
- **延迟的一次性任务用 `scheduler_create` 加 `recurring: false`**
- **监视器过滤器要收紧** —— 优先 `grep --line-buffered`，不要原始日志流
- **不要用 sleep 循环**在普通命令里轮询 —— 改用带 `timeout_ms` 的 `get_command_or_subagent_output`
- **设置合理的轮询间隔** —— 远程 API 用 30 秒以上以避免速率限制，本地检查用更短的
