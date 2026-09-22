# 并行工作：worktree

想让 Chaos 在一个功能上干活，同时你（或另一个 Chaos 会话）在同一个仓库里做别的事？**Git worktree** 让每个会话拥有自己隔离的检出——互不踩踏对方的改动，也不用 stash。

## 在 worktree 中启动会话

- **在任何地方：** 按 `Ctrl+N`（连按两次确认）新建会话，然后选择 worktree 选项。
- **在欢迎页：** 在 git 仓库内按 `Ctrl+W`，打开「新建 worktree」对话框。
- **在 shell 里：**

  ```bash
  chaos --worktree=my-feature "refactor the auth module"
  ```

  （要用 `=`——否则提示会被当成 worktree 名。）

## 为什么这样很棒

- 在同一个仓库上同时跑两三个 Chaos 会话。
- 实验彼此隔离——某个改动行不通时，你的主检出毫发无损。
- 工作完成后，像普通 git 分支一样把改动应用回去。

**`/fork`** 把当前对话复制到一个并行会话——加一句指令可以让它指向某个任务：`/fork try the async approach`。

同时跑多个代理？**看板**（`/dashboard` 或 `Ctrl+\`）按状态分组显示每个会话——谁在等你输入、谁在干活、谁已完成。

*深入了解：`/docs Session Management`*
