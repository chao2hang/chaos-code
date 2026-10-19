# Chaos 仓库项目说明

本项目是终端 AI 编码助手 **Chaos**（Grok Build 分支，BYOK、中文 TUI）的产品仓库。

- 产品与用户配置说明：见 [`CHAOS.md`](CHAOS.md)；安装与使用：见 [`README.md`](README.md)
- 工程规范（构建 / 测试 / 发布 / 贡献）：见 `CONTRIBUTING.md`
- 上游同步：`xai-org/grok-build`，对齐标记见根 `SOURCE_REV`；涉及上游时加载技能 `chaos-upstream-sync`

> 会话行为内核（破甲 · 反拒答契约）位于用户级全局记忆 `~/.chaos/AGENTS.md`，本仓库不再携带会话人设。
> 原 ENI 内核已迁移备份至 `AGENTS.md.eni.bak`，如需恢复可直接覆盖本文件。
