import { describe, expect, it } from 'vitest'
import { dropReasonFor, newMessageId, webSocketUrl, type IdSource } from './transport'

describe('web socket URL', () => {
  it('uses a secure WebSocket for an HTTPS page and preserves host port and base path', () => {
    expect(webSocketUrl({ protocol: 'https:', hostname: 'chaos.example', port: '9443', pathname: '/app/' }))
      .toBe('wss://chaos.example:9443/app/ws')
  })

  it('uses a plain WebSocket for a local HTTP page and supplies the default service port', () => {
    expect(webSocketUrl({ protocol: 'http:', hostname: 'localhost', port: '', pathname: '/' }))
      .toBe('ws://localhost:8787/ws')
  })

  it('formats IPv6 hosts and supports an explicit API prefix', () => {
    expect(webSocketUrl({ protocol: 'https:', hostname: '[::1]', port: '', pathname: '/chaos' }, '8788'))
      .toBe('wss://[::1]:8788/chaos/ws')
  })
})

describe('message ids', () => {
  /** The wire shape `crypto.randomUUID()` used to be the only producer of. */
  const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/

  it('prefers crypto.randomUUID when the page has it', () => {
    const source: IdSource = { randomUUID: () => '11111111-2222-4333-8444-555555555555', getRandomValues: () => new Uint8Array(16) }
    expect(newMessageId(source)).toBe('11111111-2222-4333-8444-555555555555')
  })

  /**
   * The shape a plain-HTTP page over a routable address actually has: the browser is present,
   * `randomUUID` is not. Before this existed the page threw on the first message it tried to
   * send, and every panel kept showing the status of a request that had never left.
   */
  it('writes the version and variant bits into whatever getRandomValues returned', () => {
    const counted: IdSource = {
      getRandomValues: <T extends Uint8Array>(values: T) => {
        for (let index = 0; index < values.length; index += 1) values[index] = index + 1
        return values
      },
    }
    // 0x40 lands in byte 6 and 0x80 in byte 8, so the two positions the fill happened to touch
    // are the two the formatter owns; a missing bit patch shows up as a changed digit here.
    expect(newMessageId(counted)).toBe('01020304-0506-4708-890a-0b0c0d0e0f10')
  })

  it('mints distinct uuids from getRandomValues where randomUUID does not exist', () => {
    // The real shape of a plain-HTTP page over a routable address: the browser is present,
    // `randomUUID` is not. Before this the page threw on the first message it tried to send,
    // and every panel kept showing the status of a request that had never left.
    const insecure: IdSource = { getRandomValues: (values) => globalThis.crypto.getRandomValues(values) }
    const ids = new Set(Array.from({ length: 500 }, () => newMessageId(insecure)))
    expect(ids.size).toBe(500)
    for (const id of ids) expect(id).toMatch(uuid)
  })

  it('mints distinct ids from an origin that has neither method', () => {
    const ids = new Set(Array.from({ length: 200 }, () => newMessageId({})))
    expect(ids.size).toBe(200)
    for (const id of ids) expect(id).toMatch(uuid)
  })

  it('refuses a stubbed WebCrypto that fills zeros, because a constant id collides by design', () => {
    const zeros: IdSource = { getRandomValues: <T extends Uint8Array>(values: T) => values }
    const first = newMessageId(zeros)
    expect(first).toMatch(uuid)
    expect(newMessageId(zeros)).not.toBe(first)
  })

  it('treats a randomUUID that throws like one that was never there', () => {
    const hostile: IdSource = {
      randomUUID: () => {
        throw new Error('blocked by the embedder')
      },
      getRandomValues: <T extends Uint8Array>(values: T) => {
        values.fill(7)
        return values
      },
    }
    expect(newMessageId(hostile)).toMatch(uuid)
  })

  it('reaches for the real browser crypto when nothing is injected', () => {
    // Node and every supported browser expose a global crypto with at least getRandomValues,
    // so the default argument has to produce a usable id without a caller naming an origin.
    expect(newMessageId()).toMatch(uuid)
  })
})

describe('why a message could not be sent', () => {
  // The spec fixes these numbers: 0 connecting, 1 open, 2 closing, 3 closed.
  it('lets the message through only when the socket is open', () => {
    expect(dropReasonFor(1)).toBeNull()
  })

  it('names every state that would otherwise drop the message in silence', () => {
    expect(dropReasonFor(undefined)).toBe('尚未连接，消息未发送')
    expect(dropReasonFor(0)).toBe('正在连接，消息未发送')
    expect(dropReasonFor(2)).toBe('连接已断开，消息未发送')
    expect(dropReasonFor(3)).toBe('连接已断开，消息未发送')
  })
})
