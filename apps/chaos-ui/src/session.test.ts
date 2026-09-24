import { describe, expect, it } from 'vitest'
import { applyServerMessage, initialSessionState, workspaceReconnectMessage } from './session'

describe('session event projection', () => {
  it('projects streaming text and completion through shipped reducer', () => {
    let state = applyServerMessage(initialSessionState, { type: 'session_created', session_id: 's1', workspace_id: 'w1' })
    state = applyServerMessage(state, { type: 'text_delta', session_id: 's1', sequence: 1, text: '真实' })
    state = applyServerMessage(state, { type: 'text_delta', session_id: 's1', sequence: 2, text: '响应' })
    state = applyServerMessage({ ...state, busy: true }, { type: 'completed', session_id: 's1', sequence: 3 })
    expect(state.sessionId).toBe('s1')
    expect(state.messages).toEqual([{ role: 'assistant', text: '真实响应' }])
    expect(state.busy).toBe(false)
  })

  it('requires explicit approval and question events before rendering actions', () => {
    const approval = applyServerMessage(initialSessionState, { type: 'tool_approval_requested', session_id: 's1', request_id: 'r1', tool: 'demo', summary: 'write', sequence: 1 })
    expect(approval.approval).toEqual({ requestId: 'r1', tool: 'demo', summary: 'write' })
    const resolved = applyServerMessage(approval, { type: 'approval_resolved', session_id: 's1', request_id: 'r1', approved: true, sequence: 2 })
    expect(resolved.approval).toBeUndefined()
    const question = applyServerMessage(initialSessionState, { type: 'question_requested', session_id: 's1', question_id: 'q1', prompt: 'continue?', sequence: 1 })
    expect(question.question?.questionId).toBe('q1')
    expect(applyServerMessage(question, { type: 'question_resolved', session_id: 's1', question_id: 'q1', answer: 'yes', sequence: 2 }).question).toBeUndefined()
  })

  it('projects workspace list, switch, and archive events', () => {
    let state = applyServerMessage(initialSessionState, { type: 'workspaces', active_workspace_id: 'w1', workspaces: [{ id: 'w1', name: '默认工作区', archived: false, last_used_sequence: 1, last_session_id: null }, { id: 'w2', name: '项目', archived: false, last_used_sequence: 2, last_session_id: null }] })
    expect(state.workspaces).toHaveLength(2)
    state = applyServerMessage(state, { type: 'workspace_switched', workspace_id: 'w2' })
    expect(state.activeWorkspaceId).toBe('w2')
    state = applyServerMessage(state, { type: 'workspace_archived', workspace_id: 'w1' })
    expect(state.workspaces.find((workspace) => workspace.id === 'w1')?.archived).toBe(true)
  })

  it('isolates transcript and session selection when switching workspaces', () => {
    let state = applyServerMessage(initialSessionState, { type: 'session_created', session_id: 'session-a', workspace_id: 'workspace-a' })
    state = applyServerMessage(state, { type: 'text_delta', session_id: 'session-a', text: 'A transcript', sequence: 1 })
    state = applyServerMessage(state, { type: 'session_created', session_id: 'session-b', workspace_id: 'workspace-b' })
    state = applyServerMessage(state, { type: 'text_delta', session_id: 'session-b', text: 'B transcript', sequence: 1 })
    state = applyServerMessage(state, { type: 'workspaces', active_workspace_id: 'workspace-b', workspaces: [
      { id: 'workspace-a', name: 'A', archived: false, last_used_sequence: 1, last_session_id: 'session-a' },
      { id: 'workspace-b', name: 'B', archived: false, last_used_sequence: 2, last_session_id: 'session-b' },
    ] })
    state = applyServerMessage(state, { type: 'workspace_switched', workspace_id: 'workspace-a' })
    expect(state.activeWorkspaceId).toBe('workspace-a')
    expect(state.sessionId).toBe('session-a')
    expect(state.messages).toEqual([])
    state = applyServerMessage(state, { type: 'session_snapshot', session_id: 'session-a', workspace_id: 'workspace-a', sequence: 1, pending_approval: null, pending_question: null, messages: [{ role: 'assistant', text: 'A transcript' }] })
    expect(state.messages).toEqual([{ role: 'assistant', text: 'A transcript' }])
    expect(state.sessionId).toBe('session-a')
    expect(state.workspaceSessions).toEqual({ 'workspace-a': 'session-a', 'workspace-b': 'session-b' })
    state = applyServerMessage(state, { type: 'workspaces', active_workspace_id: 'workspace-b', workspaces: [
      { id: 'workspace-a', name: 'A', archived: false, last_used_sequence: 1, last_session_id: 'session-a' },
      { id: 'workspace-b', name: 'B', archived: false, last_used_sequence: 2, last_session_id: 'session-b' },
    ] })
    expect(state.activeWorkspaceId).toBe('workspace-b')
    expect(state.sessionId).toBe('session-b')
    expect(state.messages).toEqual([])
  })

  it('reconnects with the selected workspace session rather than a stale previous session', () => {
    const selected = {
      ...initialSessionState,
      activeWorkspaceId: 'workspace-a',
      sessionId: 'session-a',
      workspaceSessions: { 'workspace-a': 'session-a', 'workspace-b': 'session-b' },
    }
    expect(workspaceReconnectMessage(selected)).toMatchObject({ type: 'resume', session_id: 'session-a', workspace_id: 'workspace-a' })
    const newWorkspace = { ...selected, activeWorkspaceId: 'workspace-new', sessionId: undefined }
    expect(workspaceReconnectMessage(newWorkspace)).toMatchObject({ type: 'create_session', workspace_id: 'workspace-new' })
  })

  it('uses snapshot as the reconnect source of truth', () => {
    const state = applyServerMessage({ ...initialSessionState, messages: [{ role: 'user', text: 'stale' }] }, { type: 'session_snapshot', session_id: 's1', workspace_id: null, sequence: 1, pending_approval: { request_id: 'r1', tool: 'demo.tool', summary: 'confirm', confirmations_required: 1, confirmations: 0 }, pending_question: null, messages: [{ role: 'user', text: 'restored' }] })
    expect(state.messages).toEqual([{ role: 'user', text: 'restored' }])
    expect(state.approval).toEqual({ requestId: 'r1', tool: 'demo.tool', summary: 'confirm' })
  })
})
