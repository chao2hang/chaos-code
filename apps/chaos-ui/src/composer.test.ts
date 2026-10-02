import { describe, expect, it } from 'vitest'
import { getComposerSuggestions, initialComposerHistory, moveSuggestionIndex, navigatePromptHistory, recordPrompt, shouldSubmitOnKey } from './composer'

describe('composer history and keyboard behavior', () => {
  it('records unique adjacent prompts and restores the draft after navigating back', () => {
    let history = recordPrompt(initialComposerHistory, 'first prompt')
    history = recordPrompt(history, 'first prompt')
    history = recordPrompt(history, 'second prompt')
    expect(history.entries).toEqual(['first prompt', 'second prompt'])
    const previous = navigatePromptHistory(history, -1, 'unsent draft')
    expect(previous.value).toBe('second prompt')
    const older = navigatePromptHistory(previous.history, -1, previous.value)
    expect(older.value).toBe('first prompt')
    const next = navigatePromptHistory(older.history, 1, older.value)
    expect(next.value).toBe('second prompt')
    const draft = navigatePromptHistory(next.history, 1, next.value)
    expect(draft.value).toBe('unsent draft')
  })

  it('preserves a pending draft when a prompt is sent before draft restoration', () => {
    let history = navigatePromptHistory(
      { entries: ['first prompt'], index: 1, draft: 'unfinished follow-up' },
      -1,
      'unfinished follow-up',
    ).history
    history = recordPrompt(history, 'sent from history')
    const latest = navigatePromptHistory(history, -1, '')
    expect(latest.value).toBe('sent from history')
    expect(navigatePromptHistory(latest.history, 1, latest.value).value).toBe('unfinished follow-up')
  })

  it('does not navigate prompt history during IME composition', () => {
    const history = { entries: ['previous prompt'], index: 1, draft: 'draft in progress' }
    expect(navigatePromptHistory(history, -1, 'draft in progress', true)).toEqual({ history, value: 'draft in progress' })
    expect(navigatePromptHistory(history, 1, 'draft in progress', true)).toEqual({ history, value: 'draft in progress' })
  })

  it('keeps blank submission history unchanged', () => {
    const history = recordPrompt({ entries: ['previous'], index: 1, draft: 'unsent' }, '   ')
    expect(history).toEqual({ entries: ['previous'], index: 1, draft: 'unsent' })
  })

  it('wraps keyboard selection over available suggestions and handles empty lists', () => {
    expect(moveSuggestionIndex(-1, 3, 1)).toBe(0)
    expect(moveSuggestionIndex(0, 3, -1)).toBe(2)
    expect(moveSuggestionIndex(1, 3, 1)).toBe(2)
    expect(moveSuggestionIndex(0, 0, 1)).toBe(0)
  })

  it('submits Enter but preserves Shift+Enter and IME composition input', () => {
    expect(shouldSubmitOnKey({ key: 'Enter', shiftKey: false, isComposing: false })).toBe(true)
    expect(shouldSubmitOnKey({ key: 'Enter', shiftKey: true, isComposing: false })).toBe(false)
    expect(shouldSubmitOnKey({ key: 'Enter', shiftKey: false, isComposing: true })).toBe(false)
    expect(shouldSubmitOnKey({ key: 'Enter', shiftKey: false, isComposing: false, keyCode: 229 })).toBe(false)
  })

  it('filters slash commands and file mentions for autocomplete', () => {
      const slashMatches = getComposerSuggestions('/app')
      expect(slashMatches.some((cmd) => cmd.label === '/approve-tool')).toBe(true)
      expect(slashMatches.every((cmd) => cmd.kind === 'command')).toBe(true)

      const fileMatches = getComposerSuggestions('check this @main', ['src/main.tsx', 'src/style.css', 'README.md'])
      expect(fileMatches).toHaveLength(1)
      expect(fileMatches[0].label).toBe('@src/main.tsx')
      expect(fileMatches[0].insert).toBe('check this @src/main.tsx ')

    expect(getComposerSuggestions('regular prompt')).toEqual([])
  })
})
