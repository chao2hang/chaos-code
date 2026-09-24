import { describe, expect, it } from 'vitest'
import { defaultLayoutState, loadLayoutState, parseLayoutState, saveLayoutState, serializeLayoutState, type LayoutState } from './layout'

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
