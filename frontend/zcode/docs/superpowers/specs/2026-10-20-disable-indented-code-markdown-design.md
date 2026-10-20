# 禁用 CommonMark 缩进代码块以避免列表内容碎裂为多张空代码卡片

日期：2026-10-20
状态：设计 → 实施
影响面：`packages/ui/src/components/ai-elements/message.tsx`（挂载 `disableIndentedCodeRemarkPlugin`），不改变其它层级契约。

## 背景

在用户与助手对话时，当助手输出带有列表项且换行对齐缩进的内容（例如 JavaScript 三元运算符分支、长逻辑条件或带缩进的代码片段）时，界面上原本连续的逻辑会被碎裂拆分成多个独立的代码卡片，每个卡片都被打上默认语言标签 `text`，并带有完整的卡片边框、内边距和右上角操作按钮。

典型场景示例：
```markdown
* const forceOpenHistory = shouldForceOpenAbnormalHistory(unit.header, options.sessionPhase);
* const assistantHistoryDefaultOpen =
* unit.workSegments && unit.workSegments.length > 0
    ? !unit.timelineOnly &&
      (forceOpenHistory ||
        (isLastTurn && unit.workSegments.at(-1)?.workStatus?.state === "running") ||
        (unit.workSegments.length === 1 &&
          unit.latestAssistantTextRow === undefined &&
          unit.assistantWorkRows.length > 0))
* }; });
```

在现有渲染逻辑下，从第 4 行开始的每一行缩进内容都被解析成了一个独立的 `code` 节点，导致短小的一段逻辑被切割成 6 个臃肿的 `text` 卡片，排版极其破碎丑陋。

## 根因分析

1. **CommonMark 缩进代码块规范（Indented Code Block）**：
   CommonMark 规范规定，在无序列表项标记或普通段落之后，若出现 4 个或更多空格的缩进，解析器默认将其识别为缩进代码块（AST 节点为 `type: "code"`, `lang: null`）。
2. **AI 场景下模型的输出习惯**：
   LLM 在格式化伪代码、表达式或逻辑推理时，经常在列表项换行后使用 4~8 个空格进行美化缩进，但并未包裹标准 Markdown 围栏（```）。
3. **前端渲染器的映射**：
   `MessageResponse` 将 AST 中的每一个 `code` 节点映射为完整的 `CodeBlock` 组件，未指定语言时回退为 `text`。带有大边框和外边距的代码块嵌套在 `li` 中，直接破坏了视觉层次。

## 目标与方案

在现代 AI 交互平台中，真正的代码块均使用围栏代码块（Fenced Code Blocks，如 ```` ```ts ````）。缩进代码块几乎 100% 属于意外触发的误伤。

因此，在 Markdown 解析（micromark/remark）层面彻底禁用 `codeIndented`：
- **微标记扩展（micromarkExtensions）**：通过 unified 插件注入 `{ disable: { null: ["codeIndented"] } }`。
- **效果**：
  - 列表项内部带缩进的行保持为所属列表项的自然段落内容，不会被打断或拆分。
  - 标准围栏代码块（```）与行内代码（`...`）保持原有解析与高亮能力，完全不受影响。
- **改动范围**：仅在 `@zcode/ui` 的 `MessageResponse` 渲染管线挂载 `disableIndentedCodeRemarkPlugin`。
