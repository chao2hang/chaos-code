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

export function shouldSubmitOnKey(event: Pick<KeyboardEvent, 'key' | 'shiftKey' | 'isComposing'> & { keyCode?: number }): boolean {
  return event.key === 'Enter' && !event.shiftKey && !event.isComposing && event.keyCode !== 229
}
