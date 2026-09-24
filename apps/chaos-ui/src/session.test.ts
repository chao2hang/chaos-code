import { describe, expect, it } from 'vitest'
import { applyServerMessage, initialSessionState } from './session'

describe('session event projection', () => {
  it('projects streaming text and completion through shipped reducer', () => {
    let state = applyServerMessage(initialSessionState, { type: 'session_created', session_id: 's1' })
    state = applyServerMessage(state, { type: 'text_delta', text: '真实' })
    state = applyServerMessage(state, { type: 'text_delta', text: '响应' })
    state = applyServerMessage({ ...state, busy: true }, { type: 'completed' })
    expect(state.sessionId).toBe('s1')
    expect(state.messages).toEqual([{ role: 'assistant', text: '真实响应' }])
    expect(state.busy).toBe(false)
  })

  it('requires explicit approval and question events before rendering actions', () => {
    const approval = applyServerMessage(initialSessionState, { type: 'tool_approval_requested', request_id: 'r1', tool: 'demo', summary: 'write' })
    expect(approval.approval).toEqual({ requestId: 'r1', tool: 'demo', summary: 'write' })
    const resolved = applyServerMessage(approval, { type: 'approval_resolved' })
    expect(resolved.approval).toBeUndefined()
    const question = applyServerMessage(initialSessionState, { type: 'question_requested', question_id: 'q1', prompt: 'continue?' })
    expect(question.question?.questionId).toBe('q1')
    expect(applyServerMessage(question, { type: 'question_resolved' }).question).toBeUndefined()
  })

  it('uses snapshot as the reconnect source of truth', () => {
    const state = applyServerMessage({ ...initialSessionState, messages: [{ role: 'user', text: 'stale' }] }, { type: 'session_snapshot', messages: [{ role: 'user', text: 'restored' }] })
    expect(state.messages).toEqual([{ role: 'user', text: 'restored' }])
  })
})
