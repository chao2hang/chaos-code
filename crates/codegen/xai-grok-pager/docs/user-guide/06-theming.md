# 主题与外观

Chaos 从中心主题绘制全部 TUI 颜色。运行时可切换主题、跟随系统浅色/深色外观，
并通过配置文件调整回滚区布局、动画与块样式。

> 界面显示名为 **Chaos Night / Chaos Day**；配置键仍兼容上游的 `groknight` /
> `grokday` 等别名。用户配置根见[配置](05-configuration.md)。

---

## 可用主题

内置五套主题，外加跟随系统外观的 `auto`：

| 主题 | 配置项 | 说明 | 需要 Truecolor |
|-------|-------------|-------------|--------------------|
| **Chaos Night**（内部 `GrokNight`） | `groknight`, `grok-night`, `dark` | 中性深色底 + 品红强调。默认主题。256/16 色终端量化后仍清晰。 | 否 |
| **Chaos Day**（内部 `GrokDay`） | `grokday`, `grok-day`, `light`, `day` | 浅色主题，适合亮色终端背景。 | 否 |
| **Tokyo Night**（内部 `TokyoNight`） | `tokyonight`, `tokyo-night`, `tokyo` | 取自 Tokyo Night 配色的深蓝调背景。量化后会失去自身特色。 | 是 |
| **Rose Pine Moon**（内部 `RosePineMoon`） | `rosepine`, `rose-pine`, `rosepine-moon`, `rose-pine-moon` | 柔和深色配淡紫强调色，出自 Rosé Pine 家族。 | 是 |
| **Oscura Midnight**（内部 `OscuraMidnight`） | `oscura`, `oscura-midnight` | 深黑底配紫色强调色。 | 是 |

主题名不区分大小写。`auto` 选项（别名 `system`）见[自动主题（跟随系统外观）](#自动主题跟随系统外观)。

### 最小模式没有主题

**最小模式**（`--minimal`）始终使用一套固定的、跟随终端的配色渲染，完全忽略 `theme` 设置（这些设置仍然对全屏 TUI 生效）。最小模式直接画在你终端自己的背景上，因此用的是终端的默认前景/背景色加上它的 16 色 ANSI 调色板——也就是 `git` 或 `ls` 用的那套颜色——无需检测或配置，在任何浅色或深色终端配置下都保持可读。最小模式下 `/theme` 与 `/settings` 里的主题行都不可用。

最小模式下的语法高亮**不会**在浅色与深色主题文件之间切换（有意避免极性检测）。接近灰色的 token 继承终端默认前景色；有色 token 使用基础 ANSI 强调色（红/绿/黄/蓝/品红/青），因此读文件输出与围栏代码在浅色和深色配置下都清晰可读。

---

## 切换主题

### 在 TUI 中

运行 `/theme` 斜杠命令（别名 `/t`）打开主题选择器。用方向键在列表中移动时，Chaos 会实时预览每个主题。按 Enter 应用并保存所选主题，或按 Escape 放弃。

不想用选择器时，也可以直接传名字：

```
/theme tokyonight
```

只提交 `/theme` 而不在选择器里挑选，则切换到下一个主题。

### 通过配置文件

在 `~/.chaos/config.toml`（或兼容的 `~/.grok/config.toml`）里设置主题：

```toml
[ui]
theme = "tokyonight"
```

---

## 自动主题（跟随系统外观）

设置 `theme = "auto"`，Chaos 就会跟随操作系统的浅色/深色外观自动切换主题：

```toml
[ui]
theme = "auto"
```

默认情况下，深色模式映射到 **GrokNight**、浅色模式映射到 **GrokDay**。可以用 `auto_dark_theme` 与 `auto_light_theme` 覆盖任一映射：

```toml
[ui]
theme = "auto"
auto_dark_theme = "tokyonight"
auto_light_theme = "grokday"
```

`theme = "system"` 是 `theme = "auto"` 的别名。

### 检测方式

| 平台 | 方式 |
|----------|--------|
| **macOS** | 读取 `AppleInterfaceStyle` 系统偏好 |
| **Linux** | 查询 XDG Desktop Portal（`org.freedesktop.appearance.color-scheme`） |
| **Windows** | 读取系统个性化设置注册表 |
| **SSH / tmux / 无头环境** | 依次取 `GROK_APPEARANCE` 或 `LC_GROK_APPEARANCE`（`dark`/`light`）、`COLORFGBG`，最后在启动时发一次 OSC 11 背景查询。`chaos wrap ssh …` 会把本地 OS 主题写进 `LC_GROK_APPEARANCE`，使它能在 SSH 登录 shell 中继续生效。新建的 tmux 会话只有在 tmux 服务器/会话是用该环境变量创建时（或 `update-environment` 包含它时）才会继承它。当 tmux 是直接终端（而不是编辑器里的 `:terminal`）且版本 ≥ 3.3 时，OSC 11 会被 DCS 包裹；要穿透到外层模拟器还需要 `allow-passthrough`，而且回复是尽力而为的。 |

运行期间，Chaos 每 5 秒轮询一次桌面 API 与环境变量提示。在本地桌面上切换系统的浅色/深色模式，几秒内即生效，无需重启。走 SSH 时，wrap 写入的环境变量在该跳上固定不变。

你也可以设置 `GROK_THEME`（或 `LC_GROK_THEME`）来强制指定某个主题或 `auto`，无需编辑 `config.toml`。

### 通过设置面板

运行 `/settings`（别名 `/config`），打开 **外观** 分类，即可交互式设置 **自动深色主题** 与 **自动浅色主题**。在 `/theme` 选择器里选中 `auto` 就会用这两组映射启用自动模式。

---

## 颜色支持检测

启动时，Chaos 会检测终端支持的颜色能力级别：

| 级别 | 说明 | 检测 |
|-------|-------------|-----------|
| **Truecolor**（24 位） | 完整 RGB 颜色。所有主题都按设计效果渲染。 | `COLORTERM=truecolor` 或等价的终端能力 |
| **256 色** | 索引调色板。RGB 值映射到最接近的调色板项。 | 标准 xterm-256color |
| **16 色** | 仅 ANSI 颜色名。颜色映射到最接近的 ANSI 颜色。 | 基础终端支持 |

设置 `NO_COLOR` 后，Chaos 不输出任何颜色，以单色渲染。

运行 `/doctor` 可以查看检测到的颜色级别以及当前终端可用的主题。如果拿不到 truecolor，Doctor 会给出相应的配置步骤，或说明终端的限制。

### 自动量化

每个主题都用完整 RGB 值定义。启动时，Chaos 会把所有颜色量化到检测到的能力级别。也就是说：

- 在 **truecolor** 终端上，颜色原样使用。
- 在 **256 色** 终端上，每个 RGB 值映射到最接近的索引调色板项。
- 在 **16 色** 终端上，颜色映射到 ANSI 颜色名。

GrokNight 与 GrokDay 使用的中性灰色量化后依然干净。TokyoNight、RosePineMoon 和 OscuraMidnight 使用的带色背景很有辨识度，量化后会失去特色，所以主题选择器在非 truecolor 终端上会把它们隐藏起来。

### 运行时生成的颜色

运行时生成的颜色（语法高亮、背景混色）也走同一条量化管线，确保在所有终端类型下外观一致。

---

## 光标颜色

Chaos 用 OSC 12 转义序列把终端光标设成当前主题的 `accent_user` 颜色，用来表示当前有活跃的 Chaos 会话。光标颜色：

- 在启动时和切换主题时应用。
- 退出时通过 OSC 112 恢复为终端默认值。

支持 OSC 12 的终端都能生效（大多数现代终端都支持）。

---

## 紧凑模式

用 `/compact-mode` 斜杠命令切换紧凑模式。紧凑模式会：

- 去掉外层纵向留白（上下边距变为 0）。
- 把横向留白压到最小（1 列）。
- 减少提示框区域与信息块的顶部留白。

该设置保存在 `~/.chaos/config.toml` 的 `[ui].compact_mode` 下，重启后依然生效。

小屏幕上用紧凑模式可以最大化内容区域。

---

## 语法高亮

Chaos 内置三份 `.tmTheme` 文件用于代码块语法高亮，并按当前主题选用其中一份：

- `grok-night.tmTheme` —— Chaos Night、Rose Pine Moon 与 Oscura Midnight
- `grok-day.tmTheme` —— Chaos Day
- `tokyo-night.tmTheme` —— Tokyo Night

切换主题时，Chaos 会自动选用匹配的那份文件。`.tmTheme` 文件编译在二进制里，无法用自己的文件替换。

---

## 用 pager.toml 深度定制

若要细粒度地控制 TUI 外观，可以创建 `~/.chaos/pager.toml`。这个文件控制回滚区布局、块样式、动画等。所有设置都有默认值，只需写出你要覆盖的那些值。（开发构建会把这个文件当作模板生成，逐条注释掉默认值——取消注释某一行即可覆盖它；保持注释的值会继续跟随未来的默认值。）

### 布局

控制视口留白与块间距：

```toml
[scrollback.layout]
outer_vpad = 1          # Vertical padding (top/bottom) for the viewport
outer_hpad_left = 2     # Left margin (minimum: 1)
outer_hpad_right = 2    # Right margin (minimum: 1)
block_pad_left = 2      # Padding between accent line and content
block_pad_right = 2     # Padding after content at right edge
```

### 滚动条

```toml
[scrollback.scrollbar]
enabled = true          # Show/hide the scrollbar
gap_left = 0            # Gap between content and scrollbar (0 = adjacent)
gap_right = 0           # Gap between scrollbar and screen edge (0 = at edge)
# scrollbar_bg = "none" # Override background color (or "none" for theme default)
# scrollbar_fg = "none" # Override thumb color (or "none" for theme default)
```

### 滚动行为

```toml
[scrollback.scroll]
margin = 0                  # Context lines above/below selected entry (0 = edge)
min_page_fraction = 0       # Minimum scroll as % of viewport (0-100)
follow_indicator = "center" # "center" = show the ▼/▲ scroll arrows, "none" = hidden
follow_auto_select = true   # Auto-select latest entry when following
follow_by_overscroll = true # Scrolling past bottom engages follow mode
anchor_on_fold = true       # Keep block header at same screen position when folding
```

### 显示选项

```toml
[scrollback.display]
sticky_headers = true              # Pin user prompts as headers when scrolled past
tab_width = 4                      # Spaces per tab character (0 = pass through)
expandable_indicator = true        # Show "›" on foldable collapsed entries
expandable_indicator_char = "›"    # Character to use (default: "›")
collapsed_accent_char = "❙"        # Accent for collapsed groupable blocks (falls back to "|" on the legacy Windows console)
dim_accent = 0.5                   # Blend factor for dimmed accents (0.0-1.0)
line_under_last_entry = false      # Horizontal line below last entry
selection_buttons = false          # Show copy/view buttons on selection box
```

### 动画

```toml
[animation]
fps = 30           # Frame rate (1-60). Higher = smoother, more CPU
wave_rows = 32     # Rows per wave cycle for accent animation
```

### 块样式：编辑 diff

```toml
[scrollback.blocks.edit]
indent = true                   # Indent diff content
vpad = false                    # Vertical padding around diffs
# expanded_by_default = true    # Unset: follows [ui] collapsed_edit_blocks in config.toml
                                # (flag on = collapsed one-liner); uncomment to pin either shape
hunk_separator = "…"            # Separator between hunks ("…", "───", "⋯", or "" for none)
dual_line_numbers = false       # Two-column line numbers (old + new, like GitHub)
# line_summary = false          # Show +N/-M in the collapsed header; unset follows the same flag
# bg = "none"                   # Block background ("none", "light", "dark")
```

### 块样式：思考/推理

```toml
[scrollback.blocks.thinking]
accent_enabled = true       # Show accent line for thinking blocks
animate = true              # Animate accent line while thinking
truncated_lines = 3         # Lines to show in truncated mode
bg_blend = 70               # Markdown-color blend with background (0-100)
header = true               # Show "Thinking..." header
header_bright = false       # Bright header style (vs dim/muted)
```

### 块样式：工具调用

```toml
[scrollback.blocks.tool]
muted_collapsed = true     # Gray out collapsed tool calls
dim_details = true          # Dim parenthetical details (line counts, match counts)
bullet = "diamond"          # Bullet style before tool headers
```

可用的项目符号样式：

| 配置值 | 字符 | 说明 |
|-------------|-----------|-------------|
| `none` | （无） | 无项目符号 |
| `dot` | `·` | 中点（最小） |
| `small-circle` | `•` | 项目符号 |
| `circle` | `●` | 实心圆 |
| `small-triangle` | `▸` | 右向小三角 |
| `triangle` | `▶` | 右向三角 |
| `diamond` | `◆` | 实心菱形（默认） |

### 块样式：执行（shell 命令）

```toml
[scrollback.blocks.execute]
first_lines = 2                   # Output lines shown at start in truncated mode
last_lines = 3                    # Output lines shown at end in truncated mode
accent_enabled = true             # Show accent line (animated while running)
header_style = "label"            # "shell" ($ prefix) or "label" (Run prefix)
muted_command_collapsed = true    # Mute command text when collapsed
```

### 块样式：用户提示（回滚区）

```toml
[scrollback.blocks.prompt]
vpad = true            # Vertical padding
bg = "light"           # Background ("none", "light", "dark")
show_prefix = true     # Show the prompt prefix character
min_lines = 2          # Minimum content lines in truncated/sticky mode
```

### 提示输入控件

```toml
[prompt]
collapse_unfocused = true    # Collapse when scrollback is focused
mouse_hover = true           # Show hover highlight on mouse over
show_prefix = true           # Show the prompt prefix character
```

### 终端行为

```toml
[terminal]
alt_screen = "auto"    # "auto", "always", or "never"
```

备用屏幕（alt-screen）策略：
- `auto` —— 在普通终端和普通 tmux 中进入全屏；在 tmux 控制模式和 Zellij 中内联运行。
- `always` —— 总是进入全屏。
- `never` —— 从不进入全屏；在主回滚区内联运行。

### 插件界面

```toml
disable_plugins = false   # Set to true to hide /hooks, /plugins commands and annotations
```

---

## 主题颜色槽位

每个主题都定义下列颜色槽位，TUI 各处都从这里取色：

**背景：** `bg_base`, `bg_light`, `bg_dark`, `bg_highlight`, `bg_hover`, `bg_terminal`, `bg_visual`

**强调色：** `accent_user`, `accent_assistant`, `accent_thinking`, `accent_tool`, `accent_system`, `accent_error`, `accent_success`, `accent_running`, `accent_skill`, `accent_plan`, `accent_verify`, `accent_remember`, `accent_model`

**文本：** `text_primary`, `text_secondary`

**灰阶：** `gray_dim`, `gray`, `gray_bright`

**语义色：** `command`, `path`, `running`, `warning`, `fuzzy_accent`

**边框与滚动条：** `selection_border`, `hover_border`, `prompt_border`, `prompt_border_active`, `scrollbar_bg`, `scrollbar_fg`

**粘贴：** `paste_bg`, `paste_fg`, `paste_dim`

**Diff：** `diff_delete_bg`, `diff_delete_fg`, `diff_insert_bg`, `diff_insert_fg`, `diff_equal_fg`, `diff_gutter_fg`

**Markdown：** 标题颜色（`md_heading_h1`-`md_heading_h6`）、`md_code`, `md_code_bg`, `md_text`, `md_muted`, `md_task_checked`, `md_task_unchecked`, `link_fg`

主题系统在内部管理这些槽位，并按你的终端能力自动量化。
