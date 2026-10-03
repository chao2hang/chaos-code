import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import {
  ariaShortcut,
  formatShortcut,
  isApplePlatform,
  matchShortcut,
  SHORTCUTS,
  SHORTCUT_TABS,
  tabForShortcut,
  type KeyEvent,
} from './shortcuts'

const MAIN_SOURCE = readFileSync(new URL('main.tsx', import.meta.url), 'utf8')

/**
 * The physical key a binding ends with, in the case a press reports it in when
 * Shift is not held. The shift state comes from the binding, not from this.
 */
function lastKey(keys: string): string {
  const parts = keys.split('+')
  return (parts[parts.length - 1] ?? '').toLowerCase()
}

function press(key: string, modifiers: Partial<Omit<KeyEvent, 'key'>> = {}): KeyEvent {
  return { key, ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, ...modifiers }
}

/** The text between a function's declaration and its closing brace. */
function functionBody(source: string, declaration: string): string {
  const start = source.indexOf(declaration)
  if (start < 0) throw new Error(`main.tsx has no ${declaration}`)
  let depth = 0
  for (let index = source.indexOf('{', start); index < source.length; index += 1) {
    if (source[index] === '{') depth += 1
    if (source[index] === '}') {
      depth -= 1
      if (depth === 0) return source.slice(start, index + 1)
    }
  }
  throw new Error(`unbalanced braces after ${declaration}`)
}

describe('shortcut table', () => {
  it('binds each number key to the tab rendered in that position', () => {
    const renderedOrder = [...MAIN_SOURCE.matchAll(/aria-keyshortcuts=\{ariaShortcut\(tabBinding\('([^']+)'\)/g)].map((match) => match[1])
    expect(renderedOrder).toEqual([...SHORTCUT_TABS])
    SHORTCUT_TABS.forEach((tab, index) => {
      expect(matchShortcut(press(String(index + 1), { ctrlKey: true }))).toBe(`tab:${tab}`)
      expect(matchShortcut(press(String(index + 1), { metaKey: true }))).toBe(`tab:${tab}`)
    })
  })

  it('leaves bare and alt-modified keys to the composer and the window manager', () => {
    // The composer handles bare Escape/Enter/ArrowUp/ArrowDown/1-9 itself, and the
    // window manager owns Alt. A shell binding on either would swallow them.
    expect(matchShortcut(press('1'))).toBeNull()
    expect(matchShortcut(press('k'))).toBeNull()
    expect(matchShortcut(press('l', { shiftKey: true }))).toBeNull()
    SHORTCUTS.forEach((shortcut) => {
      const bare = press(lastKey(shortcut.keys))
      expect(matchShortcut(bare)).toBeNull()
      expect(matchShortcut(press(lastKey(shortcut.keys), { ctrlKey: true, altKey: true }))).toBeNull()
    })
  })

  it('requires exactly one of Ctrl and Cmd, so a stray Meta does not fire a binding', () => {
    expect(matchShortcut(press('.', { ctrlKey: true }))).toBe('run:cancel')
    expect(matchShortcut(press('.', { ctrlKey: true, metaKey: true }))).toBeNull()
    expect(matchShortcut(press('.', {}))).toBeNull()
  })

  it('distinguishes the shifted and unshifted forms it binds differently', () => {
    expect(matchShortcut(press('l', { ctrlKey: true, shiftKey: true }))).toBe('theme:cycle')
    expect(matchShortcut(press('l', { ctrlKey: true }))).toBeNull()
    // CapsLock reports 'L' without Shift. Nothing is bound to an unshifted Cmd/Ctrl+L,
    // so a CapsLock user pressing Ctrl+L must not get the theme cycle.
    expect(matchShortcut(press('L', { ctrlKey: true }))).toBeNull()
    expect(matchShortcut(press('k', { ctrlKey: true, shiftKey: true }))).toBeNull()
    expect(matchShortcut(press('.', { ctrlKey: true, shiftKey: true }))).toBeNull()
  })

  it('reaches every binding it advertises, including the tenth tab position as unset', () => {
    const ids = SHORTCUTS.map((shortcut) => matchShortcut(press(lastKey(shortcut.keys), {
      ctrlKey: true,
      shiftKey: shortcut.keys.includes('Shift'),
    })))
    expect(ids).toEqual(SHORTCUTS.map((shortcut) => shortcut.id))
    expect(matchShortcut(press('8', { ctrlKey: true }))).toBeNull()
    expect(matchShortcut(press('0', { ctrlKey: true }))).toBeNull()
  })

  it('spells Mod per platform for readers and for assistive tech', () => {
    expect(isApplePlatform('MacIntel')).toBe(true)
    expect(isApplePlatform('iPhone')).toBe(true)
    expect(isApplePlatform('Win32')).toBe(false)
    expect(isApplePlatform('')).toBe(false)
    expect(formatShortcut('Mod+1', 'MacIntel')).toBe('Cmd+1')
    expect(formatShortcut('Mod+1', 'Win32')).toBe('Ctrl+1')
    expect(formatShortcut('Mod+1')).toBe('Ctrl+1')
    expect(ariaShortcut('Mod+1', 'MacIntel')).toBe('Meta+1')
    expect(ariaShortcut('Mod+Shift+L', 'Win32')).toBe('Control+Shift+L')
  })

  it('resolves only its own tab ids', () => {
    expect(tabForShortcut('tab:settings')).toBe('settings')
    expect(tabForShortcut('theme:cycle')).toBeNull()
    expect(tabForShortcut('tab:settings:extra')).toBeNull()
    expect(tabForShortcut('tab:broadcast')).toBeNull()
  })

  it('lists each action and binding exactly once', () => {
    expect(new Set(SHORTCUTS.map((shortcut) => shortcut.id)).size).toBe(SHORTCUTS.length)
    expect(new Set(SHORTCUTS.map((shortcut) => shortcut.keys)).size).toBe(SHORTCUTS.length)
    expect(SHORTCUTS.every((shortcut) => shortcut.label.trim().length > 0)).toBe(true)
  })
})

describe('shortcut wiring in the shell', () => {
  const runShortcut = functionBody(MAIN_SOURCE, 'function runShortcut')

  it('performs every action the table advertises', () => {
    for (const shortcut of SHORTCUTS) {
      if (tabForShortcut(shortcut.id)) {
        expect(runShortcut).toContain('tabForShortcut(id)')
        expect(runShortcut).toContain('goToTab(tab)')
      } else {
        expect(runShortcut).toContain(`'${shortcut.id}'`)
      }
    }
    expect(runShortcut).toContain('cancel()')
    expect(runShortcut).toContain('composerRef.current?.focus()')
    expect(runShortcut).toContain('nextTheme(current.theme)')
  })

  it('listens once and claims the key before the browser can act on it', () => {
    expect(MAIN_SOURCE).toContain('window.addEventListener(\'keydown\', onKeyDown)')
    expect(MAIN_SOURCE).toContain('matchShortcut(event)')
    expect(MAIN_SOURCE).toContain('event.preventDefault()')
    // The listener outlives every render, so the action has to be read through a
    // ref refreshed each render or it would act on a stale session id.
    expect(MAIN_SOURCE).toContain('runShortcutRef.current = runShortcut')
  })

  it('has a panel for every tab the number keys can reach', () => {
    for (const tab of SHORTCUT_TABS) {
      if (tab === 'chat') continue
      expect(MAIN_SOURCE).toContain(`{activeTab === '${tab}' && (`)
    }
  })
})
