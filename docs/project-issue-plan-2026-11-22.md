# 2026-11-22：Issues #29–#36 修复与发版计划

> 本文是本次审查与分批发布依据，不另建复选清单；项目的维护勾选入口仍为根目录 `TODO.md`。Owner：本轮执行代理 lead；目标日期：2026-11-22 完成首批可验证修复，后续批次由项目维护者按依赖安排。关联 Issues：[ #29 ](https://github.com/chao2hang/chaos-code/issues/29) 至 [ #36 ](https://github.com/chao2hang/chaos-code/issues/36)。

## 当前事实

- 2026-11-22 14:42 UTC 从 GitHub API 确认新增且开放的 #29–#36 共 8 条，无 PR、评论、标签或指定维护者；此前“0 条开放”是这些 issue 创建前的查询结果。
- `origin/main`/本地 HEAD 为 `7df6c2c24`；最新 GitHub Release 为 `v1.0.1`。上轮已有未提交的发布签名校验、推理 `none` 错误分类和 PTY 忽略原因修复，必须保留并测试，不能把它们冒充本次 8 条 issue 已完成。
- 当前环境无 GitHub HTTPS 写凭据，SSH 也未授权；本轮无法在该状态下推送 tag/发布。已在仓库忽略的 `target/` 安装固定 Rust 1.94.0、按仓库哈希核验 protoc 29.3，并安装 Node 20；上轮 `reasoning_effort=none` 的定向 Rust 回归与版本/npm/发布目标门禁通过，但新 issue 与全仓测试仍待完成。

## 依赖与验收目标

| 阶段 | Issue | 目标与安全边界 | 发版口径 |
| --- | --- | --- | --- |
| 第一批：可靠性 | #29、#30 | 类型化的无头终止原因与退出码；对已知 provider 工具/推理不兼容组合先校验或有界重试。`--client` 现只标识客户端，不等于强制切换 endpoint；未知远端能力无法凭空保证零 token 失败。用本地 mock 验证 400、max-turns、拒绝权限、正常结果；不能把模型答复当成文件质量验收。 | 只有完整回归/兼容性评审通过才纳入下一版本。 |
| 第二批：可观测性 | #36、#32 | 先约定统一 `run.exit_reason`、`turns` 与实际工具结果，再增加 JSON run report 和逐行 flush 的 events；报告路径/哈希/命令只写可证实的事实，任意 shell 的隐式写入必须显式注明采集边界。NDJSON 需 10 秒静默心跳和标准输出隔离。 | 新增 CLI 行为宜发布次版本，而非声称为纯补丁。 |
| 第三批：预算与工具 | #33、#31、#34 | 先保证 token 使用量有可信来源，未知价格不得凭估价声明 `--max-cost` 生效；doctor 依赖 #30 能力契约，探测本身可能收费；unified diff 不等于既有自定义 apply_patch 格式，`--check` 必须输出可解析 JSON 且不写会话状态。 | 各自通过真实黑盒测试后纳入次版本。 |
| 安全专项 | #35 | 新建 fail-closed `workdir` 策略，不把既有 `workspace/strict` 当别名；必须覆盖 `/tmp/b`、symlink/`..`、rename、chmod、子进程及原生编辑，记录可信拒绝事件。默认开启须以 Linux/macOS/Windows 强制执行测试和独立安全审查为前提。 | 未达成跨出口验收绝不默认开启或宣称已修复。 |

## 发布门禁

1. 保留上轮未提交修改；相关 Rust 测试、`cargo fmt --check`、必要的 check/clippy、Node/Python 发布脚本及静态安全门禁必须对同一树通过。全仓现存其他红灯要逐项定性，不因本批目标门禁绿而冒称全绿。
2. #29 退出码改变下游编排行为；如最终包含新 CLI 功能，按语义化版本选择次版本（候选 `v1.1.0`）；若仅发布兼容修复才用补丁版（候选 `v1.0.2`）。选择必须与实际提交范围及 CHANGELOG 一致。
3. 只有远端写凭据、签名预检、必需平台构建、校验和、签名、SBOM 和 GitHub Release 工作流完整验收具备后，才能推送 tag 并称为“已发布”。Windows npm 包名/registry 按已有门禁继续隔离，不擅自开启 npm 发布。
4. 提交前复查 `git diff --check`、敏感信息扫描、工作树与 `origin/main` 同步；推送后再次读 GitHub commit/tag/Release 和资产列表，绝不把本地 tag 或排队中的 workflow 当作发布成功。
