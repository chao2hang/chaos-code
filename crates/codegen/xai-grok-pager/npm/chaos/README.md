# chaos-code

**Chaos** — terminal AI coding assistant. Bring-your-own-key (BYOK); no Grok/xAI login.

The installed command is **`chaos`**.

## Install

### One-liner (GitHub Release binary — recommended)

Works when npm platform packages are incomplete (e.g. Windows `win32-x64` missing).

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.sh | bash
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.ps1 | iex
```

Pin a version: `bash -s -- --version 0.2.113` / `-Version 0.2.113`.  
Scripts install under `~/.chaos/bin` (or `~/.grok/bin`) and configure PATH.

### npm

```bash
npm i -g chaos-code
```

Requires Node.js ≥ 20. npm installs the meta package plus one platform package
(`chaos-code-<os>-<cpu>`) that carries a brotli-compressed binary.

If you see `no platform binary installed for win32-x64`, use the one-liner above
or download `chaos-win32-x64.exe` from
[GitHub Releases](https://github.com/chao2hang/chaos-code/releases/latest).

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
# Release installer (re-run one-liner, or):
curl -fsSL https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.sh | bash -s -- --force

# npm:
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
