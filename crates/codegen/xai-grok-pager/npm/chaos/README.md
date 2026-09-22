# chaos-code

**Chaos** —— 终端里的 AI 编码助手。自带密钥（BYOK），不需要 Grok / xAI 登录。

安装后的命令是 **`chaos`**。

## 安装

### 一行命令（GitHub Release 二进制，推荐）

当 npm 平台包不全时（例如 Windows 的 `win32-x64` 缺失）用这个。

**macOS / Linux：**

```bash
curl -fsSL https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.sh | bash
# 国内 / GitHub 慢：
# curl -fsSL .../install.sh | CHAOS_CN=1 bash
export PATH="$HOME/.chaos/bin:$PATH"   # 只对当前 shell 生效；新终端会自动读到 PATH
chaos --version
```

**Windows（PowerShell）：**

```powershell
# 国内 / GitHub 慢：$env:CHAOS_CN = "1"
irm https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.ps1 | iex
# 然后开一个「新」终端，或者执行：$env:Path = "$env:USERPROFILE\.chaos\bin;$env:Path"
chaos --version
```

指定版本：`bash -s -- --version 0.2.118` / `-Version 0.2.118`。  
脚本装到 `~/.chaos/bin`（也兼容 `~/.grok/bin`），会就地升级已有安装并配置 PATH。  
可选：用 `CHAOS_GITHUB_MIRROR=https://ghfast.top` 或 `CHAOS_CN=1` 加速 GitHub 下载（SHA256 校验仍然照做）。

### npm

```bash
npm i -g chaos-code
```

需要 Node.js ≥ 20。npm 会装上元包，外加一个平台包
（`chaos-code-<os>-<cpu>`），二进制以 brotli 压缩放在里面。

如果看到 `no platform binary installed for win32-x64`，请改用上面的一行命令，
或者从 [GitHub Releases](https://github.com/chao2hang/chaos-code/releases/latest)
下载 `chaos-win32-x64.exe`。

## 开始使用

```bash
# 启动交互式 TUI
chaos

# 单次任务
chaos -p "Explain this codebase"

chaos --version
```

模型与 Provider 配置写在 `~/.chaos/config.toml`（旧路径 `~/.grok/` 也读）。
见仓库里的 [CHAOS.md](https://github.com/chao2hang/chaos-code/blob/main/CHAOS.md)。

## 更新

```bash
# Release 安装脚本（重跑一遍一行命令，或者）：
curl -fsSL https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.sh | bash -s -- --force

# npm：
npm i -g chaos-code@latest
# 或者，如果当初是用 npm 装的：
chaos update
```

## 支持的平台

| 平台 | 架构 |
|---|---|
| macOS | Apple Silicon (arm64)、Intel (x64) |
| Linux | x86_64、arm64 |
| Windows | x86_64、arm64 |

## 从源码构建

```bash
cargo build -p xai-grok-pager-bin --release
# 二进制：target/release/chaos
```

## 许可

Apache-2.0。见仓库根目录的 `LICENSE` 与 `THIRD-PARTY-NOTICES`。
