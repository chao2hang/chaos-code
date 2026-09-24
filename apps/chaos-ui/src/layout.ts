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
