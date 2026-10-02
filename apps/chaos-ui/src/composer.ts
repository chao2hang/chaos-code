export type ComposerHistory = {
  entries: string[]
  index: number
  draft: string
}

export const initialComposerHistory: ComposerHistory = { entries: [], index: 0, draft: '' }

export function recordPrompt(history: ComposerHistory, prompt: string): ComposerHistory {
  const value = prompt.trim()
  if (!value) return history
  const entries = history.entries[history.entries.length - 1] === value ? history.entries : [...history.entries, value]
  return { entries, index: entries.length, draft: history.draft || prompt }
}

export function navigatePromptHistory(history: ComposerHistory, direction: -1 | 1, currentDraft: string, isComposing = false): { history: ComposerHistory; value: string } {
  if (isComposing) return { history, value: currentDraft }
  const draft = history.draft || currentDraft
  const atLatest = history.index >= history.entries.length
  if (direction === -1) {
    if (history.entries.length === 0) return { history: { ...history, draft }, value: currentDraft }
    const index = Math.max(0, Math.min(history.index, history.entries.length) - 1)
    return { history: { ...history, index, draft: atLatest && currentDraft ? currentDraft : draft }, value: history.entries[index] }
  }
  const index = Math.min(history.entries.length, history.index + 1)
  const value = index === history.entries.length ? draft : history.entries[index]
  return { history: { ...history, index, draft }, value }
}

export function moveSuggestionIndex(current: number, count: number, direction: -1 | 1): number {
  if (count <= 0) return 0
  return (current + direction + count) % count
}

export function shouldSubmitOnKey(event: Pick<KeyboardEvent, 'key' | 'shiftKey' | 'isComposing'> & { keyCode?: number }): boolean {
  return event.key === 'Enter' && !event.shiftKey && !event.isComposing && event.keyCode !== 229
}

export type ComposerSuggestion = {
  kind: 'command' | 'file'
  label: string
  insert: string
  description: string
}

export const SLASH_COMMANDS: ComposerSuggestion[] = [
  { kind: 'command', label: '/approve-tool', insert: '/approve-tool ', description: '发起工具审批请求' },
  { kind: 'command', label: '/ask', insert: '/ask ', description: '向用户发起确认问题' },
  { kind: 'command', label: '/review', insert: '/review ', description: '审查当前工作区代码变更' },
  { kind: 'command', label: '/explain', insert: '/explain ', description: '解释选中或打开的文件' },
  { kind: 'command', label: '/test', insert: '/test ', description: '生成并验证单元测试' },
]

export function getComposerSuggestions(input: string, fileEntries: string[] = []): ComposerSuggestion[] {
  const trimmed = input.trimStart()
  if (trimmed.startsWith('/') && !trimmed.includes(' ')) {
    const query = trimmed.toLowerCase()
    return SLASH_COMMANDS.filter((cmd) => cmd.label.startsWith(query))
  }
  const atMatch = /(?:^|\s)@([^\s]*)$/.exec(input)
  if (atMatch) {
    const query = atMatch[1].toLowerCase()
    return fileEntries
      .filter((entry) => entry.toLowerCase().includes(query))
      .slice(0, 8)
      .map((entry) => ({
        kind: 'file' as const,
        label: `@${entry}`,
        insert: input.replace(/@([^\s]*)$/, `@${entry} `),
        description: '工作区文件引用',
      }))
  }
  return []
}
