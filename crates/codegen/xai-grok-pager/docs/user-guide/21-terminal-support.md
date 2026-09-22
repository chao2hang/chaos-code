# 终端支持与故障排查

Chaos 以全屏 TUI 运行，依赖终端提供颜色、剪贴板、键盘输入、鼠标输入与全屏
显示能力。不同的终端、多路复用器、容器与 SSH 会话对这些特性的支持各有差异。

## 诊断并修复终端问题

在 Chaos 里运行 `/doctor` 检查当前会话并查看可用修复。如果 Chaos 无法启动，
在 shell 里运行 `chaos doctor`；需要机器可读的报告时用
`chaos doctor --json`。

Doctor 会检查终端、多路复用器、颜色支持、键盘与换行行为、剪贴板路由，以及
（在包含音频采集的构建里）麦克风可用性。应用内命令还能检查实时会话细节，
例如通知焦点跟踪与沙箱 profile 冲突。

报告可以既包含问题或建议、又以成功状态退出。`chaos doctor --json` 在被管道
接管时报告相同的颜色能力。麦克风检查不会真正开始录音，因此 Doctor 无法检测
只在采集期间表现为静音的 macOS 权限失败。

`/terminal-setup`、`/terminal-check` 与 `/terminal-info` 仍是 `/doctor`
的别名。

当 Doctor 发现某个明确的 tmux 不健康设置时，`/doctor fix` 会列出可用的自动
修复。一次只应用一个指名修复，例如 `/doctor fix tmux-clipboard` 或
`chaos doctor fix dcs-passthrough --yes`。Doctor 可以持久化以下四个 tmux
选项：

- `terminal.tmux-clipboard` — `set -g set-clipboard on`
- `terminal.dcs-passthrough` — `set -wg allow-passthrough on`
- `terminal.tmux-extended-keys` — `set -g extended-keys on`
- `terminal.tmux-truecolor` — `set -as terminal-features ",*:RGB"`

tmux 修复只编辑承载受影响 tmux 服务器的那台机器上的持久配置（含远程会话）。
普通 tmux 使用真实的 `$HOME/.tmux.conf`；Byobu-tmux 使用其生效的
`BYOBU_CONFIG_DIR`，在该目录不可用或不安全时拒绝猜测。Chaos 会保留文件的
行尾符与权限模式，修改既有文件时先做备份，并拒绝有冲突或含义不明的直接赋值。

Chaos 刻意**不**执行 `tmux source-file`，也不改动运行中的 tmux 服务器。请用
应用后给出的那条命令原样重载，或脱离再重新接入，然后再跑一次 `/doctor`。
在重载之前，实时检查项保持原样是预期行为。保守的配置扫描只检查直接全局
赋值；sourced 文件、条件分支、插件和生成的 tmux 配置请自行审阅。

---

## 已识别的终端

Chaos 根据环境变量识别这些终端模拟器：

- **Apple Terminal**
- **Ghostty**
- **iTerm2**
- **Warp**
- **WezTerm**
- **Kitty**
- **Alacritty**
- **Rio**
- **foot**（Wayland 原生，Linux）
- **VS Code**、**Cursor**、**Windsurf** 与 **Zed** 集成终端
- **JetBrains** IDE 终端
- **Chaos Desktop**
- 基于 **VTE** 的终端，如 GNOME Terminal、GNOME Console 与 Tilix
- **Windows Terminal**

识别有以下限制：

- 在 tmux 内部，标识外层终端的变量可能传达不到 Chaos。
- 通过 SSH 时，许多终端变量不会被转发。
- tmux 的全局环境反映的是第一个接入该服务器的客户端，不一定是当前终端。

---

## 常见问题与修复

### 颜色不对或缺少真彩色

运行 `/doctor`。完全受支持的配置会显示 `color truecolor` 与 `themes all`。
否则 Doctor 会显示检测到的限制与相应修复。

在 tmux 里有两个独立的问题：Chaos 发出什么颜色，以及什么颜色能穿过复用器。
`color` 行回答第一个问题。至于第二个：当接入的客户端未被标记为 `RGB` 时，
tmux 会把每个 24-bit 颜色改写为外层终端 terminfo 所声明的最接近颜色，可能
少到只有八种。此时即使 `color` 读数是 `truecolor`，主题看起来仍然发灰。
Doctor 将其报告为 `terminal.tmux-truecolor`。重载 tmux 配置并脱离后重新
接入：服务器只在重载时读取新选项，客户端只在接入时修正颜色深度，所以单独
任一步骤都不会带来变化。

### 剪贴板问题

Chaos 最多通过三条路由写入，`/doctor` 的**剪贴板**部分会展示：

- **native** — 本地操作系统的剪贴板。
- **tmux** — Chaos 运行在 tmux 内时使用的 tmux 粘贴缓冲区。
- **OSC 52** — 一种可以穿过 tmux、容器或 SSH 的转义序列。

#### Wayland

现代 Wayland 合成器无需保持终端聚焦即可更新剪贴板；较老的合成器可能要求
Chaos 保持聚焦，直到复制消息出现。当适用这种情况时 Chaos 会在启动时给出
警告；运行 `/doctor` 查看检测到的状态与步骤。

`GROK_CLIPBOARD_NO_DATA_CONTROL=1` 是禁用 data-control 路由的高级回退手段，
此时复制改用命令行剪贴板工具。

#### OSC 52 关闭开关

当该路由启用时，Chaos 在 Linux 上以及穿越 tmux、SSH 或无显示容器时发出
OSC 52。未实现 OSC 52 的终端可能把编码后的载荷当作文本显示。在启动
Chaos 之前设置 `GROK_CLIPBOARD_NO_OSC52=1` 可禁用该路由；`/doctor` 随后显示
`osc 52 off`，native 与 tmux 路由不受影响。

#### Linux X11 选区

X11 的 **PRIMARY** 与 **CLIPBOARD** 是相互独立的：

- 不带修饰键的中键点击只在 `DISPLAY` 已设置时读取 PRIMARY。在 XWayland 下，
  `xclip` 或 `xsel` 必须位于 `PATH` 中。
- `Ctrl+V` 读取 CLIPBOARD，绝不回退到 PRIMARY。
- `Shift+Insert` 仍然是终端自身的选中文本粘贴。

#### SSH 与选中文本

远程的 Chaos 进程通常无法读取本地终端的选区。请使用终端原生的
`Shift+Insert`，或在终端用「中键点击时按住 `Shift`」这一手势绕过鼠标上报。

当 Chaos 无法通过 SSH 识别外层终端时，它会预测 OSC 52 将被发送，但把该路由
标记为未验证。复制提示会给出备份文件名，以便你找回文本。运行 `/doctor`
查看其它复制选项。

#### 通过 SSH 使用 Apple Terminal

Apple Terminal 不支持 OSC 52，因此远程复制无法到达本地剪贴板。每次复制仍会
保存到备份文件（默认 `~/.chaos/last-copy.txt`，可用 `GROK_COPY_FILE`
覆盖）；当投递未验证或剪贴板不可达时，提示会给出该路径。你也可以用
`/copy <file>` 或 `/minimal`。

若要直接转发剪贴板，请在本地机器上通过 `chaos wrap` 运行 SSH 命令，例如
`chaos wrap ssh user@host`。同一条命令也可以包装容器与 pod shell，并且会在
连接断开后恢复终端模式。

当 SSH 会话没有使用 `chaos wrap` 时，Chaos 会显示一次性提示
“Run `/doctor` for details and fixes.”。会话通过 wrap 启动后该提示不再出现。
关闭它的方式：`/settings` → **显示情境提示** → **SSH 包装**，或在
`$CHAOS_HOME/config.toml` 的 `[ui.contextual_hints]` 下设
`ssh_wrap = false`。该设置不会隐藏 Doctor 的建议。

对于反复的 SSH 使用，Doctor 提供 `chaos doctor fix ssh-wrap`。它还会显示
一次性命令、将被修改的文件，以及应当绕过该别名的情况。ID
`terminal.ssh-wrap` 仍然被接受并出现在 JSON 里。

> **警告**：`chaos wrap` 是实验性功能，未必在所有环境下可用。

#### iTerm2

iTerm2 可能要求为 OSC 52 剪贴板访问授予权限。运行 `/doctor`；
`terminal.iterm2-clipboard-permission` 建议会指出需要检查的设置。

### 全屏或备用屏幕未激活

Zellij 与 tmux 控制模式可能限制备用屏幕。在这些环境里 Chaos 通常使用内联
模式。运行 `/doctor` 查看检测到的状态。你可以在 `~/.chaos/pager.toml` 里
配置 `[terminal] alt_screen`，或运行 `chaos --no-alt-screen` 确认内联模式
可用。

### Zellij 键位与 Chaos 冲突

Zellij 可能在按键到达 Chaos 之前拦截 Ctrl/Alt 组合键。在 Zellij 0.41 及更高
版本中，使用 **Unlock-First (non-colliding)** 预设：

1. 按 `Ctrl+o`，然后按 `c`。
2. 打开 **Change Mode Behavior**。
3. 选择 **Unlock-First (non-colliding)**。
4. 按 `Enter` 应用。

需要 Zellij 自己的面板或会话控制时按 `Ctrl+g`。在最小模式下，如果 `Ctrl+G`
仍然到不了 Chaos，打开命令面板并选择**在外部编辑器中编辑提示**。这会保留
当前草稿；直接输入 `/edit-prompt` 会开启一个空的编辑器草稿，因为该命令本身
占用了输入框。

### Ctrl+Enter 在 WezTerm 中不触发插话

WezTerm 默认禁用 Kitty 键盘协议。在 Chaos 里运行 `/doctor`。
`terminal.wezterm-kitty` 检查项会给出设置与重启步骤。通过 SSH 时，Doctor
只显示当前会话中可行的替代方案。Apple Terminal 用 `Ctrl+O` 触发插话，因为
它无法区分带修饰的 Enter 组合键。

### Shift+Enter 在 VS Code 中不插入换行

VS Code、Cursor、Windsurf 与 Zed 终端使用 xterm.js，后者只部分实现了 Kitty
键盘协议，并对某些 Shift 加可打印键的组合编码有误。因此 Chaos 不在那里协商
该协议，Shift+Enter 可能与 Enter 一样到达为同一个 `CR`。当 `TERM_PROGRAM`
未被转发时，经 SSH 访问的 VS Code 也受影响。用 `Alt+Enter` 插入换行；
`/doctor` 会报告 `terminal.newline-fallback` 及检测到的解释与替代方案。

### Cmd+Enter 不是公开的发送或换行组合键

`Cmd+Enter` 不是公开的发送或换行组合键。Chaos 只把 `Shift+Enter` 与
`Alt+Enter` 公开为换行。许多终端把 Cmd+Enter 绑定到全屏，因此 `SUPER` 不在
换行匹配器之内，送达的 `SUPER+Enter` 也不匹配代理的裸 Enter 发送绑定。当
Kitty（或其它能送达 `SUPER` 的协议）真的送达 `SUPER+Enter` 时，输入框仍然
插入换行：该按键错过发送、落入文本区，而文本区把任何 Enter 都当作换行。
Apple Terminal 是另一条本地路径：CoreGraphics 补救机制把按住 Cmd 视为带
修饰的 Enter，于是对到达的裸 Enter 插入换行。通过 SSH 时 Cmd 修饰永远不会
到达，该组合键看起来就是裸 Enter，于是直接发送。

草稿非空时，输入框底栏会显示当前可用的换行组合键。SSH 下它优先显示
`Alt+Enter`。你也可以先输入 `\` 再按 Enter，或用 `/ml`。不要指望 Cmd+Enter
在远程会话中插入换行。

### 鼠标滚动失效

如果 Chaos 不再收到鼠标输入，请在终端中重新启用鼠标上报：

- **Apple Terminal**：**View → Allow Mouse Reporting**（`Cmd+R`）。
- **iTerm2**：**Settings → Profiles → Terminal → Enable mouse reporting**。

### 语音听写录不到内容

约 10 秒后仍无转写文本时，Chaos 停止采集并显示
**“No speech was detected. Voice stopped.”**，附麦克风修复步骤。在 macOS
上，被拒绝的麦克风授权看起来可能与静音一样，因为权限属于承载 Chaos 的那个
终端。打开**系统设置 → 隐私与安全性 → 麦克风**，启用该终端并重启它。如果
访问已开启，请在**系统设置 → 声音 → 输入**下检查输入设备与音量后重试。

运行 `chaos doctor`，或在语音模式开启时运行 `/doctor`。**语音**部分会显示
Chaos 将使用的麦克风。若没有可用输入设备，Doctor 会显示
`voice.no-input-device` 及后续步骤。当 macOS 以静音代替报错时，Doctor 无法
被动检测到被拒的麦克风访问。

在 macOS 上，每次听写使用一个短生命周期的采集辅助进程，使音频栈的内存在
采集结束时释放。如果怀疑辅助进程本身有问题，可设置
`GROK_VOICE_CAPTURE=inprocess`，改用进程内回退路径做对比。

### Byobu 与 GNU screen

Byobu 在 GNU screen 上支持有限。`/doctor` 会报告 `terminal.byobu-screen`
并说明如何切换到 Byobu 的 tmux 后端。

### 阿拉伯语与波斯语（RTL）文本

许多终端自己就会重排从右向左的文本（基于 VTE 的终端、Terminal.app、
Konsole、mlterm 等）。因此 Chaos **默认不**做 RTL 重排。

如果**回滚区**（或列表内容）里的阿拉伯语或波斯语读起来是反的，请在
`~/.chaos/pager.toml`（或项目配置）中启用应用侧重排：

```toml
[scrollback.display]
rtl_bidi = true
```

该设置随外观配置热重载（无需完全重启）。如果默认情况下文本正常、启用后反
而错了，请把它关掉——你的终端已经在处理 bidi。

启用后：

- 重排回滚区、列表内容与全屏块查看器中的完整内容行（看板预览与钩子弹窗
  镜像回滚区，也同样重排）。行首装饰、下拉菜单与模态框保持逻辑序，以保证
  命中检测一致。
- Markdown 表格的列不变。
- 搜索高亮、选区/拖拽复制、双击选词/选 URL 以及链接命中目标都在同一行的
  绘制（视觉）单元格与逻辑文本之间映射，因此屏幕上的高亮落在正确的字形上，
  而剪贴板粘贴仍保持逻辑序。
- 基方向按每个绘制行解析。以英文开头的软换行续行，其基方向可以与该段落
  首行不同。

这不是完整的镜像 RTL 界面。

---

## 仍然卡住？

运行 `/feedback` 上报。
