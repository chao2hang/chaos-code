import type { WorkspaceInfo } from './generated/protocol'
import type { SessionState } from './session'

// The host reports `active_workspace_id` as the nil UUID when it has no active
// workspace, because the wire field is not optional. It is a placeholder, not an
// id: echoing it back on `create_session`/`resume` is a lookup for a workspace
// that never existed, which the host answers with `workspace_unavailable`.
export const NIL_WORKSPACE_ID = '00000000-0000-0000-0000-000000000000'

export function activeWorkspaceIdOrNull(workspaceId: string | undefined): string | null {
  return workspaceId && workspaceId !== NIL_WORKSPACE_ID ? workspaceId : null
}

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
    toolActivities: [],
    turnOutcomes: {},
    gitLoading: false,
    gitError: undefined,
    terminalLoading: false,
    terminalError: undefined,
  }
}

export function workspaceChanged(state: SessionState, workspaceId?: string): SessionState {
  const workspace = workspaceId ? state.workspaces.find((item) => item.id === workspaceId) : undefined
  return {
    ...state,
    activeWorkspaceId: workspaceId,
    sessionId: workspaceId ? state.workspaceSessions[workspaceId] ?? workspace?.last_session_id : undefined,
    messages: [],
    approval: undefined,
    question: undefined,
    files: undefined,
    filesLoading: false,
    filesError: undefined,
    activeFile: undefined,
    fileLoading: false,
    fileError: undefined,
    searchResults: undefined,
    searchLoading: false,
    searchError: undefined,
    busy: false,
    status: '工作区已切换',
    toolActivities: [],
    turnOutcomes: {},
    gitStatus: undefined,
    gitMutationResult: undefined,
    gitLoading: false,
    gitError: undefined,
    terminalResult: undefined,
    terminalLoading: false,
    terminalError: undefined,
  }
}

export function workspaceSessionMap(workspaces: WorkspaceInfo[]): Record<string, string> {
  return Object.fromEntries(
    workspaces.flatMap((workspace) => workspace.last_session_id
      ? [[workspace.id, workspace.last_session_id] as const]
      : []),
  )
}
