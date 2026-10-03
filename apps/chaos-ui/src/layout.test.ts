import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import {
  COMPACT_BREAKPOINT_PX,
  COMPACT_VIEWPORT_QUERY,
  defaultLayoutState,
  loadLayoutState,
  parseLayoutState,
  resolveFocusWrap,
  resolveSidebarVisibility,
  saveLayoutState,
  serializeLayoutState,
  type LayoutState,
} from './layout'

class MemoryStorage {
  private values = new Map<string, string>()
  getItem(key: string) { return this.values.get(key) ?? null }
  setItem(key: string, value: string) { this.values.set(key, value) }
}

describe('layout persistence and recovery', () => {
  it('round-trips the shipped layout through storage', () => {
    const storage = new MemoryStorage()
    const layout: LayoutState = { version: 1, sidebarWidth: 300, composerHeight: 180, theme: 'light', panelOpen: false }
    expect(saveLayoutState(storage, layout)).toBe(true)
    expect(loadLayoutState(storage)).toEqual(layout)
    expect(JSON.parse(serializeLayoutState(layout))).toEqual(layout)
  })

  it('falls back on malformed, incompatible and out-of-range layout values', () => {
    expect(parseLayoutState('{broken')).toEqual(defaultLayoutState)
    expect(parseLayoutState(JSON.stringify({ version: 2, sidebarWidth: 300, composerHeight: 180, theme: 'dark', panelOpen: true }))).toEqual(defaultLayoutState)
    expect(parseLayoutState(JSON.stringify({ version: 1, sidebarWidth: 900, composerHeight: 10, theme: 'dark', panelOpen: true }))).toEqual({ ...defaultLayoutState, sidebarWidth: 480, composerHeight: 88 })
  })

  it('clamps dimensions to safe bounds and retains valid display preferences', () => {
    expect(parseLayoutState(JSON.stringify({ version: 1, sidebarWidth: 300, composerHeight: 150, theme: 'system', panelOpen: false })))
      .toEqual({ version: 1, sidebarWidth: 300, composerHeight: 150, theme: 'system', panelOpen: false })
  })

  it('survives storage being unavailable or throwing', () => {
    const storage = { getItem: () => { throw new Error('blocked') }, setItem: () => { throw new Error('blocked') } }
    expect(loadLayoutState(storage)).toEqual(defaultLayoutState)
    expect(saveLayoutState(storage, defaultLayoutState)).toBe(false)
  })
})

// The drawer's position comes from the stylesheet and its behaviour from the media
// query in `main.tsx`. If those two numbers drift, the affordance keeps flipping a
// state the layout is no longer listening to, and nothing on screen explains it.
const MAIN_SOURCE = readFileSync(new URL('main.tsx', import.meta.url), 'utf8')
const STYLE_SOURCE = readFileSync(new URL('style.css', import.meta.url), 'utf8')

function mediaBlocks(): { breakpoint: number; body: string }[] {
  const blocks: { breakpoint: number; body: string }[] = []
  const pattern = /@media\s*\(max-width:\s*(\d+)px\)\s*\{/g
  let match: RegExpExecArray | null
  while ((match = pattern.exec(STYLE_SOURCE))) {
    let depth = 1
    let cursor = match.index + match[0].length
    while (cursor < STYLE_SOURCE.length && depth > 0) {
      const char = STYLE_SOURCE[cursor]
      if (char === '{') depth += 1
      else if (char === '}') depth -= 1
      cursor += 1
    }
    blocks.push({ breakpoint: Number(match[1]), body: STYLE_SOURCE.slice(match.index + match[0].length, cursor - 1) })
  }
  return blocks
}


describe('compact viewport sidebar', () => {
  it('shows the drawer only on a phone and only when it was opened', () => {
    expect(resolveSidebarVisibility({ compact: true, drawerOpen: true, panelOpen: false })).toBe(true)
    expect(resolveSidebarVisibility({ compact: true, drawerOpen: true, panelOpen: true })).toBe(true)
    expect(resolveSidebarVisibility({ compact: true, drawerOpen: false, panelOpen: true })).toBe(false)
    expect(resolveSidebarVisibility({ compact: true, drawerOpen: false, panelOpen: false })).toBe(false)
  })

  it('leaves the docked sidebar to the persisted preference', () => {
    // The drawer state is ephemeral. Honouring it while wide would show a sidebar the
    // user collapsed, and writing it back would collapse a docked sidebar on reload.
    expect(resolveSidebarVisibility({ compact: false, drawerOpen: true, panelOpen: false })).toBe(false)
    expect(resolveSidebarVisibility({ compact: false, drawerOpen: false, panelOpen: true })).toBe(true)
  })

  it('shares one breakpoint between the stylesheet and the query the shell listens to', () => {
    expect(COMPACT_VIEWPORT_QUERY).toBe(`(max-width: ${COMPACT_BREAKPOINT_PX}px)`)
    const drawerRules = (body: string) => [
      /\.sidebar-col\s*\{[^}]*position:\s*fixed/.test(body),
      /\.sidebar-backdrop\s*\{/.test(body),
    ].filter(Boolean).length

    const compactBlocks = mediaBlocks().filter(({ breakpoint }) => breakpoint === COMPACT_BREAKPOINT_PX)
    expect(compactBlocks).toHaveLength(1)
    expect(drawerRules(compactBlocks[0].body), 'the drawer and its scrim must be laid out under that breakpoint').toBe(2)
    const elsewhere = mediaBlocks()
      .filter(({ breakpoint }) => breakpoint !== COMPACT_BREAKPOINT_PX)
      .filter(({ body }) => drawerRules(body) > 0)
      .map(({ breakpoint }) => breakpoint)
    expect(elsewhere, 'a second breakpoint would leave the toggle driving a drawer nothing positions').toEqual([])
  })

  it('wires both sidebar toggles through the compact-aware visibility', () => {
    expect(MAIN_SOURCE).toContain('resolveSidebarVisibility({ compact, drawerOpen: sidebarDrawerOpen, panelOpen: layout.panelOpen })')
    expect(MAIN_SOURCE).toContain('style={{ display: sidebarVisible ?')
    // Neither toggle may flip the docked preference directly.
    expect(MAIN_SOURCE.match(/onClick=\{togglePanel\}/g) ?? []).toEqual([])
    expect(MAIN_SOURCE.match(/onClick=\{toggleSidebar\}/g)?.length).toBe(2)
  })

  it('wraps Tab at the drawer edges and nowhere else', () => {
    // Forward off the end and backward off the front are the only two edges.
    expect(resolveFocusWrap({ count: 5, activeIndex: 4, shiftKey: false })).toBe('first')
    expect(resolveFocusWrap({ count: 5, activeIndex: 0, shiftKey: true })).toBe('last')
    expect(resolveFocusWrap({ count: 5, activeIndex: 1, shiftKey: false })).toBeNull()
    expect(resolveFocusWrap({ count: 5, activeIndex: 3, shiftKey: true })).toBeNull()
    expect(resolveFocusWrap({ count: 5, activeIndex: 2, shiftKey: false })).toBeNull()
    // A single-control drawer wraps onto itself in both directions.
    expect(resolveFocusWrap({ count: 1, activeIndex: 0, shiftKey: false })).toBe('first')
    expect(resolveFocusWrap({ count: 1, activeIndex: 0, shiftKey: true })).toBe('last')
    // Nothing to wrap when there are no controls or focus is not on one of them.
    expect(resolveFocusWrap({ count: 0, activeIndex: 0, shiftKey: false })).toBeNull()
    expect(resolveFocusWrap({ count: 5, activeIndex: -1, shiftKey: true })).toBeNull()
  })

  it('routes the drawer keydown and the scrim-covered regions through the shipped helpers', () => {
    expect(MAIN_SOURCE).toContain('onKeyDown={trapDrawerFocus}')
    expect(MAIN_SOURCE).toContain('resolveFocusWrap({')
    expect(MAIN_SOURCE.match(/inert=\{drawerCoversShell\}/g)?.length).toBe(2)
    expect(MAIN_SOURCE).toContain('const drawerCoversShell = compact && sidebarDrawerOpen')
    // Leaving the compact breakpoint has to close the drawer, or the stale state
    // resurrects the overlay the next time the window is narrowed.
    expect(MAIN_SOURCE).toContain('if (!compact) setSidebarDrawerOpen(false)')
  })

  it('renders the brand heading in the header exactly while the sidebar is hidden', () => {
    // The sidebar carries the brand unconditionally and its `display` follows
    // `sidebarVisible`, so the header copy must be gated on that same flag. A narrower
    // gate (e.g. `compact`) leaves a collapsed desktop sidebar with no level-1 heading
    // anywhere in the accessibility tree; a wider one shows the brand twice.
    expect(MAIN_SOURCE.match(/className="brand-text/g)?.length).toBe(2)
    const gate = /\{(!?[\w.]+) && \(\s*<>\s*<h1 className="brand-text header-brand"/u.exec(MAIN_SOURCE)
    expect(gate, 'the header brand must be conditionally rendered').not.toBeNull()
    expect(gate?.[1], 'the header brand must cover every case in which the sidebar is hidden').toBe('!sidebarVisible')
  })
})
