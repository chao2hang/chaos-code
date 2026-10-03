// The settings panel's content, derived rather than declared.
//
// Nine categories, one per row of the M3.1 checklist, but the panel does not
// invent a control per row. Most of those rows describe a decision the *host*
// made before it could bind a socket -- where state is kept, whether a token is
// required, which capabilities Safe Web Mode withholds -- and a settings control
// that looked editable while nothing read it would be worse than none. So each
// category says which control it genuinely offers (`control`) and the rest are
// facts that came from `host_info`, each with the reason the browser cannot
// change it.
//
// `main.tsx` renders exactly this, and `settings.test.ts` asserts the mapping,
// including that a category never claims a control the app cannot perform.

import type { HostInfo, SafeModeRefusal } from './generated/protocol'
import { SHORTCUTS, formatShortcut } from './shortcuts'

/** One line of the panel: a label, the value, and why if it is not editable. */
export type SettingsRow = {
  /** Stable id, mirrored into `data-testid` so the browser tests can assert it. */
  id: string
  label: string
  value: string
  /** Why the browser cannot change this. Absent means an editable control follows. */
  locked?: string
  /** Rendered as a warning when the fact describes something that is unavailable. */
  warn?: boolean
}

export type SettingsCategory = {
  id: string
  title: string
  /**
   * The one control this category offers. `none` means every row is a fact, and
   * the panel renders no input rather than an input that goes nowhere.
   */
  control: 'appearance' | 'model' | 'provider' | 'none'
  rows: SettingsRow[]
  /** Capabilities Safe Web Mode withholds; only the permissions category sets this. */
  refusals?: SafeModeRefusal[]
}

export type SettingsInput = {
  /** null until the host answers `get_host_info`; the panel says so rather than guessing. */
  host: HostInfo | null
  /** The browser's own theme choice, which is the one setting the browser owns. */
  theme: 'dark' | 'light' | 'system'
  model: string | null
  baseUrl: string | null
  hasApiKey: boolean
  platform?: string
}

const NOT_LOADED = 'host 尚未回报'

function stateBackendLabel(backend: HostInfo['state_backend']): string {
  switch (backend) {
    case 'memory':
      return '仅内存（重启后丢掉所有会话）'
    case 'json_file':
      return 'JSON 快照文件（启动时由 CHAOS_WEB_STATE 指定）'
    case 'sqlite':
      return 'SQLite（启动时由 CHAOS_WEB_SQLITE 指定）'
  }
}

function previewLabel(host: HostInfo): string {
  switch (host.preview_proxy) {
    case 'disabled':
      return '未开启（没有端口被转发）'
    case 'named_only':
      return `仅公开域名可达：${host.preview_ports.join('、') || '无端口'}`
    case 'any_origin':
      return `任意来源可达（含 loopback）：${host.preview_ports.join('、') || '无端口'}`
  }
}

function updateLabel(mode: HostInfo['update_mode']): string {
  return mode === 'self_update' ? '由本进程自行替换二进制并重启' : '由启动它的方式决定（cargo / npm / 安装包）'
}

const STARTUP_ONLY = '由启动这个 host 的环境变量决定，浏览器改不了；改完要重启 host'

export function buildSettingsCategories(input: SettingsInput): SettingsCategory[] {
  const { host } = input
  const platform = input.platform ?? ''
  const startup = { locked: STARTUP_ONLY }

  const general: SettingsCategory = {
    id: 'general',
    title: '通用',
    control: 'none',
    rows: [
      { id: 'general-version', label: '服务进程版本', value: host ? host.host_version : NOT_LOADED, ...startup },
      { id: 'general-protocol', label: '协议版本', value: host ? String(host.protocol_version) : NOT_LOADED, ...startup },
      {
        id: 'general-bind',
        label: '监听地址',
        value: host ? host.bind_addr : NOT_LOADED,
        ...startup,
        warn: Boolean(host && !host.bind_addr.startsWith('127.')),
      },
      {
        id: 'general-state',
        label: '状态持久化',
        value: host ? stateBackendLabel(host.state_backend) : NOT_LOADED,
        locked: host?.state_backend === 'memory' ? `${STARTUP_ONLY}；当前设置不会在重启后保留` : STARTUP_ONLY,
        warn: host ? host.state_backend === 'memory' : false,
      },
    ],
  }

  const appearance: SettingsCategory = {
    id: 'appearance',
    title: '外观',
    control: 'appearance',
    rows: [
      { id: 'appearance-theme', label: '主题', value: themeLabel(input.theme) },
      {
        id: 'appearance-theme-scope',
        label: '保存位置',
        value: '本浏览器的本地存储，不上传给 host',
        locked: 'host 不记录外观偏好，换台机器要重新选',
      },
    ],
  }

  const model: SettingsCategory = {
    id: 'model',
    title: '模型',
    control: 'model',
    rows: [
      {
        id: 'model-current',
        label: '当前模型',
        value: input.model?.trim() ? input.model : '未设置（由 host 侧的启动配置决定）',
        warn: !input.model?.trim(),
      },
    ],
  }

  const provider: SettingsCategory = {
    id: 'provider',
    title: 'Provider',
    control: 'provider',
    rows: [
      {
        id: 'provider-base-url',
        label: 'Base URL',
        value: input.baseUrl?.trim() ? input.baseUrl : '未设置',
        warn: !input.baseUrl?.trim(),
      },
      {
        id: 'provider-api-key',
        label: 'API Key',
        value: input.hasApiKey ? 'host 侧已配置' : 'host 侧未配置',
        locked: '凭据不由浏览器提交，也不经由这里显示；存入 OS keyring 的方案仍待选型',
        warn: !input.hasApiKey,
      },
    ],
  }

  const permissions: SettingsCategory = {
    id: 'permissions',
    title: '权限',
    control: 'none',
    rows: [
      {
        id: 'permissions-safe-mode',
        label: 'Safe Web Mode',
        value: host ? (host.safe_web_mode ? '已开启' : '未开启') : NOT_LOADED,
        locked: STARTUP_ONLY,
        warn: Boolean(host?.safe_web_mode),
      },
      {
        id: 'permissions-workspace',
        label: '工作区',
        value: host ? (host.workspace_root ?? '未绑定任何工作区') : NOT_LOADED,
        locked: host && !host.workspace_root ? '启动时没给 CHAOS_WORKSPACE_ROOT，因此读写、搜索与终端都无处可用' : STARTUP_ONLY,
        warn: Boolean(host && !host.workspace_root),
      },
      {
        id: 'permissions-approval',
        label: '写操作前确认',
        value: '文件写入、终端命令与 Git 变更都要逐次确认',
        locked: '确认次数由引擎按操作决定，这里没有可放宽的开关',
      },
    ],
    refusals: host ? host.safe_mode_refusals : [],
  }

  const security: SettingsCategory = {
    id: 'security',
    title: '安全',
    control: 'none',
    rows: [
      {
        id: 'security-token',
        label: '访问令牌',
        value: host ? (host.token_required ? '必需（缺令牌的请求被拒）' : '未设置（任何能连上端口的进程都可达）') : NOT_LOADED,
        locked: STARTUP_ONLY,
        warn: host ? !host.token_required : false,
      },
      {
        id: 'security-origin',
        label: '公开域名',
        value: host ? (host.public_origin ?? '未声明（只接受 loopback 的 Host/Origin）') : NOT_LOADED,
        ...startup,
      },
      {
        id: 'security-preview',
        label: '预览代理',
        value: host ? previewLabel(host) : NOT_LOADED,
        locked: '转发的端口名单由 CHAOS_WEB_PREVIEW_PORTS 决定；开启「任意来源」还需要 CHAOS_WEB_PREVIEW_ALLOW_PUBLIC',
        warn: host?.preview_proxy === 'any_origin',
      },
    ],
  }

  const shortcuts: SettingsCategory = {
    id: 'shortcuts',
    title: '快捷键',
    control: 'none',
    rows: SHORTCUTS.map((shortcut) => ({
      id: `shortcut-${shortcut.id.replace(':', '-')}`,
      label: shortcut.label,
      value: formatShortcut(shortcut.keys, platform),
      locked: '快捷键表就是处理器本身（src/shortcuts.ts），暂不支持自定义',
    })),
  }

  const remote: SettingsCategory = {
    id: 'remote',
    title: '远程',
    control: 'none',
    rows: [
      {
        id: 'remote-workspace',
        label: '这个 host 读写的工作区',
        value: host ? (host.workspace_root ? '本机路径' : '无') : NOT_LOADED,
        locked: '本 host 只提供本机工作区；远程工作区由 chaos-remote CLI 单独配置，不经过浏览器',
      },
      {
        id: 'remote-detached',
        label: '断线后继续运行',
        value: '不承诺：没有附加的会话不会替你继续跑',
        locked: 'ADR-004 只允许本机 Agent 驱动远程工作区，detached Agent 明确不在范围内',
      },
    ],
  }

  const updates: SettingsCategory = {
    id: 'updates',
    title: '更新',
    control: 'none',
    rows: [
      {
        id: 'updates-mode',
        label: '更新方式',
        value: host ? updateLabel(host.update_mode) : NOT_LOADED,
        locked: 'Web host 不替换自身二进制；换成哪个版本由启动它的方式决定',
      },
      {
        id: 'updates-frontend',
        label: '页面资源版本',
        value: '随浏览器加载的构建，与上面的进程版本是两件事',
        locked: '要两者一致就重新构建并重启 host',
        warn: false,
      },
    ],
  }

  return [general, appearance, model, provider, permissions, security, shortcuts, remote, updates]
}

/**
 * The line above the withheld-capability list. Safe Web Mode being off does not
 * make the inventory a lie, so it is worded as what would happen rather than
 * dropped.
 */
export function refusalSummary(host: HostInfo | null): string {
  if (!host) return '要等 host 回报，才知道这次启动的 Safe Web Mode 具体拦下哪些操作。'
  const count = host.safe_mode_refusals.length
  return host.safe_web_mode
    ? `Safe Web Mode 已开启，下面 ${count} 类操作会被直接拒绝，浏览器上点亮的按钮也一样。`
    : `Safe Web Mode 未开启；若开启，下面 ${count} 类操作会被直接拒绝。`
}

export function themeLabel(theme: SettingsInput['theme']): string {
  switch (theme) {
    case 'dark':
      return '深色'
    case 'light':
      return '浅色'
    case 'system':
      return '跟随系统'
  }
}

/** The three themes in the order the cycle button walks them. */
export const THEME_ORDER = ['dark', 'light', 'system'] as const

export function nextTheme(theme: SettingsInput['theme']): SettingsInput['theme'] {
  const index = THEME_ORDER.indexOf(theme)
  return THEME_ORDER[(index + 1) % THEME_ORDER.length]
}
