import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import type { HostInfo } from './generated/protocol'
import { buildSettingsCategories, nextTheme, refusalSummary, THEME_ORDER, themeLabel, type SettingsInput } from './settings'
import { SHORTCUTS, formatShortcut } from './shortcuts'

const MAIN_SOURCE = readFileSync(new URL('main.tsx', import.meta.url), 'utf8')

const NOT_LOADED = 'host 尚未回报'

function hostInfo(overrides: Partial<HostInfo> = {}): HostInfo {
  return {
    host_version: '0.9.3',
    protocol_version: 7,
    bind_addr: '127.0.0.1:8787',
    state_backend: 'json_file',
    safe_web_mode: false,
    workspace_root: '/home/dev/project',
    token_required: true,
    public_origin: null,
    preview_proxy: 'disabled',
    preview_ports: [],
    update_mode: 'external',
    safe_mode_refusals: [
      { message: 'propose_file_write', capability: '把文件写进工作区' },
      { message: 'approve', capability: '批准待审操作' },
    ],
    ...overrides,
  }
}

function input(overrides: Partial<SettingsInput> = {}): SettingsInput {
  return { host: hostInfo(), theme: 'dark', model: null, baseUrl: null, hasApiKey: false, ...overrides }
}

function category(categories: ReturnType<typeof buildSettingsCategories>, id: string) {
  const found = categories.find((candidate) => candidate.id === id)
  if (!found) throw new Error(`no ${id} category`)
  return found
}

function row(categoryId: string, rowId: string, overrides: Partial<SettingsInput> = {}) {
  const found = category(buildSettingsCategories(input(overrides)), categoryId).rows.find((candidate) => candidate.id === rowId)
  if (!found) throw new Error(`no ${rowId} row in ${categoryId}`)
  return found
}

describe('settings categories', () => {
  it('covers the nine areas the checklist names, in that order', () => {
    const ids = buildSettingsCategories(input()).map((entry) => entry.id)
    expect(ids).toEqual(['general', 'appearance', 'model', 'provider', 'permissions', 'security', 'shortcuts', 'remote', 'updates'])
    const titles = buildSettingsCategories(input()).map((entry) => entry.title)
    expect(titles).toEqual(['通用', '外观', '模型', 'Provider', '权限', '安全', '快捷键', '远程', '更新'])
  })

  it('says the host has not reported instead of showing a plausible default', () => {
    const categories = buildSettingsCategories(input({ host: null }))
    const hostRows = [
      'general-version', 'general-protocol', 'general-bind', 'general-state',
      'permissions-safe-mode', 'permissions-workspace',
      'security-token', 'security-origin', 'security-preview',
      'updates-mode',
    ]
    const values = new Map(categories.flatMap((entry) => entry.rows).map((entry) => [entry.id, entry.value]))
    for (const id of hostRows) expect(values.get(id), id).toBe(NOT_LOADED)
    expect(category(categories, 'permissions').refusals).toEqual([])
    // The three facts the browser owns do not depend on the host answering.
    expect(values.get('appearance-theme')).toBe('深色')
    expect(values.get('provider-api-key')).toBe('host 侧未配置')
  })

  it('reports the serving process rather than the page', () => {
    const host = hostInfo({ host_version: '1.2.0', protocol_version: 11, bind_addr: '127.0.0.1:9999', workspace_root: '/srv/app' })
    expect(row('general', 'general-version', { host }).value).toBe('1.2.0')
    expect(row('general', 'general-protocol', { host }).value).toBe('11')
    expect(row('general', 'general-bind', { host }).value).toBe('127.0.0.1:9999')
    expect(row('permissions', 'permissions-workspace', { host }).value).toBe('/srv/app')
    expect(row('general', 'general-version', { host }).warn).toBeFalsy()
  })

  it('warns on the configurations that are actually exposed or lossy', () => {
    const exposed = input({ host: hostInfo({ bind_addr: '0.0.0.0:8787', token_required: false, state_backend: 'memory', preview_proxy: 'any_origin', preview_ports: [3000], safe_web_mode: true }) })
    expect(row('general', 'general-bind', exposed).warn).toBe(true)
    expect(row('general', 'general-state', exposed).warn).toBe(true)
    expect(row('security', 'security-token', exposed).warn).toBe(true)
    expect(row('security', 'security-preview', exposed).warn).toBe(true)
    expect(row('permissions', 'permissions-safe-mode', exposed).warn).toBe(true)
    expect(row('security', 'security-preview', exposed).value).toContain('3000')
    expect(row('general', 'general-state', exposed).locked).toContain('不会在重启后保留')

    const loopback = input({ host: hostInfo() })
    expect(row('general', 'general-bind', loopback).warn).toBe(false)
    expect(row('security', 'security-token', loopback).warn).toBe(false)
    expect(row('security', 'security-preview', loopback).warn).toBe(false)
    expect(row('permissions', 'permissions-safe-mode', loopback).warn).toBe(false)
  })

  it('names each state backend and preview mode the host can report', () => {
    expect(row('general', 'general-state', { host: hostInfo({ state_backend: 'memory' }) }).value).toContain('仅内存')
    expect(row('general', 'general-state', { host: hostInfo({ state_backend: 'sqlite' }) }).value).toContain('SQLite')
    expect(row('security', 'security-preview', { host: hostInfo({ preview_proxy: 'named_only', preview_ports: [5173, 8080] }) }).value).toContain('5173')
    expect(row('security', 'security-preview', { host: hostInfo({ preview_proxy: 'named_only' }) }).value).toContain('无端口')
    expect(row('updates', 'updates-mode', { host: hostInfo({ update_mode: 'self_update' }) }).value).toContain('替换二进制')
    expect(row('updates', 'updates-mode', { host: hostInfo({ update_mode: 'external' }) }).value).toContain('启动它的方式')
  })

  it('words the withheld-capability list for the mode the host is actually in', () => {
    expect(refusalSummary(null)).toContain('要等 host 回报')
    expect(refusalSummary(hostInfo({ safe_web_mode: true }))).toContain('已开启')
    expect(refusalSummary(hostInfo({ safe_web_mode: true }))).toContain('2 类操作')
    expect(refusalSummary(hostInfo({ safe_web_mode: false }))).toContain('未开启')
    expect(refusalSummary(hostInfo({ safe_mode_refusals: [] }))).toContain('0 类操作')
    expect(MAIN_SOURCE).toContain('refusalSummary(session.hostInfo ?? null)')
  })

  it('lists exactly the capabilities the host says it withholds', () => {
    const host = hostInfo({ safe_web_mode: true, safe_mode_refusals: [{ message: 'propose_terminal', capability: '在工作区执行命令' }] })
    const permissions = category(buildSettingsCategories(input({ host })), 'permissions')
    expect(permissions.refusals).toEqual([{ message: 'propose_terminal', capability: '在工作区执行命令' }])
  })

  it('renders one shortcut row per binding, spelled for the reader platform', () => {
    const onWindows = category(buildSettingsCategories(input({ platform: 'Win32' })), 'shortcuts')
    expect(onWindows.rows.map((entry) => entry.value)).toEqual(SHORTCUTS.map((shortcut) => formatShortcut(shortcut.keys, 'Win32')))
    expect(onWindows.rows.every((entry) => entry.locked)).toBe(true)
    const onMac = category(buildSettingsCategories(input({ platform: 'MacIntel' })), 'shortcuts')
    expect(onMac.rows[0]?.value).toBe(formatShortcut(SHORTCUTS[0]!.keys, 'MacIntel'))
    expect(onMac.rows[0]?.value).toContain('Cmd')
  })

  it('keeps row ids unique because they become test ids in the browser', () => {
    const rows = buildSettingsCategories(input()).flatMap((entry) => entry.rows)
    expect(new Set(rows.map((entry) => entry.id)).size).toBe(rows.length)
    expect(MAIN_SOURCE).toContain('data-testid={row.id}')
    expect(MAIN_SOURCE).toContain('data-testid={`settings-category-${category.id}`}')
  })

  it('claims only the controls the shell actually renders', () => {
    const claimed = new Set(buildSettingsCategories(input()).map((entry) => entry.control))
    expect([...claimed].sort()).toEqual(['appearance', 'model', 'none', 'provider'])
    expect(MAIN_SOURCE).toContain("category.control === 'appearance'")
    expect(MAIN_SOURCE).toContain("category.control === 'model'")
    expect(MAIN_SOURCE).toContain("category.control === 'provider'")
    // Nothing may offer a control the panel does not render.
    for (const entry of buildSettingsCategories(input())) {
      if (entry.control === 'none') continue
      expect(MAIN_SOURCE).toContain(`category.control === '${entry.control}'`)
    }
    // The theme control renders one button per entry of THEME_ORDER, and the row in
    // the panel is the same value the button sets.
    expect(MAIN_SOURCE).toContain('THEME_ORDER.map((theme) => (')
    expect(MAIN_SOURCE).toContain('data-testid={`theme-${theme}`}')
    expect(MAIN_SOURCE).toContain('aria-pressed={layout.theme === theme}')
    expect(MAIN_SOURCE).toContain('onClick={() => setLayout((current) => ({ ...current, theme }))}')
    expect(MAIN_SOURCE).toContain('aria-label="Provider Model"')
    expect(MAIN_SOURCE).toContain('aria-label="Provider Base URL"')
  })

  it('locks every row the browser cannot change and explains why', () => {
    const locked = buildSettingsCategories(input({ host: hostInfo() }))
      .filter((entry) => entry.control === 'none')
      .flatMap((entry) => entry.rows)
    expect(locked.length).toBeGreaterThan(15)
    for (const entry of locked) {
      expect(entry.locked, entry.id).toBeTruthy()
      expect(entry.locked!.length, entry.id).toBeGreaterThan(4)
    }
  })

  it('shows what the browser owns about the model and provider', () => {
    expect(row('model', 'model-current', { model: '  ' }).value).toContain('未设置')
    expect(row('model', 'model-current', { model: 'gpt-4o' }).value).toBe('gpt-4o')
    expect(row('provider', 'provider-base-url', { baseUrl: 'https://api.example.test/v1' }).value).toBe('https://api.example.test/v1')
    expect(row('provider', 'provider-api-key', { hasApiKey: true }).value).toBe('host 侧已配置')
    expect(row('provider', 'provider-api-key', { hasApiKey: true }).warn).toBe(false)
    expect(row('provider', 'provider-api-key', { hasApiKey: false }).warn).toBe(true)
    // The key is never echoed, so nothing here can render its contents.
    expect(row('provider', 'provider-api-key', { hasApiKey: true }).value).not.toContain('sk-')
  })
})

describe('theme control', () => {
  it('walks the three themes and comes back round', () => {
    expect(THEME_ORDER).toEqual(['dark', 'light', 'system'])
    let theme: SettingsInput['theme'] = 'dark'
    const visited: SettingsInput['theme'][] = [theme]
    for (let step = 0; step < 3; step += 1) {
      theme = nextTheme(theme)
      visited.push(theme)
    }
    expect(visited).toEqual(['dark', 'light', 'system', 'dark'])
  })

  it('labels every theme the control can set', () => {
    expect(THEME_ORDER.map(themeLabel)).toEqual(['深色', '浅色', '跟随系统'])
    expect(row('appearance', 'appearance-theme', { theme: 'system' }).value).toBe('跟随系统')
    expect(row('appearance', 'appearance-theme-scope', { theme: 'system' }).value).toContain('本地存储')
    expect(row('appearance', 'appearance-theme-scope', { theme: 'system' }).locked).toContain('host 不记录')
  })
})
