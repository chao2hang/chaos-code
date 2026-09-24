import { describe, expect, it } from 'vitest'
import { initialSessionState } from './session'
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

  it('selects a new workspace without reusing another workspace session', () => {
    const selected = selectWorkspaceSession({ ...initialSessionState, workspaces }, 'new')
    expect(selected.sessionId).toBeUndefined()
    expect(selected.messages).toEqual([])
    expect(workspaceChanged(selected, 'b').sessionId).toBe('session-b')
  })
})
