import { newMessageId } from './transport'
import type { ClientMessage, DiffPreview, HostInfo, MarketplaceEntry, ServerMessage as ProtocolServerMessage, TimelineMessage } from './generated/protocol'

export type Message = TimelineMessage
export type Approval = { requestId: string; tool: string; summary: string; confirmationStep: number }
export type Question = { questionId: string; prompt: string }
/** `turnAnchor` is the transcript length when the tool started; the turn it belongs
 * to is read back out of the messages, so a projection never has to trust a counter
 * that a reconnect could have rebuilt differently. `unresolved` is set here rather
 * than by the host: the turn ended without a `tool_result`, so the card must stop
 * claiming the tool is still running. */
export type ToolActivity = { id: string; tool: string; status: 'running' | 'completed' | 'unresolved'; progress?: string; result?: string; turnAnchor: number }
export type PendingGitOperation = { requestId: string; sessionId: string; operation: string }
export type Upload = {
  filename: string
  byteLen: number
  uploadId?: string
  sentBytes: number
  status: UploadStatus
  path?: string
  bytes?: number
  error?: string
}
export type ServerMessage = ProtocolServerMessage

/** How a turn ended, as far as this connection could see it. `streaming` is derived
 * from `busy`, the other three come from the terminal event that said so. */
export type TurnOutcome = 'streaming' | 'completed' | 'cancelled' | 'failed'

/** One prompt and everything the host produced for it. `key` is the ordinal of the
 * prompt within this view; -1 is content that arrived before any prompt here, which
 * is what a restored transcript looks like before the first send. */
export type Turn = { key: number; prompt: Message | null; replies: Message[]; tools: ToolActivity[]; outcome: TurnOutcome | undefined }

import type { WorkspaceInfo } from './generated/protocol'
import { isUploadFailure, type UploadStatus } from './attachments'
import { activeWorkspaceIdOrNull, NIL_WORKSPACE_ID, workspaceChanged, workspaceSessionMap } from './workspace-ui'

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
  files?: { path: string; entries: string[]; directories: string[] }
  filesLoading: boolean
  filesError?: string
  activeFile?: { path: string; contents: string }
  fileLoading: boolean
  fileError?: string
  searchResults?: { query: string; matches: string[] }
  searchLoading: boolean
  searchError?: string
  gitStatus?: { branch: string | null; entries: string[] }
  gitMutationResult?: { operation: string; result: string }
  gitLoading: boolean
  gitError?: string
  /** A commit message the host's Provider offered for what is staged. Filling the
   * commit form with it is a separate, deliberate step: nothing here commits.
   * `truncated` is the host's warning that the diff behind the wording was cut
   * short, so the message may describe only part of the change. */
  commitSuggestion?: { message: string; truncated: boolean }
  commitSuggesting: boolean
  commitSuggestionError?: string
  terminalLoading: boolean
  terminalError?: string
  settings?: { baseUrl: string | null; model: string | null; hasApiKey: boolean }
  /** What the serving process says about itself; undefined until it answers `get_host_info`. */
  hostInfo?: HostInfo
  providerValidation?: { baseUrl: string; model: string; reachable: boolean; errorCode: string | null }
  diffPreview?: DiffPreview
  /** Why the last 接受/回滚 was refused. The preview stays on screen with it: a
   * change the host would not undo is a change the user still has to see. */
  diffError?: string
  marketplaceEntries?: MarketplaceEntry[]
  terminalResult?: { output: string; exit_code: number }
  tuiImport?: { sessionId: string; cwd: string; title: string | null; messageCount: number; sourceUnchanged: boolean }
  usage?: { inputTokens: number; outputTokens: number }
  toolActivities: ToolActivity[]
  /** How each turn seen in this view ended, keyed by turn key. Restored history and
   * another workspace's transcript start it over: an outcome is only ever recorded
   * for a turn this connection actually watched finish. */
  turnOutcomes: Record<string, TurnOutcome>
  pendingGitOperation?: PendingGitOperation
  upload?: Upload
}

export const initialSessionState: SessionState = { messages: [], workspaceSessions: {}, workspaces: [], busy: false, status: '连接中', filesLoading: false, fileLoading: false, searchLoading: false, gitLoading: false, commitSuggesting: false, terminalLoading: false, toolActivities: [], turnOutcomes: {} }

/** Whether a commit-message suggestion may replace what is in the commit form.
 * An empty box always takes it; a box holding exactly what was there when the
 * request went out still does. Anything else means the user typed while waiting,
 * and their own words win -- the suggestion is offered beside the box instead. */
export function commitDraftAcceptsSuggestion(draft: string, draftAtRequest: string | null): boolean {
  return draft.trim() === '' || draft === draftAtRequest
}

/** The turn new events belong to: the one the newest prompt in `messages` opened, or
 * -1 while nothing has been prompted in this view. */
export function liveTurnKey(messages: Message[]): number {
  let key = -1
  for (const message of messages) if (message.role === 'user') key += 1
  return key
}

function recordTurnOutcome(state: SessionState, outcome: TurnOutcome): Record<string, TurnOutcome> {
  return { ...state.turnOutcomes, [String(liveTurnKey(state.messages))]: outcome }
}

/** The host ended the turn, so a card still marked running will never be answered;
 * it is settled here instead of being left to claim activity that already stopped. */
function settleRunningTools(toolActivities: ToolActivity[]): ToolActivity[] {
  if (!toolActivities.some((activity) => activity.status === 'running')) return toolActivities
  return toolActivities.map((activity) => (activity.status === 'running' ? { ...activity, status: 'unresolved' as const } : activity))
}

/** The newest card of `tool` that has not reported a result yet. Newest, because an
 * earlier turn may have left a card unsettled, and a later `tool_result` belongs to
 * the run that is actually in flight. */
function openToolIndex(toolActivities: ToolActivity[], tool: string): number {
  for (let index = toolActivities.length - 1; index >= 0; index -= 1) {
    if (toolActivities[index].tool === tool && toolActivities[index].status !== 'completed') return index
  }
  return -1
}

/** A prompt the local user just sent. The reducer owns this so the transcript, the
 * turn keys and the busy flag can only ever move together. */
export function appendLocalPrompt(state: SessionState, text: string): SessionState {
  return { ...state, messages: [...state.messages, { role: 'user', text }, { role: 'assistant', text: '' }], busy: true }
}

export function groupIntoTurns(state: SessionState): Turn[] {
  const turns: Turn[] = [{ key: -1, prompt: null, replies: [], tools: [], outcome: state.turnOutcomes['-1'] }]
  const byKey = new Map<number, Turn>([[turns[0].key, turns[0]]])
  let current = turns[0]
  let prompts = 0
  for (const message of state.messages) {
    if (message.role === 'user') {
      current = { key: prompts, prompt: message, replies: [], tools: [], outcome: state.turnOutcomes[String(prompts)] }
      prompts += 1
      turns.push(current)
      byKey.set(current.key, current)
    } else {
      current.replies.push(message)
    }
  }
  for (const tool of state.toolActivities) {
    const anchor = Math.min(Math.max(tool.turnAnchor, 0), state.messages.length)
    const turn = byKey.get(liveTurnKey(state.messages.slice(0, anchor))) ?? turns[0]
    turn.tools.push(tool)
  }
  if (state.busy) {
    const live = byKey.get(liveTurnKey(state.messages))
    if (live && live.outcome === undefined) live.outcome = 'streaming'
  }
  return turns.filter((turn) => turn.key !== -1 || turn.replies.length > 0 || turn.tools.length > 0)
}

export function eventBelongsToActiveSession(state: SessionState, message: ServerMessage): boolean {
  if (message.type === 'tui_session_import') return true
  if (!('session_id' in message) || typeof message.session_id !== 'string') return true
  return !state.sessionId || message.session_id === state.sessionId
}

export function fileChangeAffectsVisibleDirectory(state: SessionState, message: ServerMessage): boolean {
  if (message.type !== 'file_changed' || !state.files || !eventBelongsToActiveSession(state, message)) return false
  if (message.operation !== 'write' && message.operation !== 'attachment_write') return false
  const path = message.path.replace(/\\/g, '/')
  const parts = path.split('/').filter(Boolean)
  const parentPath = parts.length > 1 ? parts.slice(0, -1).join('/') : '.'
  const listedPath = (state.files.path || '.').replace(/\\/g, '/').replace(/\/$/, '') || '.'
  return parentPath === listedPath
}

// Both codes mean the host no longer knows the session we hold; a host restart is
// the usual cause, because sessions live in the process. Left alone, the composer
// keeps pointing at an id nothing answers: submit() returns early without a session
// id, so pressing send would do nothing at all.
const lostSessionErrorCodes = ['session_not_found', 'workspace_session_mismatch']

// The single answer Safe Web Mode gives for every request it refuses. It comes from
// the socket rather than the engine, so it carries no session id and does not name
// the message it refused; the error branch below attributes it to whatever request
// is in flight.
const safeWebModeRefusalCode = 'safe_web_mode_blocked'

// The codes the host uses to refuse a commit-message suggestion. Each of them is the
// whole answer to 建议提交信息: nothing else follows, so the button has to stop on the
// spot. session_not_found covers a host that dropped the session and
// safe_web_mode_blocked a safe-mode host that refuses the read outright; leaving
// either pending would freeze the button at 建议生成中 with no way to retry.
//
// A request is answered exactly once, so a panel that is waiting on something else
// still has its own answer coming; where two refused requests are in flight at the
// same time, the order of the branches below only decides which panel hears about
// the first one.
const commitSuggestionFailureCodes = ['session_not_found', 'workspace_unavailable', 'git_failed', 'nothing_staged', 'commit_suggestion_unavailable', 'agent_failed', 'commit_suggestion_empty', safeWebModeRefusalCode]

export function sessionLossRecoveryMessage(state: SessionState, message: ServerMessage): ClientMessage | null {
  if (message.type !== 'error' || !lostSessionErrorCodes.includes(message.code)) return null
  return { type: 'create_session', client_msg_id: newMessageId(), workspace_id: activeWorkspaceIdOrNull(state.activeWorkspaceId) }
}

export function workspaceReconnectMessage(state: SessionState): ClientMessage {  const workspaceId = activeWorkspaceIdOrNull(state.activeWorkspaceId)
  if (state.sessionId && workspaceId) {
    return { type: 'resume', client_msg_id: newMessageId(), session_id: state.sessionId, workspace_id: workspaceId }
  }
  return { type: 'create_session', client_msg_id: newMessageId(), workspace_id: workspaceId }
}

export function applyServerMessage(state: SessionState, message: ServerMessage): SessionState {
  if (message.type !== 'session_created' && message.type !== 'workspaces' && message.type !== 'workspace_switched' && message.type !== 'workspace_archived' && message.type !== 'diff_resolved' && !eventBelongsToActiveSession(state, message)) return state
  return applyServerMessageProjection(state, message)
}

function applyServerMessageProjection(state: SessionState, message: ServerMessage): SessionState {
  if (message.type === 'session_created' && message.session_id) return { ...state, sessionId: message.session_id, workspaceSessions: { ...state.workspaceSessions, [message.workspace_id]: message.session_id }, activeWorkspaceId: message.workspace_id, messages: [], approval: undefined, question: undefined, toolActivities: [], turnOutcomes: {}, status: '会话已创建' }
  if (message.type === 'workspaces') {
    const workspaceSessions = workspaceSessionMap(message.workspaces)
    const updated = { ...state, workspaces: message.workspaces, workspaceSessions }
    const activeWorkspaceId = message.active_workspace_id === NIL_WORKSPACE_ID ? undefined : message.active_workspace_id
    return state.activeWorkspaceId !== activeWorkspaceId
      ? workspaceChanged(updated, activeWorkspaceId)
      : { ...updated, activeWorkspaceId }
  }
  if (message.type === 'workspace_switched') return workspaceChanged(state, message.workspace_id)
  if (message.type === 'workspace_archived') {
    const nextState = { ...state, workspaces: state.workspaces.map((workspace) => workspace.id === message.workspace_id ? { ...workspace, archived: true } : workspace) }
    const candidates = nextState.workspaces.filter((workspace) => !workspace.archived && workspace.id !== message.workspace_id)
    const active = candidates.find((workspace) => workspace.id === state.activeWorkspaceId) ?? candidates[0]
    return state.activeWorkspaceId === message.workspace_id
      ? active
        ? workspaceChanged(nextState, active.id)
        : { ...nextState, activeWorkspaceId: undefined, sessionId: undefined, messages: [], approval: undefined, question: undefined, busy: false, toolActivities: [], turnOutcomes: {} }
      : nextState
  }
  if (message.type === 'session_snapshot' && message.messages) return {
    ...state,
    messages: message.messages,
    activeWorkspaceId: message.workspace_id ?? state.activeWorkspaceId,
    sessionId: message.session_id,
    workspaceSessions: message.workspace_id ? { ...state.workspaceSessions, [message.workspace_id]: message.session_id } : state.workspaceSessions,
    approval: message.pending_approval ? { requestId: message.pending_approval.request_id, tool: message.pending_approval.tool, summary: message.pending_approval.summary, confirmationStep: message.pending_approval.confirmations + 1 } : undefined,
    question: message.pending_question ? { questionId: message.pending_question.question_id, prompt: message.pending_question.prompt || '请继续回答待处理问题' } : undefined,
    status: message.pending_approval ? '等待审批' : message.pending_question ? '等待回答' : '历史已恢复',
    toolActivities: [],
    turnOutcomes: {},
  }
  if (message.type === 'tool_approval_requested' && message.request_id) {
    const approval = { requestId: message.request_id, tool: message.tool ?? 'unknown', summary: message.summary ?? '', confirmationStep: state.approval?.requestId === message.request_id ? state.approval.confirmationStep + 1 : 1 }
    const gitOperation = approval.tool.startsWith('git.') ? approval.tool.slice(4) : undefined
    return {
      ...state,
      busy: false,
      approval,
      pendingGitOperation: gitOperation ? { requestId: approval.requestId, sessionId: message.session_id, operation: gitOperation } : state.pendingGitOperation,
      upload: approval.tool === 'workspace.attach_attachment' && state.upload ? { ...state.upload, status: 'awaiting_approval' } : state.upload,
      status: '等待审批',
    }
  }
  if (message.type === 'question_requested' && message.question_id) return { ...state, busy: false, question: { questionId: message.question_id, prompt: message.prompt ?? '' }, status: '等待回答' }
  if (message.type === 'approval_resolved') {
    const workspaceWrite = message.request_id === state.approval?.requestId && state.approval.tool === 'workspace.write_file'
    return {
      ...state,
      approval: undefined,
      pendingGitOperation: message.request_id === state.pendingGitOperation?.requestId && !message.approved ? undefined : state.pendingGitOperation,
      upload: state.upload?.status === 'awaiting_approval' && !message.approved ? { ...state.upload, status: 'cancelled' } : state.upload,
      status: message.approved
        ? workspaceWrite ? (state.status.startsWith('文件已写入：') ? state.status : '等待文件写入完成')
          : state.status.startsWith('终端执行完成') || state.status.startsWith('Git ') ? state.status
            : '工具已执行'
        : state.status === '工具执行失败' ? '工具执行失败' : '审批已拒绝',
    }
  }
  if (message.type === 'question_resolved') return { ...state, question: undefined, status: '回答已提交' }
  if (message.type === 'tool_started') {
    const activity: ToolActivity = { id: `${message.session_id}:${message.sequence}`, tool: message.tool, status: 'running', turnAnchor: state.messages.length }
    return { ...state, toolActivities: [...state.toolActivities, activity].slice(-20) }
  }
  if (message.type === 'tool_progress') {
    const index = openToolIndex(state.toolActivities, message.tool)
    if (index < 0) return state
    const toolActivities = [...state.toolActivities]
    toolActivities[index] = { ...toolActivities[index], progress: message.progress.slice(0, 2000) }
    return { ...state, toolActivities }
  }
  if (message.type === 'tool_result') {
    const index = openToolIndex(state.toolActivities, message.tool)
    if (index < 0) return state
    const toolActivities = [...state.toolActivities]
    toolActivities[index] = { ...toolActivities[index], status: 'completed', result: message.result.slice(0, 8000) }
    return { ...state, toolActivities }
  }
  if (message.type === 'text_delta') {
    const last = state.messages[state.messages.length - 1]
    const messages = !last || last.role !== 'assistant'
      ? [...state.messages, { role: 'assistant' as const, text: message.text ?? '' }]
      : [...state.messages.slice(0, -1), { ...last, text: `${last.text}${message.text ?? ''}` }]
    return { ...state, messages }
  }
  if (message.type === 'files_listed') return { ...state, files: { path: message.path, entries: message.entries, directories: message.directories }, filesLoading: false, filesError: undefined }
  if (message.type === 'attachment_validated') return state.upload?.status === 'validating' ? { ...state, upload: { ...state.upload, status: 'beginning' } } : state
  if (message.type === 'attachment_started') return { ...state, upload: { filename: message.filename, byteLen: state.upload?.byteLen ?? 0, uploadId: message.upload_id, sentBytes: 0, status: 'uploading' }, status: `正在上传 ${message.filename}` }
  if (message.type === 'attachment_progress') return state.upload?.uploadId === message.upload_id ? { ...state, upload: { ...state.upload, sentBytes: message.received } } : state
  if (message.type === 'attachment_cancelled') return state.upload?.uploadId === message.upload_id ? { ...state, upload: { ...state.upload, status: 'cancelled' }, status: '上传已取消' } : state
  if (message.type === 'attachment_completed') return {
    ...state,
    upload: { filename: state.upload?.filename ?? message.path, byteLen: state.upload?.byteLen ?? message.bytes, uploadId: message.upload_id, sentBytes: message.bytes, status: 'done', path: message.path, bytes: message.bytes },
    status: `附件已写入 ${message.path}（${message.bytes} 字节）`,
  }
  if (fileChangeAffectsVisibleDirectory(state, message)) return { ...state, filesLoading: true, filesError: undefined }
  if (message.type === 'file_contents') return { ...state, activeFile: { path: message.path, contents: message.contents }, fileLoading: false, fileError: undefined }
  if (message.type === 'search_results') return { ...state, searchResults: { query: message.query, matches: message.matches }, searchLoading: false, searchError: undefined }
  if (message.type === 'git_status') return { ...state, gitStatus: { branch: message.branch, entries: message.entries }, gitLoading: false, gitError: undefined }
  if (message.type === 'commit_message_suggestion') return { ...state, commitSuggestion: { message: message.message, truncated: message.truncated }, commitSuggesting: false, commitSuggestionError: undefined, status: message.truncated ? '提交信息建议已生成（暂存差异被截断）' : '提交信息建议已生成' }
  if (message.type === 'settings') return { ...state, settings: { baseUrl: message.base_url, model: message.model, hasApiKey: message.has_api_key } }
  // Every row of the settings panel other than the model/provider fields is read
  // from here, so a host that never answers leaves the panel saying so instead of
  // showing a plausible-looking default that describes a different process.
  if (message.type === 'host_info') return { ...state, hostInfo: message.info }
  if (message.type === 'settings_updated') return { ...state, settings: { baseUrl: message.base_url, model: message.model, hasApiKey: state.settings?.hasApiKey ?? false }, status: '设置已更新' }
  if (message.type === 'diff_preview') return { ...state, diffPreview: message.preview, diffError: undefined }
  if (message.type === 'diff_resolved') return { ...state, diffPreview: undefined, diffError: undefined, status: `Diff ${message.action}` }
  if (message.type === 'marketplace_scan') return { ...state, marketplaceEntries: message.entries }
  if (message.type === 'terminal_result') return { ...state, terminalResult: { output: message.output, exit_code: message.exit_code }, terminalLoading: false, terminalError: undefined, status: `终端执行完成（退出码：${message.exit_code}）` }
  if (message.type === 'git_mutation_result') {
    const operation = message.operation.startsWith('git.') ? message.operation.slice(4) : message.operation
    const pendingGitOperation = state.pendingGitOperation?.operation === operation ? undefined : state.pendingGitOperation
    return { ...state, pendingGitOperation, gitMutationResult: { operation, result: message.result }, gitLoading: false, gitError: undefined, status: `Git ${operation} 执行完成` }
  }
  if (message.type === 'provider_validation') return { ...state, providerValidation: { baseUrl: message.base_url, model: message.model, reachable: message.reachable, errorCode: message.error_code }, status: message.reachable ? 'Provider 验证通过' : `Provider 验证未通：${message.error_code ?? '未知错误'}` }
  if (message.type === 'tui_session_import') return { ...state, tuiImport: { sessionId: message.session_id, cwd: message.cwd, title: message.title, messageCount: message.message_count, sourceUnchanged: message.source_unchanged }, status: `已导入 TUI 会话：${message.session_id}` }
  if (message.type === 'usage') return { ...state, usage: { inputTokens: message.input_tokens, outputTokens: message.output_tokens } }
  if (message.type === 'file_written') {
    const toolActivities = state.toolActivities.map((activity) => activity.tool === 'workspace.write_file' && activity.status !== 'completed'
      ? { ...activity, status: 'completed' as const, result: `已写入 ${message.path}（${message.bytes} 字节）` }
      : activity)
    return { ...state, toolActivities, status: `文件已写入：${message.path}（${message.bytes} 字节）` }
  }
  if (message.type === 'completed' || message.type === 'cancelled') return { ...state, busy: false, toolActivities: settleRunningTools(state.toolActivities), turnOutcomes: recordTurnOutcome(state, message.type === 'cancelled' ? 'cancelled' : 'completed') }
  if (message.type === 'error') {
    const code = message.code ?? ''
    // Safe Web Mode refuses in the socket, before the engine sees the message, so
    // its answer names no request and says only that the message was not passed
    // through. It is still an answer, and every request the mode refuses is refused
    // with these same words, so the panel waiting on one has to be told: a control
    // left showing 处理中 is waiting for a reply that will never arrive.
    const refusedBySafeMode = code === safeWebModeRefusalCode
    if (state.upload && isUploadFailure(code, state.upload.status)) return { ...state, busy: false, upload: { ...state.upload, status: 'failed', error: message.message }, toolActivities: settleRunningTools(state.toolActivities), turnOutcomes: recordTurnOutcome(state, 'failed'), status: '上传失败' }
    // A refused commit-message suggestion is about the commit form, not about the
    // turn: it must not settle the turn as failed, nor clear a pending approval
    // that belongs to a Git mutation the user has not resolved yet.
    if (state.commitSuggesting && commitSuggestionFailureCodes.includes(code)) return { ...state, commitSuggesting: false, commitSuggestionError: message.message, status: '提交信息建议失败' }
    const toolFailure = ['tool_unavailable', 'tool_failed', 'terminal_unavailable', 'terminal_failed', 'git_failed'].includes(code)
    if (state.gitLoading && (code === 'git_failed' || refusedBySafeMode)) return { ...state, pendingGitOperation: undefined, gitLoading: false, gitError: message.message, status: 'Git 操作失败' }
    if (state.terminalLoading && (['terminal_unavailable', 'terminal_failed'].includes(code) || refusedBySafeMode)) return { ...state, terminalLoading: false, terminalError: message.message, status: '终端执行失败' }
    // A refused 接受/回滚 keeps the preview: the refusal means the file is no longer
    // what the preview claims, which is exactly when the user still has to see it.
    if (code === 'diff_failed') return { ...state, busy: false, diffError: message.message, status: '差异操作失败' }
    if (state.filesLoading && !state.fileLoading && !state.searchLoading) return { ...state, filesLoading: false, filesError: message.message, status: '目录读取失败' }
    if (state.fileLoading && !state.filesLoading && !state.searchLoading) return { ...state, fileLoading: false, fileError: message.message, status: '文件读取失败' }
    if (state.searchLoading && !state.filesLoading && !state.fileLoading) return { ...state, searchLoading: false, searchError: message.message, status: '文件搜索失败' }
    if (state.filesLoading || state.fileLoading || state.searchLoading) return { ...state, filesLoading: false, fileLoading: false, searchLoading: false, status: '文件请求失败' }
    // No panel was waiting, so this refused something the app asks for on its own --
    // the workspace list it sends on connect is the usual one. Saying 请求错误 over
    // the top of a session that works fine, and settling no turn at all as failed,
    // would both be wrong: the mode told us why nothing happened.
    if (refusedBySafeMode) return { ...state, status: '安全模式已拒绝' }
    return { ...state, busy: false, toolActivities: settleRunningTools(state.toolActivities), turnOutcomes: recordTurnOutcome(state, 'failed'), status: toolFailure ? '工具执行失败' : '请求错误' }
  }
  return state
}
