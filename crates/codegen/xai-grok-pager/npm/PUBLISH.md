# Publishing chaos-code

用户安装：

```bash
npm i -g chaos-code
```

发版有两条路：

| 路径 | 何时用 | 认证方式 |
|------|--------|----------|
| **A. 本机发布** | 手动提供平台二进制 | 本机 `npm login` |
| **B. CI 多平台** | workflow 已 push | GitHub secret `NPM_TOKEN` + 仓库变量 `CHAOS_NPM_PUBLISH_ENABLED=true` |

`chaos-code-win32-arm64` 与 `chaos-code-win32-x64` 目前是 npm `0.0.1-security` 保留包名；该阻塞解除前，tag 仅发布 GitHub Release，npm 发布默认关闭。只有包名与 token 权限均处理完成后，才设置仓库变量 `CHAOS_NPM_PUBLISH_ENABLED=true` 启用 npm。

## 架构

```
tag v0.4.2 / workflow_dispatch
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

GitHub Release 与 npm 发布已解耦。当前 `CHAOS_NPM_PUBLISH_ENABLED` 缺省关闭；在 npm 占位包处理完成前，tag 只发布 GitHub Release，不会尝试 npm。

常见情况：

1. **workflow 还在本地、没 push** — 远程 `Actions` 页为空，看起来像「没有 Actions」。  
   API 上本仓库 Actions 已是 enabled；把 `.github/workflows/*.yml` 合进 `main` 并 push 后，Actions 页才会出现 workflow。
2. **Secret 入口在仓库设置里，不在 Actions 运行页** —  
   仓库 → **Settings → Secrets and variables → Actions → New repository secret**，名字 `NPM_TOKEN`。  
   没有跑过 workflow 也可以先加 Secret；没有 Secret 也能用路径 A 本机发布。
3. **个人免费账号**默认有 Actions 分钟数；若组织关掉了 Actions，需检查组织权限设置。

---

## 路径 A：本机发布（无需 GitHub Secret）

默认要求具备**全部六个平台二进制包**，否则脚本拒绝发布元包。若要临时只发布某个平台，必须显式设置 `PUBLISH_NPM_ALLOW_PARTIAL=1`；这只发布该平台包，不发布元包，不能据此宣称跨平台可安装。正式发版建议使用路径 B 的完整矩阵构建。

```bash
# 1. npm 登录（浏览器 / 一次性密码，只在本机）
npm login
npm whoami

# 2. 正式全平台发布必须提供所有六个平台二进制。partial 示例仅供单平台测试，不发布元包：
# export CHAOS_DARWIN_ARM64=/artifacts/darwin-arm64/chaos
# export CHAOS_DARWIN_X64=/artifacts/darwin-x64/chaos
# export CHAOS_LINUX_ARM64=/artifacts/linux-arm64/chaos
# export CHAOS_LINUX_X64=/artifacts/linux-x64/chaos
# export CHAOS_WIN32_ARM64=/artifacts/win32-arm64/chaos.exe
# export CHAOS_WIN32_X64=/artifacts/win32-x64/chaos.exe

# 只发布当前主机平台时，设置 partial 并设置对应二进制，例如：
export PUBLISH_NPM_ALLOW_PARTIAL=1
export CHAOS_LINUX_X64=/artifacts/linux-x64/chaos

# 正式全平台发布时，取消 partial 模式；并取消上面六个平台路径的注释。
# unset PUBLISH_NPM_ALLOW_PARTIAL

# 3. 组装并 dry-run
./scripts/ci/local-publish-host.sh
# 可选指定版本：./scripts/ci/local-publish-host.sh --version 0.4.2

# 4. 确认 dry-run 无误后真正发布；partial 模式仅发布平台包，不发布元包
./scripts/ci/local-publish-host.sh --publish
```

脚本会：

1. 校验所选平台二进制都已提供；全量模式要求六个平台齐全
2. brotli 压缩并组装所选 `chaos-code-<platform>` 包
3. 全量模式依次发布六个平台包再发元包；partial 模式只发布平台包；两种模式均核验每个发布版本已出现在 npm registry

**限制**：npm 元包固定声明六个平台包，因此缺任一平台时会拒绝发布元包。当前 Windows 两个包名处于 npm 安全占位状态，本机模式也无法绕过；全平台安装需先由包所有者通过 npm 支持解决，再使用路径 B。

---

## 路径 B：CI 六平台（需要 Secret）

### 一次性准备

1. **npm 账号**有权发布无 scope 包名 `chaos-code` 与可发布的 `chaos-code-*` 包，且 `NPM_TOKEN` 有对应权限。先用实际 token 在安全环境里验证，不能只看 secret 是否存在。`chaos-code-win32-arm64` 与 `chaos-code-win32-x64` 当前为 npm `0.0.1-security` 保留包名，未解决前全量 npm 发布会失败；不要擅自改平台包名或 pins。
   （不要用 `chaos-cli`：该名已被 npm 废弃占位。）
2. **先 push workflow**（否则远程没有 Actions 定义）：
   ```bash
   git add .github/workflows scripts/ci crates/codegen/xai-grok-pager/npm
   git commit -m "ci: release workflow for multi-platform npm publish"
   git push origin main
   ```
3. 只有 npm 支持解除 Windows 包名占位后，才添加具有发布权限的 `NPM_TOKEN`，并将仓库变量 `CHAOS_NPM_PUBLISH_ENABLED` 设为 `true`。否则保持默认关闭，由 tag 只发布 GitHub Release。
4. workflow 已声明 `contents: write`（创建 Release 用 `GITHUB_TOKEN`，无需自建）。

### 发版步骤

#### 打 tag（推荐）

```bash
# bump 版本后（CI 会把 npm package.json stamp 到 tag 版本）：
git tag v0.4.2
git push origin v0.4.2
```

`Release` workflow：`resolve-version` → 矩阵 `build` → `package`  
默认只执行 build + GitHub Release。启用 `CHAOS_NPM_PUBLISH_ENABLED=true` 后才会额外执行 npm 发布和 registry 逐包核验；任何 npm 错误都会使 package job 失败，而 GitHub Release 步骤使用 `always()`，只要二进制布局成功仍会上传产物。因 tag 不可改写，修复 npm 阻塞后应以新补丁版本发布。

#### 手动跑 workflow

GitHub → Actions → **Release** → **Run workflow**：

- `version`：如 `0.4.2`（可空则读 package.json）
- `publish_npm`：是否写 npmjs；默认为关闭；启用前需有可发布各包的 `NPM_TOKEN`，且 Windows 包名已解除 npm 安全占位
- `create_github_release`：是否上传二进制到 Releases

## 本地调试（非发版，不 publish）

仅当前主机：

```bash
cargo build -p xai-grok-pager-bin --profile release-dist
# 产物：target/<triple>/release-dist/chaos

ONLY_PLATFORMS="linux-x64" node crates/codegen/xai-grok-pager/npm/chaos/scripts/assemble-platform-packages.js
# 将 linux-x64 换成当前主机平台；仅用于本地验证，不要发布此不完整集合。

cd crates/codegen/xai-grok-pager/npm
npm install -g ./chaos-linux-x64 ./chaos   # 按本机平台改目录名
chaos --version
```

Dry-run 发布（不真正 push registry）：

```bash
DRY_RUN=1 NPM_TOKEN=dummy scripts/ci/publish-npm.sh
```

`publish-npm.sh` 的 dry-run 会校验六个平台二进制归档均已组装；不会连接 registry 或执行发布。

## 环境变量（assemble）

| Env | 含义 |
|-----|------|
| `CHAOS_LINUX_X64` | Linux x64 二进制路径 |
| `CHAOS_LINUX_ARM64` | Linux arm64 |
| `CHAOS_DARWIN_ARM64` | macOS Apple Silicon |
| `CHAOS_DARWIN_X64` | macOS Intel |
| `CHAOS_WIN32_X64` | Windows x64 `chaos.exe` |
| `CHAOS_WIN32_ARM64` | Windows arm64 `chaos.exe` |
| `ONLY_PLATFORMS` | 空格分隔的平台列表，例如 `linux-x64 darwin-arm64`；未设置时组装所有已提供的二进制 |

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
