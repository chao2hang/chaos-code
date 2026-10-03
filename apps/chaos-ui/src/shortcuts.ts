// The keyboard shortcuts the app actually handles.
//
// This table is the handler, not a description of it: `main.tsx` asks
// `matchShortcut` what a key press means and acts on the answer, so the list the
// settings panel renders cannot drift from the keys that do something.
// `every_shortcut_is_wired` in `shortcuts.test.ts` fails if a row here has no
// branch in `main.tsx` to perform it.

/** Tabs the shell can switch to, in the order `Ctrl/Cmd + <n>` reaches them. */
export const SHORTCUT_TABS = ['chat', 'files', 'git', 'terminal', 'settings', 'marketplace', 'diff'] as const

export type ShortcutTab = (typeof SHORTCUT_TABS)[number]

export type Shortcut = {
  /** The action a press performs; `main.tsx` switches on this. */
  id: string
  /**
   * The binding with the platform modifier left as the literal token `Mod`, so
   * the table is the same text on every machine and `formatShortcut` decides how
   * to spell it for the reader's OS.
   */
  keys: string
  /** What it does, in the words the panel shows. */
  label: string
}

/**
 * One entry per key the shell listens for.
 *
 * Every binding carries a modifier on purpose. The composer handles bare
 * `Escape`, `Enter`, `ArrowUp` and `ArrowDown` for its own suggestions and
 * history, and a shell-level binding on a bare key would swallow those.
 */
export const SHORTCUTS: readonly Shortcut[] = [
  ...SHORTCUT_TABS.map((tab, index) => ({
    id: `tab:${tab}`,
    keys: `Mod+${index + 1}`,
    label: `切换到「${tabTitle(tab)}」`,
  })),
  { id: 'theme:cycle', keys: 'Mod+Shift+L', label: '切换主题（深色 / 浅色 / 跟随系统）' },
  { id: 'run:cancel', keys: 'Mod+.', label: '取消正在运行的回复' },
  { id: 'composer:focus', keys: 'Mod+K', label: '聚焦输入框' },
]

/** The subset of a keyboard event this module needs, so tests need no DOM. */
export type KeyEvent = {
  key: string
  ctrlKey: boolean
  metaKey: boolean
  shiftKey: boolean
  altKey: boolean
}

export function isApplePlatform(platform: string): boolean {
  return /Mac|iPhone|iPad/i.test(platform)
}

/** Spells the `Mod` token for the reader's OS: `Cmd` on Apple, `Ctrl` elsewhere. */
export function formatShortcut(keys: string, platform = ''): string {
  return keys.replace('Mod', isApplePlatform(platform) ? 'Cmd' : 'Ctrl')
}

/**
 * The same binding in the vocabulary `aria-keyshortcuts` accepts, which names the
 * physical modifiers (`Control`, `Meta`) rather than the ones a style guide prints.
 */
export function ariaShortcut(keys: string, platform = ''): string {
  return keys.replace('Mod', isApplePlatform(platform) ? 'Meta' : 'Control')
}

function tabTitle(tab: ShortcutTab): string {
  switch (tab) {
    case 'chat':
      return '对话'
    case 'files':
      return '文件'
    case 'git':
      return 'Git'
    case 'terminal':
      return '终端'
    case 'settings':
      return '设置'
    case 'marketplace':
      return '插件市场'
    case 'diff':
      return '变更审查'
  }
}

export function tabForShortcut(id: string): ShortcutTab | null {
  if (!id.startsWith('tab:')) return null
  const tab = id.slice('tab:'.length)
  return (SHORTCUT_TABS as readonly string[]).includes(tab) ? (tab as ShortcutTab) : null
}

/**
 * Which shortcut a key press means, or null for "not ours, leave it alone".
 *
 * `Mod` means exactly one of Ctrl or Cmd. `Alt` is always somebody else's
 * binding — window managers and the browser itself take those — so a press
 * carrying it is never ours.
 */
export function matchShortcut(event: KeyEvent): string | null {
  if (event.altKey) return null
  const modCount = Number(event.ctrlKey) + Number(event.metaKey)
  if (modCount !== 1) return null
  // Only `shiftKey` decides the shifted bindings. Browsers report the case per
  // CapsLock without setting Shift, and a press that never held Shift must not be
  // read as one that did.
  const key = event.key.length === 1 ? event.key.toLowerCase() : event.key
  if (/^[1-9]$/.test(key) && !event.shiftKey) {
    const tab = SHORTCUT_TABS[Number(key) - 1]
    return tab ? `tab:${tab}` : null
  }
  if (key === 'l') return event.shiftKey ? 'theme:cycle' : null
  if (key === 'k') return event.shiftKey ? null : 'composer:focus'
  if (key === '.') return event.shiftKey ? null : 'run:cancel'
  return null
}
