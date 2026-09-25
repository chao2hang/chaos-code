import type { ClientMessage, ServerMessage as ProtocolServerMessage, TimelineMessage } from './generated/protocol'

export type Message = TimelineMessage
export type Approval = { requestId: string; tool: string; summary: string }
export type Question = { questionId: string; prompt: string }
export type ServerMessage = ProtocolServerMessage

import type { WorkspaceInfo } from './generated/protocol'
import { workspaceChanged, workspaceSessionMap } from './workspace-ui'

export type SessionState = {
  messages: Message[]
  workspaceSessions: Record<string, string>
  workspaces: WorkspaceInfo[]
  activeWorkspaceId?: string
  busy: boolean
  status: string
  sessionId?: string
  approval?: Approval
  question?: Question
}

export const initialSessionState: SessionState = { messages: [], workspaceSessions: {}, workspaces: [], busy: false, status: '连接中' }

export function workspaceReconnectMessage(state: SessionState): ClientMessage {
  if (state.sessionId && state.activeWorkspaceId) {
    return { type: 'resume', client_msg_id: crypto.randomUUID(), session_id: state.sessionId, workspace_id: state.activeWorkspaceId }
  }
  return { type: 'create_session', client_msg_id: crypto.randomUUID(), workspace_id: state.activeWorkspaceId ?? null }
}

export function applyServerMessage(state: SessionState, message: ServerMessage): SessionState {
  if (message.type === 'session_created' && message.session_id) return { ...state, sessionId: message.session_id, workspaceSessions: { ...state.workspaceSessions, [message.workspace_id]: message.session_id }, activeWorkspaceId: message.workspace_id, status: '会话已创建' }
  if (message.type === 'workspaces') {
    const workspaceSessions = workspaceSessionMap(message.workspaces)
    const updated = { ...state, workspaces: message.workspaces, workspaceSessions }
    return state.activeWorkspaceId !== message.active_workspace_id
      ? workspaceChanged(updated, message.active_workspace_id)
      : { ...updated, activeWorkspaceId: message.active_workspace_id }
  }
  if (message.type === 'workspace_switched') return workspaceChanged(state, message.workspace_id)
  if (message.type === 'workspace_archived') {
    const nextState = { ...state, workspaces: state.workspaces.map((workspace) => workspace.id === message.workspace_id ? { ...workspace, archived: true } : workspace) }
    const candidates = nextState.workspaces.filter((workspace) => !workspace.archived && workspace.id !== message.workspace_id)
    const active = candidates.find((workspace) => workspace.id === state.activeWorkspaceId) ?? candidates[0]
    return state.activeWorkspaceId === message.workspace_id
      ? active
        ? workspaceChanged(nextState, active.id)
        : { ...nextState, activeWorkspaceId: undefined, sessionId: undefined, messages: [], approval: undefined, question: undefined, busy: false }
      : nextState
  }
  if (message.type === 'session_snapshot' && message.messages) return {
    ...state,
    messages: message.messages,
    activeWorkspaceId: message.workspace_id ?? state.activeWorkspaceId,
    sessionId: message.session_id,
    workspaceSessions: message.workspace_id ? { ...state.workspaceSessions, [message.workspace_id]: message.session_id } : state.workspaceSessions,
    approval: message.pending_approval ? { requestId: message.pending_approval.request_id, tool: message.pending_approval.tool, summary: message.pending_approval.summary } : undefined,
    question: message.pending_question ? { questionId: message.pending_question.question_id, prompt: message.pending_question.prompt || '请继续回答待处理问题' } : undefined,
    status: message.pending_approval ? '等待审批' : message.pending_question ? '等待回答' : '历史已恢复',
  }
  if (message.type === 'tool_approval_requested' && message.request_id) return { ...state, busy: false, approval: { requestId: message.request_id, tool: message.tool ?? 'unknown', summary: message.summary ?? '' }, status: '等待审批' }
  if (message.type === 'question_requested' && message.question_id) return { ...state, busy: false, question: { questionId: message.question_id, prompt: message.prompt ?? '' }, status: '等待回答' }
  if (message.type === 'approval_resolved') return {
    ...state,
    approval: undefined,
    status: message.approved ? '工具已执行' : state.status === '工具执行失败' ? '工具执行失败' : '审批已拒绝',
  }
  if (message.type === 'question_resolved') return { ...state, question: undefined, status: '回答已提交' }
  if (message.type === 'text_delta') {
    const last = state.messages[state.messages.length - 1]
    const messages = !last || last.role !== 'assistant'
      ? [...state.messages, { role: 'assistant' as const, text: message.text ?? '' }]
      : [...state.messages.slice(0, -1), { ...last, text: `${last.text}${message.text ?? ''}` }]
    return { ...state, messages }
  }
  if (message.type === 'completed' || message.type === 'cancelled') return { ...state, busy: false }
  if (message.type === 'error') {
    const toolFailure = ['tool_unavailable', 'tool_failed', 'terminal_unavailable', 'terminal_failed', 'git_failed'].includes(message.code ?? '')
    return { ...state, busy: false, status: toolFailure ? '工具执行失败' : '请求错误' }
  }
  return state
}
