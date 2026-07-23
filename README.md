# Chaos

**Chaos** 是终端 AI 编码助手（二进制名 `chaos`）。它**不使用 Grok / xAI 登录**；
模型、接口地址与密钥均由用户自行配置（BYOK）。完整产品说明见
[CHAOS.md](CHAOS.md)。

本仓库基于 [Grok Build](https://github.com/xai-org/grok-build) 上游源码改造；内部
crate 仍保留 `xai-grok-*` 命名以利同步上游。`SOURCE_REV` 记录当前对齐的 monorepo
提交 SHA。

[安装](#安装) ·
[从源码构建](#从源码构建) ·
[配置](#配置) ·
[文档](#文档) ·
[仓库结构](#仓库结构)

---

## 安装

### npm（推荐，预编译二进制）

```sh
npm i -g chaos-code
chaos --version
```

需要 Node.js ≥ 20。npm 会安装元包 + 当前平台的 optional 依赖包
（如 `chaos-code-linux-x64`），`postinstall` 将二进制解压到
`~/.chaos/bin/chaos`（若已有 `~/.grok` 则沿用其 `bin/`）。

**发布**：

- **现在没有 Actions / 加不了 `NPM_TOKEN`**：本机 `npm login` 后  
  `./scripts/ci/local-publish-host.sh --publish`（只发当前平台）。  
- **有 CI 之后**：push `.github/workflows`，在  
  Settings → Secrets → Actions 加 `NPM_TOKEN`，再打 `v*` tag。  

详见
[`npm/PUBLISH.md`](crates/codegen/xai-grok-pager/npm/PUBLISH.md)。

### 从源码构建

环境要求：

- **Rust** — 工具链由 [`rust-toolchain.toml`](rust-toolchain.toml) 固定；
  `rustup` 首次构建时会自动安装。
- **[DotSlash](https://dotslash-cli.com)** — 供 [`bin/`](bin/) 下 hermetic 工具
  （尤其是 [`bin/protoc`](bin/protoc)）下载运行。构建前请确保 `dotslash` 在
  `PATH` 中：

  ```sh
  cargo install dotslash
  # 或预编译包：https://dotslash-cli.com/docs/installation/
  /usr/bin/env dotslash --help
  ```

- **protoc** — 通过 DotSlash 解析 `bin/protoc`，或使用 `PATH` / `$PROTOC` 中的
  `protoc`。
- 支持 macOS / Linux 构建主机；Windows 为 best-effort。

```sh
cargo run -p xai-grok-pager-bin              # 构建并启动 TUI（二进制名 chaos）
cargo build -p xai-grok-pager-bin --release  # 产物：target/release/chaos
cargo check -p xai-grok-pager-bin            # 快速校验
./target/release/chaos --version
```

**不要**运行上游的 `https://x.ai/cli/install.sh`：那会安装官方 `grok`，不是 Chaos。

## 配置

用户配置目录双读（不覆盖任一侧）：

1. `$CHAOS_HOME` → 2. `$GROK_HOME` → 3. 已有 `~/.chaos` → 4. 已有 `~/.grok` →
5. 默认新建 `~/.chaos`

项目级 `.chaos/` 与 `.grok/` 同样双读。模型与 Provider 示例见
[CHAOS.md](CHAOS.md)。密钥请用环境变量，勿提交到 Git。

## 文档

- 产品入口与 BYOK 说明：[CHAOS.md](CHAOS.md)
- 用户指南（随 pager 发布）：
  [`crates/codegen/xai-grok-pager/docs/user-guide/`](crates/codegen/xai-grok-pager/docs/user-guide/)
  （部分章节仍写上游路径名；以 CHAOS.md 与双读策略为准）

## 仓库结构

| Path | Contents |
|------|----------|
| `crates/codegen/xai-grok-pager-bin` | 组合根包；产出 `chaos` 二进制 |
| `crates/codegen/xai-grok-pager` | TUI：scrollback、prompt、模态、渲染 |
| `crates/codegen/xai-grok-shell` | Agent 运行时 + leader/stdio/headless |
| `crates/codegen/xai-grok-tools` | 工具实现（终端、编辑、搜索等） |
| `crates/codegen/xai-grok-workspace` | 主机文件系统、VCS、执行、检查点 |
| `crates/codegen/...` | 其余 CLI 依赖闭包（config、MCP、markdown、sandbox 等） |
| `crates/common/`、`crates/build/`、`prod/mc/` | 闭包用到的共享叶子 crate |
| `third_party/` | 上游 vendored 源码（Mermaid 等） |

> [!IMPORTANT]
> The root `Cargo.toml` (workspace members, dependency versions, lints,
> profiles) is **generated** — treat it as read-only. Prefer editing per-crate
> `Cargo.toml` files.

## Development

```sh
cargo check -p <crate>        # always target specific crates; full-workspace builds are slow
cargo test -p xai-grok-config # per-crate tests
cargo clippy -p <crate>       # lint config: clippy.toml at the repo root
cargo fmt --all               # rustfmt.toml at the repo root
```

## Contributing

> [!NOTE]
> External contributions are not accepted. See [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

First-party code in this repository is licensed under the **Apache License,
Version 2.0** — see [`LICENSE`](LICENSE).

Third-party and vendored code remains under its original licenses. See:

- [`THIRD-PARTY-NOTICES`](THIRD-PARTY-NOTICES) — crates.io / git dependencies,
  bundled UI themes, and **in-tree source ports** (including openai/codex and
  sst/opencode tool implementations)
- [`crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md`](crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md)
  — crate-local notice for the codex and opencode ports (license texts +
  Apache §4(b) change notice)
- [`third_party/NOTICE`](third_party/NOTICE) — vendored Mermaid-stack index
