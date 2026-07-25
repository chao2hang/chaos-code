# Chaos

**Chaos** 是终端 AI 编码助手（二进制名 `chaos`）。**不使用 Grok / xAI 登录**；模型、接口与密钥由用户自行配置（BYOK）。产品说明见 [CHAOS.md](CHAOS.md)。

本仓库基于 [Grok Build](https://github.com/xai-org/grok-build) 改造；crate 仍保留 `xai-grok-*` 命名以便同步上游。`SOURCE_REV` 记录对齐的 monorepo 提交。

[安装](#安装) · [更新](#更新) · [配置](#配置) · [从源码构建](#从源码构建) · [文档](#文档) · [开发](#开发)

---

## 安装

**推荐主渠道：GitHub Release 预编译二进制**（不依赖 Node / npm）。  
发布页：[Releases](https://github.com/chao2hang/chaos-code/releases)

| 平台 | 推荐方式 |
|------|----------|
| macOS / Linux | `install.sh` 一键安装 |
| Windows（cmd） | `install.bat`（**不需要** `iex`） |
| Windows（PowerShell） | `install.ps1`，或同上 bat |
| 受限环境 | 浏览器手动下载对应资产 |

默认安装目录：`~/.chaos/bin`（Windows：`%USERPROFILE%\.chaos\bin`）。若已有 `~/.grok` 或设置了 `CHAOS_HOME` / `GROK_HOME`，则沿用其 `bin/`。

**不要**运行上游 `https://x.ai/cli/install.sh`（那是官方 `grok`，不是 Chaos）。

### macOS / Linux

```sh
curl -fsSL https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.sh | bash
chaos --version
```

固定版本 / 强制覆盖：

```sh
curl -fsSL https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.sh | bash -s -- --version 0.2.113
curl -fsSL https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.sh | bash -s -- --force
```

本地仓库：

```sh
./scripts/install.sh --version 0.2.113
```

### Windows

任选其一即可。装好后**新开一个终端**再执行 `chaos --version`（用户 PATH 更新后需新会话）。

#### 方式 A：cmd 一键（推荐，无需 `iex`）

> [!IMPORTANT]
> 以下命令是 **cmd.exe** 语法（`&&`、`%TEMP%`）。**请勿粘贴到 PowerShell**——Windows PowerShell 5.1 不支持 `&&`（会报 `The token '&&' is not a valid statement separator`）。PowerShell 用户请用[方式 B](#方式-bpowershell)，或本节末尾的 PowerShell 等价写法。

```bat
curl -L -o "%TEMP%\install-chaos.bat" https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.bat && "%TEMP%\install-chaos.bat"
```

固定版本 / 强制覆盖：

```bat
"%TEMP%\install-chaos.bat" --version 0.2.113
"%TEMP%\install-chaos.bat" --force
```

本地仓库：

```bat
scripts\install.bat
scripts\install.bat --version 0.2.113 --force
```

`install.bat` 会优先调用同目录的 `install.ps1`；若无 PowerShell 或脚本失败，则回退为直接下载 `chaos.exe` 并写入用户 PATH。

在 **PowerShell** 里想走 bat 安装器，用原生等价写法（`;` 分隔、`$env:TEMP` 变量、`curl.exe` 避开别名）：

```powershell
$bat = "$env:TEMP\install-chaos.bat"
curl.exe -L -o $bat https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.bat; & $bat
```

#### 方式 B：PowerShell

```powershell
irm https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.ps1 | iex
```

固定版本（管道 `iex` 不便传参时）：

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.ps1))) -Version 0.2.113
```

先下载再执行（组策略限制管道时更稳）：

```powershell
Invoke-WebRequest -Uri "https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.ps1" -OutFile "$env:TEMP\install-chaos.ps1"
powershell -ExecutionPolicy Bypass -File "$env:TEMP\install-chaos.ps1"
```

#### 方式 C：手动下载 exe

1. 打开 [Releases](https://github.com/chao2hang/chaos-code/releases/latest)
2. 下载 `chaos-win32-x64.exe`（ARM 机用 `chaos-win32-arm64.exe`）
3. 重命名为 `chaos.exe`，放到 `%USERPROFILE%\.chaos\bin\`
4. 把该目录加入**用户** PATH，新开终端验证：`chaos --version`

cmd 示例：

```bat
mkdir "%USERPROFILE%\.chaos\bin" 2>nul
curl -L -o "%USERPROFILE%\.chaos\bin\chaos.exe" https://github.com/chao2hang/chaos-code/releases/latest/download/chaos-win32-x64.exe
rem Prefer install.bat for PATH. Manual setx: read *user* Path only (do not use %%Path%%).
for /f "tokens=2*" %%A in ('reg query "HKCU\Environment" /v Path 2^>nul') do setx Path "%%B;%USERPROFILE%\.chaos\bin"
```

### 安装脚本说明

| 项 | 说明 |
|----|------|
| 下载源 | `https://github.com/chao2hang/chaos-code/releases` 对应平台资产 |
| 安装目录 | `$CHAOS_HOME/bin` → `$GROK_HOME/bin` → 已有 `~/.chaos` / `~/.grok` → 默认 `~/.chaos/bin` |
| PATH | Unix 写入 shell rc；Windows 写**用户** PATH（需新终端） |
| 不改 PATH | `install.sh --no-path` / `install.ps1 -NoPath` / `install.bat --no-path` |

| 脚本 | 用途 |
|------|------|
| [`scripts/install.sh`](scripts/install.sh) | macOS / Linux |
| [`scripts/install.bat`](scripts/install.bat) | Windows cmd（无 iex） |
| [`scripts/install.ps1`](scripts/install.ps1) | Windows PowerShell |

---

## 更新

安装后用内置更新（默认 **GitHub Release**，不依赖 xAI / npm）：

```sh
chaos update
chaos update --version 0.2.113
```

强制渠道（一般不必改）：

```sh
GROK_INSTALLER=gh-release chaos update   # 默认
GROK_INSTALLER=npm chaos update          # 改走 npm（需平台包齐全）
```

---

## 其他安装方式（可选）

### npm

需要 Node.js ≥ 20。适合已有 Node 环境的用户；**不是**推荐主渠道，Windows 平台包可能缺失或被 registry 拦截。

```sh
npm i -g chaos-code
chaos --version
```

装不上时请改用上文 **GitHub Release** 安装。维护者发布说明见
[`npm/PUBLISH.md`](crates/codegen/xai-grok-pager/npm/PUBLISH.md)。

### 从源码构建

环境：

- **Rust** — [`rust-toolchain.toml`](rust-toolchain.toml)（`rustup` 会自动安装）
- **[DotSlash](https://dotslash-cli.com)** — 供 [`bin/protoc`](bin/protoc) 等 hermetic 工具
- **protoc** — 经 DotSlash，或 `PATH` / `$PROTOC`

```sh
cargo install dotslash   # 若尚未安装
cargo build -p xai-grok-pager-bin --release
./target/release/chaos --version

cargo run -p xai-grok-pager-bin   # 开发：构建并启动 TUI
```

支持 macOS / Linux 构建主机；Windows 为 best-effort。

---

## 配置

用户配置目录解析顺序（**不覆盖**任一侧已有文件）：

1. `$CHAOS_HOME` → 2. `$GROK_HOME` → 3. 已有 `~/.chaos` → 4. 已有 `~/.grok` → 5. 默认新建 `~/.chaos`

项目级 `.chaos/` 与 `.grok/` 同样双读。模型与 Provider 示例见 [CHAOS.md](CHAOS.md)。密钥请用环境变量，勿提交到 Git。

---

## 文档

- 产品与 BYOK：[CHAOS.md](CHAOS.md)
- 用户指南：[`crates/codegen/xai-grok-pager/docs/user-guide/`](crates/codegen/xai-grok-pager/docs/user-guide/)（部分章节仍为上游路径名，以 CHAOS.md 与双读策略为准）

---

## 仓库结构

| 路径 | 内容 |
|------|------|
| `crates/codegen/xai-grok-pager-bin` | 组合根包；产出 `chaos` 二进制 |
| `crates/codegen/xai-grok-pager` | TUI |
| `crates/codegen/xai-grok-shell` | Agent 运行时 |
| `crates/codegen/xai-grok-tools` | 工具实现 |
| `crates/codegen/xai-grok-workspace` | 文件系统、VCS、执行、检查点 |
| `crates/codegen/...` | 其余 CLI 依赖（config、MCP、markdown、sandbox 等） |
| `crates/common/`、`crates/build/`、`prod/mc/` | 共享叶子 crate |
| `third_party/` | vendored 源码（Mermaid 等） |
| `scripts/` | 安装与发版脚本 |

> [!IMPORTANT]
> 根目录 `Cargo.toml`（workspace members、依赖版本、lints、profiles）为**生成文件**，请只改各 crate 内的 `Cargo.toml`。

---

## 开发

```sh
cargo check -p <crate>        # 指定 crate；全 workspace 很慢
cargo test -p xai-grok-config
cargo clippy -p <crate>
cargo fmt --all
```

---

## Contributing

> [!NOTE]
> 不接受外部贡献。见 [`CONTRIBUTING.md`](CONTRIBUTING.md)。

## License

本仓库第一方代码为 **Apache License 2.0**，见 [`LICENSE`](LICENSE)。

第三方与 vendored 代码保留原许可证：

- [`THIRD-PARTY-NOTICES`](THIRD-PARTY-NOTICES)
- [`crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md`](crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md)
- [`third_party/NOTICE`](third_party/NOTICE)
