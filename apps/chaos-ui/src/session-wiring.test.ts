import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

// The message handler applies every host reply to `sessionStateRef` and writes
// the result, so a functional `setSession` would leave that ref one render behind
// and a fast reply would be applied to a stale state. An attachment rejection hit
// exactly this: the panel forgot the upload and showed nothing.
const MAIN_SOURCE = readFileSync(new URL('main.tsx', import.meta.url), 'utf8')

describe('session state plumbing', () => {
  it('writes session state through the ref-synchronising helper only', () => {
    expect(MAIN_SOURCE).toContain('sessionStateRef.current = next')
    expect(MAIN_SOURCE).not.toMatch(/setSession\(\(/)
    expect(MAIN_SOURCE.match(/updateSession\(\(/g)?.length).toBeGreaterThan(10)
  })
})
