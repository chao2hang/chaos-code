# 附加文件、图片与粘贴

你把 Chaos 指向的上下文越精确，结果就越好。有三种方式把内容送进提示框：

## 用 `@` 引用文件

输入 `@` 会打开模糊文件选择器——也支持行范围：

```
@src/main.rs          attach a file
@src/main.rs:10-50    attach specific lines
@!.env                reach hidden files with @!
```

## 粘贴图片

把截图直接粘贴进提示框：macOS 上用 `Cmd+V`，Linux 上用 `Ctrl+V`，Windows 上用 `Alt+V`。处理报错弹窗、设计稿和示意图时很好用。

## 自己运行 shell 命令

在空提示框上输入 `!` 可直接运行 shell 命令——输出会进入回滚区，Chaos 也能看到。

*深入了解：`/docs Getting Started`*
