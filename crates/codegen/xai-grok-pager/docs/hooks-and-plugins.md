# 钩子与插件指南

Chaos 支持**钩子**（事件驱动的 shell 命令）与**插件**（技能、代理、钩子和 MCP 服务器的打包）。两者都通过一个统一的模态界面管理。

## 打开模态

| 方式 | 打开的标签页 |
|--------|-------------|
| `Ctrl+L` | 插件（任意面板；**非 VS Code 系** —— 在 VS Code / Cursor / Windsurf / Zed 上用 `/plugins`） |
| `/plugins` | 插件（任意终端） |
| `/hooks` | Hooks 标签页 |

## 标签页

模态有三个标签页：**Hooks**、**插件**和**市场**。用 `Tab` / `→` 切到下一个，用 `Shift+Tab` / `←` 切回上一个。

---

## Hooks 标签页

钩子是在 `session_start`、`post_tool_use`、`notification` 等事件上自动运行的 shell 命令（或 HTTP 调用）。想自己写钩子，见[创建自定义钩子](custom-hooks.md)。

钩子按来源分组：
- **全局钩子** —— 来自 `~/.chaos/hooks/`
- **项目钩子** —— 来自你仓库里的 `.chaos/hooks/`
- **插件钩子** —— 随已安装的插件打包
- **自定义钩子** —— 手动通过路径添加

每个钩子显示：
- 触发它的**事件**（例如 `session_start`、`post_tool_use`）
- 运行的**命令**或 **URL**
- **超时**时长
- **状态** —— 已启用或 `[disabled]`

### 快捷键（Hooks 标签页）

| 键 | 操作 |
|-----|--------|
| `l` | 重新加载所有钩子 |
| `a` | 按路径添加钩子 |
| `r` | 移除选中的钩子 |
| `e` | 启用 / 禁用选中的钩子 |
| `Space` | 展开 / 折叠分组 |

---

## 插件标签页

插件是包含技能、代理、钩子和 MCP 服务器配置任意组合的目录。

展开后，每个插件显示：
- **名称**与**版本**
- **作用域** —— `user`、`project`、`cli`，或插件市场源名称
- **技能** —— 名称或数量
- **代理** —— 名称或数量
- **钩子** —— 数量
- **MCP 服务器** —— 数量（未受信任时显示为 "blocked"）
- **描述**
- **冲突** —— 有任何冲突时给出 ⚠ 警告

插件钩子会自动收到 `GROK_PLUGIN_ROOT` 和 `GROK_PLUGIN_DATA` 环境变量（见[插件指南](user-guide/09-plugins.md#插件钩子中的环境变量)）。

### 快捷键（插件标签页）

| 键 | 操作 |
|-----|--------|
| `r` | 重新加载所有插件 |
| `i` | 按路径安装插件 |
| `e` | 启用 / 禁用选中的插件 |
| `Space` | 展开 / 折叠插件详情 |
| `/` | 按名称搜索插件 |

---

## 市场标签页

从已配置的插件市场源浏览并安装插件。

来源从以下位置加载：
1. **config.toml** —— `[[marketplace.sources]]` 条目
2. **settings.json** —— 来自 `~/.chaos/settings.json` 或 `~/.claude/settings.json` 的 `extraKnownMarketplaces`

每个源会连同它的插件一起显示：
- **名称**与**版本**
- **描述**
- **安装状态** —— `[installed]`、`[installed • update: v1 → v2]`，或未安装

### 快捷键（市场标签页）

| 键 | 操作 |
|-----|--------|
| `i` | 安装选中的插件 |
| `d` | 卸载选中的插件 |
| `r` | 刷新插件市场源（重新 clone/pull git 仓库） |
| `u` | 更新所有已安装的插件市场插件 |
| `Space` | 展开 / 折叠源或插件 |
| `/` | 按名称搜索插件 |

### 添加插件市场源

在市场标签页按 `a`（或运行 `chaos plugin marketplace add <source>`），
给出一个 git URL、GitHub 简写（`owner/repo`），或本地目录路径
（`/absolute`、`~/dir` 或 `./relative`）。本地路径会存为 `path` 源——从已有
检出开发插件市场时很方便。

源会写入 `~/.chaos/config.toml`：

```toml
[[marketplace.sources]]
name = "My Team Plugins"
git = "https://github.com/my-org/plugins.git"

[[marketplace.sources]]
name = "Local Dev"
path = "~/dev/my-plugins"
```

或写入 `~/.chaos/settings.json` / `~/.claude/settings.json`：

```json
{
  "extraKnownMarketplaces": {
    "my-marketplace": {
      "source": { "source": "git", "url": "git@github.com:my-org/plugins.git" },
      "autoUpdate": true
    }
  }
}
```

---

## 通用键盘快捷键

这些在所有标签页都可用：

| 键 | 操作 |
|-----|--------|
| `Tab` / `→` | 下一个标签页 |
| `Shift+Tab` / `←` | 上一个标签页 |
| `j` / `↓` | 向下移动选择 |
| `k` / `↑` | 向上移动选择 |
| `Space` | 切换展开 / 折叠 |
| `/` | 开始搜索（插件与插件市场） |
| `Backspace` | 删除搜索字符，或重新进入搜索 |
| `Esc` | 清空搜索，或关闭模态 |
| `q` | 关闭模态 |

## 确认与错误

有些操作（比如卸载插件）会请求确认：
- 按 `y` 确认
- 按 `Esc` 或任何其它键取消

错误会以消息浮层显示——按任意键关掉。

操作进行中时，模态会显示 "Processing..." 并阻塞输入，直到操作完成。

## 另见

- [创建自定义钩子](custom-hooks.md) —— 一步步教你写自己的钩子和脚本
- [钩子用户指南](user-guide/10-hooks.md) —— 事件、匹配器、信任模型
- [钩子示例](../../xai-grok-hooks/examples/README.md) —— 开箱即用的示例钩子
- [插件用户指南](user-guide/09-plugins.md) —— 安装、信任与插件市场
