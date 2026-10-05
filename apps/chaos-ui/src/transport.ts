export type WebSocketLocation = Pick<Location, 'protocol' | 'hostname' | 'port' | 'pathname'>

export function webSocketUrl(location: WebSocketLocation, defaultPort = '8787', basePathOverride?: string): string {
  const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:'
  const host = location.hostname || '127.0.0.1'
  const port = location.port ? `:${location.port}` : `:${defaultPort}`
  const basePath = (basePathOverride ?? location.pathname).replace(/\/$/, '')
  const normalizedPath = basePath === '/' ? '' : basePath
  return `${protocol}//${host}${port}${normalizedPath}/ws`
}

/** The `readyState` values the WebSocket specification fixes; spelled out so the check works
 * in any host, including the ones that run these modules without a `WebSocket` global. */
const WS_CONNECTING = 0
const WS_OPEN = 1

/**
 * Why a message cannot leave the page right now, or `null` when the socket can take it.
 *
 * Every call site used to test `readyState === WebSocket.OPEN` and return, which made a dropped
 * message indistinguishable from a delivered one: the badge kept reading as if the connection
 * were fine while the panel waited for a reply to something the host never received. Naming the
 * reason here keeps the decision in one place and lets the caller say it out loud.
 */
export function dropReasonFor(readyState: number | undefined): string | null {
  if (readyState === WS_OPEN) return null
  if (readyState === undefined) return '尚未连接，消息未发送'
  if (readyState === WS_CONNECTING) return '正在连接，消息未发送'
  return '连接已断开，消息未发送'
}

/** Where a message id can come from, in decreasing order of preference. The two methods are
 * probed separately because a browser may have exactly one of them. */
export type IdSource = {
  randomUUID?: () => string
  getRandomValues?: (values: Uint8Array<ArrayBuffer>) => unknown
}

let sequence = 0

/**
 * A `client_msg_id` this page can produce in *any* origin.
 *
 * Calling `crypto.randomUUID()` directly is the obvious spelling and the wrong one: that method
 * exists only in a secure context, so a plain-HTTP page opened over a routable address -- which is
 * where a Web host actually gets opened, since the backend itself binds loopback and the front-end
 * server is what faces the network -- sees `undefined` there. A client that mints one id per
 * message then throws on every send while the connection still reads "connected", which is a
 * silent failure with a spinner for a UI. `crypto.getRandomValues` is available in insecure
 * contexts too, so it covers every real browser; the sequenced id behind it is for hosts that
 * stub a bare `crypto` (test runners, tooling) and is unique within the page, not across
 * processes.
 */
export function newMessageId(source: IdSource | undefined = globalThis.crypto): string {
  const direct = source?.randomUUID
  if (typeof direct === 'function') {
    try {
      return direct.call(source)
    } catch {
      // An embedder that defines the method and then refuses it lands on the same path as one
      // that never had it; neither is a reason to lose the message.
    }
  }
  const bytes = randomSixteen(source)
  return bytes ? formatUuid(bytes) : sequencedUuid()
}

function randomSixteen(source?: IdSource): Uint8Array<ArrayBuffer> | undefined {
  const fill = source?.getRandomValues
  if (typeof fill !== 'function') return undefined
  const values = new Uint8Array(16)
  try {
    fill.call(source, values)
  } catch {
    return undefined
  }
  // A stubbed WebCrypto that hands back zeros would otherwise mint one id forever, and duplicate
  // ids are how a host starts answering a later message with an earlier turn's reply.
  return values.some((byte) => byte !== 0) ? values : undefined
}

function sequencedUuid(): string {
  sequence = (sequence + 1) % 0x10000
  const stamp = Date.now()
  const values = new Uint8Array(16)
  for (let index = 0; index < values.length; index += 1) {
    values[index] = ((stamp >>> ((index % 6) * 8)) + sequence * 131 + index * 17) & 0xff
  }
  return formatUuid(values)
}

function formatUuid(values: Uint8Array): string {
  values[6] = (values[6] & 0x0f) | 0x40
  values[8] = (values[8] & 0x3f) | 0x80
  const hex = Array.from(values, (byte) => byte.toString(16).padStart(2, '0')).join('')
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}
