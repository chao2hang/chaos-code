import type { WorkspaceInfo } from './generated/protocol'
import type { SessionState } from './session'

export function selectWorkspaceSession(state: SessionState, workspaceId: string): SessionState {
  const workspace = state.workspaces.find((item) => item.id === workspaceId)
  return {
    ...state,
    activeWorkspaceId: workspaceId,
    sessionId: state.workspaceSessions[workspaceId] ?? workspace?.last_session_id ?? undefined,
    messages: [],
    approval: undefined,
    question: undefined,
    busy: false,
    status: '正在切换工作区',
  }
}

export function workspaceChanged(state: SessionState, workspaceId: string): SessionState {
  const workspace = state.workspaces.find((item) => item.id === workspaceId)
  return {
    ...state,
    activeWorkspaceId: workspaceId,
    sessionId: state.workspaceSessions[workspaceId] ?? workspace?.last_session_id ?? undefined,
    messages: [],
    approval: undefined,
    question: undefined,
    busy: false,
    status: '工作区已切换',
  }
}

export function workspaceSessionMap(workspaces: WorkspaceInfo[]): Record<string, string> {
  return Object.fromEntries(
    workspaces.flatMap((workspace) => workspace.last_session_id
      ? [[workspace.id, workspace.last_session_id] as const]
      : []),
  )
}
