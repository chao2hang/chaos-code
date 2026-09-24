import { describe, expect, it } from 'vitest'
import { applyServerMessage, initialSessionState } from './session'
import { selectWorkspaceSession, workspaceChanged } from './workspace-ui'

const workspaces = [
  { id: 'a', name: 'A', archived: false, last_used_sequence: 1, last_session_id: 'session-a' },
  { id: 'b', name: 'B', archived: false, last_used_sequence: 2, last_session_id: 'session-b' },
]

describe('workspace session isolation', () => {
  it('selects each workspace session and clears the previous transcript before the snapshot arrives', () => {
    const state = {
      ...initialSessionState,
      workspaces,
      activeWorkspaceId: 'b',
      workspaceSessions: { a: 'session-a', b: 'session-b' },
      sessionId: 'session-b',
      messages: [{ role: 'assistant', text: 'workspace B private transcript' }],
      approval: { requestId: 'approval-b', tool: 'tool', summary: 'B only' },
      question: { questionId: 'question-b', prompt: 'B only' },
      busy: true,
    }
    const selected = selectWorkspaceSession(state, 'a')
    expect(selected.activeWorkspaceId).toBe('a')
    expect(selected.sessionId).toBe('session-a')
    expect(selected.messages).toEqual([])
    expect(selected.approval).toBeUndefined()
    expect(selected.question).toBeUndefined()
    expect(selected.busy).toBe(false)
  })

  it('clears an archived current workspace and selects the remaining active workspace', () => {
    const source = {
      ...initialSessionState,
      workspaces,
      activeWorkspaceId: 'a',
      workspaceSessions: { a: 'session-a', b: 'session-b' },
      sessionId: 'session-a',
      messages: [{ role: 'assistant', text: 'A private transcript' }],
      busy: true,
    }
    const archived = applyServerMessage(source, { type: 'workspace_archived', workspace_id: 'a' })
    expect(archived.activeWorkspaceId).toBe('b')
    expect(archived.sessionId).toBe('session-b')
    expect(archived.messages).toEqual([])
    expect(archived.busy).toBe(false)
    expect(archived.workspaces.find((workspace) => workspace.id === 'a')?.archived).toBe(true)
  })

  it('uses a newly-created workspace session, then switches away without stale transcript', () => {
    const created = applyServerMessage(initialSessionState, { type: 'session_created', session_id: 'session-new', workspace_id: 'workspace-new' })
    expect(created.activeWorkspaceId).toBe('workspace-new')
    expect(created.sessionId).toBe('session-new')
    const listed = applyServerMessage(created, { type: 'workspaces', active_workspace_id: 'workspace-new', workspaces: [
      { id: 'workspace-new', name: 'New', archived: false, last_used_sequence: 1, last_session_id: 'session-new' },
    ] })
    expect(listed.workspaceSessions).toEqual({ 'workspace-new': 'session-new' })
    const switched = applyServerMessage({ ...listed, messages: [{ role: 'user', text: 'New workspace' }] }, { type: 'workspace_switched', workspace_id: 'workspace-new' })
    expect(switched.sessionId).toBe('session-new')
    expect(switched.messages).toEqual([])
  })

  it('clears active conversation state if the only workspace is archived', () => {
    const state = { ...initialSessionState, workspaces: [workspaces[0]], activeWorkspaceId: 'a', sessionId: 'session-a', messages: [{ role: 'user', text: 'secret' }], busy: true }
    const archived = applyServerMessage(state, { type: 'workspace_archived', workspace_id: 'a' })
    expect(archived.activeWorkspaceId).toBeUndefined()
    expect(archived.sessionId).toBeUndefined()
    expect(archived.messages).toEqual([])
    expect(archived.busy).toBe(false)
  })

  it('clears active UI state when the last workspace is archived', () => {
    const state = { ...initialSessionState, workspaces: [workspaces[0]], activeWorkspaceId: 'a', sessionId: 'session-a', messages: [{ role: 'user', text: 'private' }], approval: { requestId: 'approval', tool: 'tool', summary: 'private' } }
    const archived = applyServerMessage(state, { type: 'workspace_archived', workspace_id: 'a' })
    expect(archived.activeWorkspaceId).toBeUndefined()
    expect(archived.sessionId).toBeUndefined()
    expect(archived.messages).toEqual([])
    expect(archived.approval).toBeUndefined()
  })

  it('selects a new workspace without reusing another workspace session', () => {
    const selected = selectWorkspaceSession({ ...initialSessionState, workspaces }, 'new')
    expect(selected.sessionId).toBeUndefined()
    expect(selected.messages).toEqual([])
    expect(workspaceChanged(selected, 'b').sessionId).toBe('session-b')
  })
})
