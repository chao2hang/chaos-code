import { describe, expect, it } from 'vitest'
import { webSocketUrl } from './transport'

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
