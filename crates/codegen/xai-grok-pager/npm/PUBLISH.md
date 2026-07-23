# Publishing chaos-code

用户安装：

```bash
npm i -g chaos-code
```

发版有两条路：

| 路径 | 何时用 | 是否需要 GitHub `NPM_TOKEN` |
|------|--------|------------------------------|
| **A. 本机发布**（当前主机平台） | 仓库还没 Actions / 先打通 npm | **否**（`npm login` 即可） |
| **B. CI 多平台** | workflow 已 push | npm 可选：无 `NPM_TOKEN` 仍构建 + GitHub Release |

`NPM_TOKEN` **不是**发版的前提：CI 无 token 时会 **跳过 npm**，仍上传二进制到 GitHub Release。本机 `npm login` 也可直接 publish。

## 架构

```
tag v0.2.110 / workflow_dispatch
        │
        ▼
  resolve-version
        │
        ├─► build (matrix × 6)
        │     linux x64/arm64 · darwin x64/arm64 · win x64/arm64
        │     cargo build -p xai-grok-pager-bin --profile release-dist
        │
        ▼
  assemble-and-publish
        stamp package.json versions
        brotli → chaos-code-<os>-<cpu>
        npm publish 平台包 → 再 publish chaos-code
        (可选) GitHub Release 挂原始二进制
```

相关文件：

| 路径 | 作用 |
|------|------|
| [`.github/workflows/release.yml`](../../../../.github/workflows/release.yml) | 发布流水线 |
| [`scripts/ci/stamp-npm-version.mjs`](../../../../scripts/ci/stamp-npm-version.mjs) | 统一 stamp 元包 + 6 个平台包版本 |
| [`scripts/ci/publish-npm.sh`](../../../../scripts/ci/publish-npm.sh) | 先平台后元包的 `npm publish` |
| [`npm/chaos/scripts/assemble-platform-packages.js`](chaos/scripts/assemble-platform-packages.js) | 二进制 → `.br` + version stamp |

## 为什么「没有 Actions / 加不了 NPM_TOKEN」

常见情况：

1. **workflow 还在本地、没 push** — 远程 `Actions` 页为空，看起来像「没有 Actions」。  
   API 上本仓库 Actions 已是 enabled；把 `.github/workflows/*.yml` 合进 `main` 并 push 后，Actions 页才会出现 workflow。
2. **Secret 入口在仓库设置里，不在 Actions 运行页** —  
   仓库 → **Settings → Secrets and variables → Actions → New repository secret**，名字 `NPM_TOKEN`。  
   没有跑过 workflow 也可以先加 Secret；没有 Secret 也能用路径 A 本机发布。
3. **个人免费账号**默认有 Actions 分钟数；若组织关掉了 Actions，才需要开权限或继续用路径 A。

---

## 路径 A：本机发布（无需 GitHub Secret）

适合现在：先让 Linux（或你当前机器）用户能 `npm i -g chaos-code`。

```bash
# 1. npm 登录（浏览器 / 一次性密码，只在本机）
npm login
npm whoami

# 2. 构建当前平台二进制
cargo build -p xai-grok-pager-bin --release
# 产物：target/release/chaos

# 3. 组装并 dry-run
./scripts/ci/local-publish-host.sh
# 可选指定版本：./scripts/ci/local-publish-host.sh --version 0.2.110

# 4. 确认 dry-run 无误后真正发布
./scripts/ci/local-publish-host.sh --publish
```

脚本会：

1. 找到本机 `chaos` 二进制  
2. `ONLY_HOST=1` brotli 进对应 `chaos-code-<platform>`  
3. 先 `npm publish` 平台包，再 publish 元包 `chaos-code`

**限制**：只发布了**当前平台**的 optional 包。其它 OS/arch 用户装元包时，对应 `chaos-code-*` 若 404，optionalDep 会跳过，postinstall 找不到二进制。要全平台支持，再上路径 B，或在 Mac/Windows 机器上各跑一遍本脚本。

---

## 路径 B：CI 六平台（需要 Secret）

### 一次性准备

1. **npm 账号**有权发布无 scope 包名 `chaos-code` 与 `chaos-code-*`  
   （不要用 `chaos-cli`：该名已被 npm 废弃占位。）
2. **先 push workflow**（否则远程没有 Actions 定义）：
   ```bash
   git add .github/workflows scripts/ci crates/codegen/xai-grok-pager/npm
   git commit -m "ci: release workflow for multi-platform npm publish"
   git push origin main
   ```
3. （可选）**Settings → Secrets and variables → Actions** 添加 `NPM_TOKEN`  
   （npm 网站 → Access Tokens → **Automation**）。没有则只发 GitHub Release。
4. workflow 已声明 `contents: write`（创建 Release 用 `GITHUB_TOKEN`，无需自建）。

### 发版步骤

#### 打 tag（推荐）

```bash
# bump 版本后（CI 会把 npm package.json stamp 到 tag 版本）：
git tag v0.2.110
git push origin v0.2.110
```

`Release` workflow：`resolve-version` → 矩阵 `build` → `package`  
（assemble → 可选 npm → GitHub Release）。

#### 手动跑 workflow

GitHub → Actions → **Release** → **Run workflow**：

- `version`：如 `0.2.110`（可空则读 package.json）
- `publish_npm`：是否写 npmjs
- `create_github_release`：是否上传二进制到 Releases

## 本地调试（非发版，不 publish）

仅当前主机：

```bash
cargo build -p xai-grok-pager-bin --profile release-dist
# 产物：target/<triple>/release-dist/chaos

ONLY_HOST=1 node crates/codegen/xai-grok-pager/npm/chaos/scripts/assemble-platform-packages.js
# 或指定路径：
# CHAOS_LINUX_X64=target/release/chaos ONLY_HOST=1 node ...assemble...

cd crates/codegen/xai-grok-pager/npm
npm install -g ./chaos-linux-x64 ./chaos   # 按本机平台改目录名
chaos --version
```

Dry-run 发布（不真正 push registry）：

```bash
DRY_RUN=1 NPM_TOKEN=dummy scripts/ci/publish-npm.sh
```

## 环境变量（assemble）

| Env | 含义 |
|-----|------|
| `CHAOS_LINUX_X64` | Linux x64 二进制路径 |
| `CHAOS_LINUX_ARM64` | Linux arm64 |
| `CHAOS_DARWIN_ARM64` | macOS Apple Silicon |
| `CHAOS_DARWIN_X64` | macOS Intel |
| `CHAOS_WIN32_X64` | Windows x64 `chaos.exe` |
| `CHAOS_WIN32_ARM64` | Windows arm64 `chaos.exe` |
| `ONLY_HOST=1` | 只组装当前 `process.platform-arch` |

## 安装与更新（用户侧）

```bash
npm i -g chaos-code
chaos --version
chaos update   # 若 installer=npm，内部执行 npm i -g chaos-code@…
```

## 故障排查

| 现象 | 处理 |
|------|------|
| `NPM_TOKEN secret is not set` | 配置 repo secret `NPM_TOKEN` |
| 平台包 404 / optionalDeps 装不上 | 必须先 publish 六个 `chaos-code-*` 再发元包；检查 publish 顺序脚本 |
| tarball 过大 | 使用 `release-dist` + `strip`；确认 brotli 后单包 < ~200MB |
| Windows arm runner 不可用 | 仓库若无 `windows-11-arm`，可暂时从 matrix 去掉 win32-arm64，或改用自托管 runner |
| 构建缺 protoc | workflow 已 apt/brew/choco 安装；本地可 `cargo install dotslash` 后用 `bin/protoc` |

## 版本对齐

CI 以 **tag / 输入 version** 为准 stamp npm。`xai-grok-version` 与二进制 `GROK_VERSION` 应在发版前提前 bump 到同一 semver，这样 `--version` 与 npm 版本一致。
