export type LayoutState = {
  version: 1
  sidebarWidth: number
  composerHeight: number
  theme: 'dark' | 'light' | 'system'
  panelOpen: boolean
}

export const defaultLayoutState: LayoutState = {
  version: 1,
  sidebarWidth: 240,
  composerHeight: 120,
  theme: 'dark',
  panelOpen: true,
}

const MIN_SIDEBAR_WIDTH = 160
const MAX_SIDEBAR_WIDTH = 480
const MIN_COMPOSER_HEIGHT = 88
const MAX_COMPOSER_HEIGHT = 480

export function parseLayoutState(raw: string | null): LayoutState {
  if (!raw) return defaultLayoutState
  try {
    const value: unknown = JSON.parse(raw)
    if (!value || typeof value !== 'object') return defaultLayoutState
    const candidate = value as Record<string, unknown>
    if (candidate.version !== 1) return defaultLayoutState
    if (typeof candidate.sidebarWidth !== 'number' || !Number.isFinite(candidate.sidebarWidth)) return defaultLayoutState
    if (typeof candidate.composerHeight !== 'number' || !Number.isFinite(candidate.composerHeight)) return defaultLayoutState
    if (candidate.theme !== 'dark' && candidate.theme !== 'light' && candidate.theme !== 'system') return defaultLayoutState
    if (typeof candidate.panelOpen !== 'boolean') return defaultLayoutState
    return {
      version: 1,
      sidebarWidth: Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, Math.round(candidate.sidebarWidth))),
      composerHeight: Math.min(MAX_COMPOSER_HEIGHT, Math.max(MIN_COMPOSER_HEIGHT, Math.round(candidate.composerHeight))),
      theme: candidate.theme,
      panelOpen: candidate.panelOpen,
    }
  } catch {
    return defaultLayoutState
  }
}

export function serializeLayoutState(layout: LayoutState): string {
  const validated = parseLayoutState(JSON.stringify(layout))
  return JSON.stringify(validated)
}

export function saveLayoutState(storage: Pick<Storage, 'setItem'>, layout: LayoutState): boolean {
  try {
    storage.setItem('chaos-ui-layout', serializeLayoutState(layout))
    return true
  } catch {
    return false
  }
}

export function loadLayoutState(storage: Pick<Storage, 'getItem'>): LayoutState {
  try {
    return parseLayoutState(storage.getItem('chaos-ui-layout'))
  } catch {
    return defaultLayoutState
  }
}

/**
 * Width at which the shell stops being a three-column grid. It has to be the same
 * number as the `@media (max-width: 768px)` block in `style.css`: the drawer is
 * positioned by that block, so a query that disagrees with it would leave the
 * open/close affordance driving a sidebar that is not actually a drawer.
 */
export const COMPACT_BREAKPOINT_PX = 768
export const COMPACT_VIEWPORT_QUERY = `(max-width: ${COMPACT_BREAKPOINT_PX}px)`

export type SidebarVisibility = {
  compact: boolean
  drawerOpen: boolean
  panelOpen: boolean
}

/**
 * The persisted `panelOpen` describes a docked sidebar. On a phone the sidebar is an
 * overlay drawer that starts closed and that the user opens per interaction, so
 * reusing that preference would cover the conversation on every load -- and writing
 * the drawer's state back would close the docked sidebar next time the window is wide.
 */
export function resolveSidebarVisibility({ compact, drawerOpen, panelOpen }: SidebarVisibility): boolean {
  return compact ? drawerOpen : panelOpen
}

/**
 * Where Tab has to be redirected so focus stays inside the open drawer. Everything the
 * scrim covers is already `inert`, which stops focus going *out* to the conversation but
 * leaves no way back in: past the drawer's last control the next Tab leaves the document
 * for the browser's own chrome, and from there it is not obvious how to get back.
 * Returning `null` means "let the browser move focus on its own".
 */
export function resolveFocusWrap({ count, activeIndex, shiftKey }: { count: number; activeIndex: number; shiftKey: boolean }): 'first' | 'last' | null {
  // `activeIndex < 0` means focus is not on one of the drawer's controls at all, so
  // there is no edge here to wrap at.
  if (count === 0 || activeIndex < 0) return null
  if (shiftKey) return activeIndex === 0 ? 'last' : null
  return activeIndex === count - 1 ? 'first' : null
}
