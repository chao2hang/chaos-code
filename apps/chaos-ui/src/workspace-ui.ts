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
    toolActivities: [],
    gitLoading: false,
    gitError: undefined,
    terminalLoading: false,
    terminalError: undefined,
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
