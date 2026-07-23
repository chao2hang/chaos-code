# chaos-code

**Chaos** — terminal AI coding assistant. Bring-your-own-key (BYOK); no Grok/xAI login.

The installed command is **`chaos`**.

## Install

```bash
npm i -g chaos-code
```

Requires Node.js ≥ 20. npm installs the meta package plus one platform package
(`chaos-code-<os>-<cpu>`) that carries a brotli-compressed binary.

## Get Started

```bash
# Launch the interactive TUI
chaos

# Single-shot task
chaos -p "Explain this codebase"

chaos --version
```

Configure models and providers under `~/.chaos/config.toml` (or legacy
`~/.grok/`). See [CHAOS.md](https://github.com/chao2hang/chaos-code/blob/main/CHAOS.md)
in the repo.

## Update

```bash
npm i -g chaos-code@latest
# or, if installed via npm:
chaos update
```

## Supported Platforms

| Platform | Architecture |
|---|---|
| macOS | Apple Silicon (arm64), Intel (x64) |
| Linux | x86_64, arm64 |
| Windows | x86_64, arm64 |

## Build from source

```bash
cargo build -p xai-grok-pager-bin --release
# binary: target/release/chaos
```

## License

Apache-2.0. See the repository root `LICENSE` and `THIRD-PARTY-NOTICES`.
