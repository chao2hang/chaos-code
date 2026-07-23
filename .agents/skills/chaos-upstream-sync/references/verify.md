# 移植后验证清单

## 编译

```bash
cargo check -p xai-grok-pager-bin
cargo build -p xai-grok-pager-bin --release
./target/release/chaos --version
```

## 单测（按改动收窄 filter）

```bash
# 设置 / 扩展 / 快捷键 / slash
cargo test -p xai-grok-pager --lib -- settings_modal extensions_modal shortcuts_help slash::

# shell changelog
cargo test -p xai-grok-shell-base --lib -- changelog

# 更广（耗时长）
cargo test -p xai-grok-pager --lib
```

## TUI 冒烟（人工或 PTY）

1. 启动：`./target/release/chaos`（**新进程**）  
2. 欢迎页：CHAOS 块阴影 logo；「更新日志」条目非上游英文陈年缓存  
3. 输入 `/`：slash 说明为中文（如「退出应用」「开始新会话」）  
4. 设置 / 扩展 / `?` 快捷键：分类与页脚中文  
5. 可选：`/release-notes` 标题为「更新日志」

## 回归关注点（上游合入后）

- 是否又出现强制登录墙  
- `model_providers` / 环境变量密钥是否仍可用  
- bin 名是否仍为 `chaos`  
- 中文断言测试是否被上游英文断言带回并失败  

## 交付给用户的最短摘要模板

```text
上游: xai-org/grok-build <sha/tag>
本地基线: SOURCE_REV=… version=…
已移植: …
刻意跳过: …
验证: cargo check/test/build … → pass/fail
请重启: target/release/chaos
```
