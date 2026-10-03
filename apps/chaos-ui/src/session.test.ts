import { describe, expect, it } from 'vitest'
import { applyServerMessage, fileChangeAffectsVisibleDirectory, initialSessionState, sessionLossRecoveryMessage, workspaceReconnectMessage } from './session'
import { NIL_WORKSPACE_ID } from './workspace-ui'
import type { HostInfo } from './generated/protocol'

describe('session event projection', () => {
  it('ignores late session-scoped events from a previous workspace session', () => {
    const current = {
      ...initialSessionState,
      sessionId: 'current-session',
      activeWorkspaceId: 'workspace-b',
      messages: [{ role: 'assistant', text: 'current transcript' }],
      status: '当前 workspace',
      gitStatus: { branch: 'current-branch', entries: [] },
    }
    const lateDelta = applyServerMessage(current, { type: 'text_delta', session_id: 'old-session', text: 'stale', sequence: 99 })
    const lateApproval = applyServerMessage(current, { type: 'tool_approval_requested', session_id: 'old-session', request_id: 'late-approval', tool: 'danger', summary: 'stale', sequence: 100 })
    const lateGit = applyServerMessage(current, { type: 'git_mutation_result', session_id: 'old-session', operation: 'git.stage', result: 'stale' })
    const lateTerminal = applyServerMessage(current, { type: 'terminal_result', session_id: 'old-session', output: 'stale', exit_code: 0 })
    const lateWorkspaceSnapshot = applyServerMessage(current, { type: 'session_snapshot', session_id: 'old-session', workspace_id: 'workspace-a', messages: [{ role: 'assistant', text: 'old transcript' }], sequence: 100, pending_approval: null, pending_question: null })
    const currentWorkspaceSnapshot = applyServerMessage(current, { type: 'session_snapshot', session_id: 'current-session', workspace_id: 'workspace-b', messages: [{ role: 'assistant', text: 'fresh snapshot' }], sequence: 2, pending_approval: null, pending_question: null })
    expect(lateDelta).toBe(current)
    expect(lateApproval).toBe(current)
    expect(lateGit).toBe(current)
    expect(lateTerminal).toBe(current)
    expect(lateWorkspaceSnapshot).toBe(current)
    expect(currentWorkspaceSnapshot.messages).toEqual([{ role: 'assistant', text: 'fresh snapshot' }])
    expect(currentWorkspaceSnapshot.sessionId).toBe('current-session')
  })

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
    expect(approval.approval).toEqual({ requestId: 'r1', tool: 'demo', summary: 'write', confirmationStep: 1 })
    const resolved = applyServerMessage(approval, { type: 'approval_resolved', session_id: 's1', request_id: 'r1', approved: true, sequence: 2 })
    expect(resolved.approval).toBeUndefined()
    expect(resolved.status).toBe('工具已执行')
    const rejected = applyServerMessage(approval, { type: 'approval_resolved', session_id: 's1', request_id: 'r1', approved: false, sequence: 2 })
    expect(rejected.status).toBe('审批已拒绝')
    expect(applyServerMessage(approval, { type: 'error', code: 'network_error', message: 'offline' }).status).toBe('请求错误')
    const failed = applyServerMessage(approval, { type: 'error', code: 'tool_unavailable', message: '没有配置获准的工具 adapter' })
    expect(failed.status).toBe('工具执行失败')
    const rejectedAfterFailure = applyServerMessage(failed, { type: 'approval_resolved', session_id: 's1', request_id: 'r1', approved: false, sequence: 2 })
    expect(rejectedAfterFailure.status).toBe('工具执行失败')
    const question = applyServerMessage(initialSessionState, { type: 'question_requested', session_id: 's1', question_id: 'q1', prompt: 'continue?', sequence: 1 })
    expect(question.question?.questionId).toBe('q1')
    expect(applyServerMessage(question, { type: 'question_resolved', session_id: 's1', question_id: 'q1', answer: 'yes', sequence: 2 }).question).toBeUndefined()
  })

  it('projects workspace list, switch, and archive events', () => {
    let state = applyServerMessage(initialSessionState, { type: 'workspaces', active_workspace_id: 'w1', workspaces: [{ id: 'w1', name: '默认工作区', archived: false, last_used_sequence: 1, last_session_id: null }, { id: 'w2', name: '项目', archived: false, last_used_sequence: 2, last_session_id: null }] })
    expect(state.workspaces).toHaveLength(2)
    state = applyServerMessage(state, { type: 'workspace_switched', workspace_id: 'w2' })
    expect(state.activeWorkspaceId).toBe('w2')
    expect(state.messages).toEqual([])
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

  it('treats the host placeholder for a missing active workspace as no workspace at all', () => {
    const before = {
      ...initialSessionState,
      activeWorkspaceId: 'workspace-a',
      sessionId: 'session-a',
      workspaceSessions: { 'workspace-a': 'session-a' },
      messages: [{ role: 'assistant' as const, text: '上一段对话' }],
    }
    const after = applyServerMessage(before, { type: 'workspaces', active_workspace_id: NIL_WORKSPACE_ID, workspaces: [] })
    expect(after.activeWorkspaceId).toBeUndefined()
    expect(after.sessionId).toBeUndefined()
    expect(after.messages).toEqual([])
    // Echoing the placeholder back would be answered with `workspace_unavailable`.
    expect(workspaceReconnectMessage(after)).toMatchObject({ type: 'create_session', workspace_id: null })
    expect(workspaceReconnectMessage({ ...after, sessionId: 'session-a' })).toMatchObject({ type: 'create_session', workspace_id: null })
  })

  it('asks for a replacement session when the host no longer knows the current one', () => {
    const state = { ...initialSessionState, activeWorkspaceId: 'workspace-a', sessionId: 'session-a' }
    expect(sessionLossRecoveryMessage(state, { type: 'error', code: 'session_not_found', message: '会话不存在' }))
      .toMatchObject({ type: 'create_session', workspace_id: 'workspace-a' })
    expect(sessionLossRecoveryMessage(state, { type: 'error', code: 'workspace_session_mismatch', message: '会话不属于请求的工作区' }))
      .toMatchObject({ type: 'create_session', workspace_id: 'workspace-a' })
    expect(sessionLossRecoveryMessage({ ...state, activeWorkspaceId: NIL_WORKSPACE_ID }, { type: 'error', code: 'session_not_found', message: '会话不存在' }))
      .toMatchObject({ type: 'create_session', workspace_id: null })
    expect(sessionLossRecoveryMessage(state, { type: 'error', code: 'approval_pending', message: '已有待审批操作' })).toBeNull()
    expect(sessionLossRecoveryMessage(state, { type: 'completed', session_id: 'session-a', sequence: 1 })).toBeNull()
  })

  it('projects file browser loading, empty, and error transitions', () => {
    let state = { ...initialSessionState, filesLoading: true }
    state = applyServerMessage(state, { type: 'files_listed', path: 'empty-folder', entries: [], directories: [] })
    expect(state.filesLoading).toBe(false)
    expect(state.filesError).toBeUndefined()
    expect(state.files).toEqual({ path: 'empty-folder', entries: [], directories: [] })

    state = applyServerMessage({ ...state, fileLoading: true }, { type: 'error', code: 'read_failed', message: '目标不是普通文件' })
    expect(state.fileLoading).toBe(false)
    expect(state.fileError).toBe('目标不是普通文件')

    state = applyServerMessage({ ...state, searchLoading: true }, { type: 'error', code: 'search_failed', message: '搜索不可用' })
    expect(state.searchLoading).toBe(false)
    expect(state.searchError).toBe('搜索不可用')

    state = applyServerMessage({ ...state, filesLoading: true }, { type: 'error', code: 'path_invalid', message: '路径不存在' })
    expect(state.filesLoading).toBe(false)
    expect(state.filesError).toBe('路径不存在')

    state = applyServerMessage({ ...state, filesLoading: true, searchLoading: true }, { type: 'error', code: 'request_failed', message: '请求失败' })
    expect(state.filesLoading).toBe(false)
    expect(state.searchLoading).toBe(false)
    expect(state.status).toBe('文件请求失败')
  })

  it('matches visible workspace paths against active-session file changes', () => {
    const visibleRoot = { ...initialSessionState, sessionId: 'active-session', files: { path: '.', entries: ['note.txt'], directories: [] } }
    const visibleNested = { ...initialSessionState, sessionId: 'active-session', files: { path: 'nested/', entries: ['note.txt'], directories: [] } }

    expect(fileChangeAffectsVisibleDirectory(visibleRoot, { type: 'file_changed', session_id: 'active-session', path: 'note.txt', operation: 'write', sequence: 1 })).toBe(true)
    expect(fileChangeAffectsVisibleDirectory(visibleRoot, { type: 'file_changed', session_id: 'active-session', path: 'nested/note.txt', operation: 'write', sequence: 1 })).toBe(false)
    expect(fileChangeAffectsVisibleDirectory(visibleNested, { type: 'file_changed', session_id: 'active-session', path: 'nested/note.txt', operation: 'write', sequence: 1 })).toBe(true)
    expect(fileChangeAffectsVisibleDirectory(visibleNested, { type: 'file_changed', session_id: 'old-session', path: 'nested/note.txt', operation: 'write', sequence: 2 })).toBe(false)
    expect(fileChangeAffectsVisibleDirectory(visibleNested, { type: 'file_changed', session_id: 'active-session', path: 'nested/note.txt', operation: 'delete', sequence: 3 })).toBe(false)
  })

  it('completes workspace write activity when the backend confirms bytes written', () => {
    const running = applyServerMessage(initialSessionState, { type: 'tool_started', session_id: 's1', tool: 'workspace.write_file', sequence: 1 })
    const written = applyServerMessage(running, { type: 'file_written', session_id: 's1', path: 'nested/needle.txt', bytes: 34 })
    expect(written.toolActivities).toEqual([{ id: 's1:1', tool: 'workspace.write_file', status: 'completed', result: '已写入 nested/needle.txt（34 字节）' }])
  })

  it('projects bounded Git and terminal loading errors and clears them on success', () => {
    let git = applyServerMessage({ ...initialSessionState, gitLoading: true }, { type: 'error', code: 'git_failed', message: 'Git root rejected' })
    expect(git).toMatchObject({ gitLoading: false, gitError: 'Git root rejected', status: 'Git 操作失败' })
    git = applyServerMessage({ ...git, gitLoading: true }, { type: 'git_mutation_result', session_id: 's1', operation: 'git.stage', result: 'staged 1 file' })
    expect(git).toMatchObject({ gitLoading: false, gitError: undefined, status: 'Git stage 执行完成' })

    const terminal = applyServerMessage({ ...initialSessionState, terminalLoading: true }, { type: 'error', code: 'terminal_unavailable', message: 'adapter missing' })
    expect(terminal).toMatchObject({ terminalLoading: false, terminalError: 'adapter missing', status: '终端执行失败' })
    const completed = applyServerMessage({ ...terminal, terminalLoading: true }, { type: 'terminal_result', session_id: 's1', output: 'ok', exit_code: 0 })
    expect(completed).toMatchObject({ terminalLoading: false, terminalError: undefined, status: '终端执行完成（退出码：0）' })
  })

  it('preserves terminal and Git operation results when approval resolution follows them', () => {
    const terminalApproved = applyServerMessage({ ...initialSessionState, approval: { requestId: 'terminal-1', tool: 'terminal.execute', summary: 'pwd', confirmationStep: 1 } }, { type: 'terminal_result', session_id: 's1', output: '/workspace', exit_code: 0 })
    const terminalResolved = applyServerMessage(terminalApproved, { type: 'approval_resolved', session_id: 's1', request_id: 'terminal-1', approved: true, sequence: 1 })
    expect(terminalResolved.status).toBe('终端执行完成（退出码：0）')

    const gitApproved = applyServerMessage({ ...initialSessionState, approval: { requestId: 'git-1', tool: 'git.stage', summary: 'file.txt', confirmationStep: 1 } }, { type: 'git_mutation_result', session_id: 's1', operation: 'git.stage', result: 'staged 1 file' })
    const gitResolved = applyServerMessage(gitApproved, { type: 'approval_resolved', session_id: 's1', request_id: 'git-1', approved: true, sequence: 1 })
    expect(gitResolved.status).toBe('Git stage 执行完成')
  })

  it('projects approval resolution and actual Git operation result as separate status events', () => {
    const pending = applyServerMessage(initialSessionState, { type: 'tool_approval_requested', session_id: 's1', request_id: 'git-stage', tool: 'git.stage', summary: 'Requested Git stage', sequence: 1 })
    const approved = applyServerMessage(pending, { type: 'approval_resolved', session_id: 's1', request_id: 'git-stage', approved: true, sequence: 2 })
    expect(approved.status).toBe('工具已执行')
    const completed = applyServerMessage(approved, { type: 'git_mutation_result', session_id: 's1', operation: 'git.stage', result: 'staged 1 file' })
    expect(completed.status).toBe('Git stage 执行完成')
    expect(completed.gitMutationResult).toEqual({ operation: 'stage', result: 'staged 1 file' })
  })

  it('projects the backend second-approval warning as a later confirmation step', () => {
    const first = applyServerMessage(initialSessionState, { type: 'tool_approval_requested', session_id: 's1', request_id: 'approval-1', tool: 'git.commit', summary: 'Requested operation', sequence: 1 })
    expect(first.approval?.confirmationStep).toBe(1)
    const second = applyServerMessage(first, { type: 'tool_approval_requested', session_id: 's1', request_id: 'approval-1', tool: 'git.commit', summary: '破坏性 Git 操作需要再次确认', sequence: 2 })
    expect(second.approval).toEqual({ requestId: 'approval-1', tool: 'git.commit', summary: '破坏性 Git 操作需要再次确认', confirmationStep: 2 })
  })

  it('projects tool activity lifecycle and bounds retained progress and results', () => {
    let state = applyServerMessage(initialSessionState, { type: 'session_created', session_id: 's1', workspace_id: 'w1' })
    for (let index = 0; index < 25; index += 1) {
      state = applyServerMessage(state, { type: 'tool_started', session_id: 's1', tool: `tool-${index}`, sequence: index + 1 })
    }
    expect(state.toolActivities).toHaveLength(20)
    expect(state.toolActivities[0].tool).toBe('tool-5')

    state = applyServerMessage(state, { type: 'tool_progress', session_id: 's1', tool: 'tool-24', progress: 'working', sequence: 26 })
    expect(state.toolActivities[state.toolActivities.length - 1]).toMatchObject({ tool: 'tool-24', status: 'running', progress: 'working' })
    state = applyServerMessage(state, { type: 'tool_result', session_id: 's1', tool: 'tool-24', result: 'done', sequence: 27 })
    expect(state.toolActivities[state.toolActivities.length - 1]).toMatchObject({ tool: 'tool-24', status: 'completed', progress: 'working', result: 'done' })
  })

  it('uses snapshot as the reconnect source of truth', () => {
    const state = applyServerMessage({ ...initialSessionState, sessionId: 's1', messages: [{ role: 'user', text: 'stale' }] }, { type: 'session_snapshot', session_id: 's1', workspace_id: null, sequence: 1, pending_approval: { request_id: 'r1', tool: 'demo.tool', summary: 'confirm', confirmations_required: 1, confirmations: 0 }, pending_question: null, messages: [{ role: 'user', text: 'restored' }] })
    expect(state.messages).toEqual([{ role: 'user', text: 'restored' }])
    expect(state.approval).toEqual({ requestId: 'r1', tool: 'demo.tool', summary: 'confirm', confirmationStep: 1 })
  })

  it('projects file, git, settings, marketplace, and diff preview events', () => {
    let state = applyServerMessage(initialSessionState, { type: 'session_created', session_id: 's1', workspace_id: 'w1' })
    state = applyServerMessage(state, { type: 'files_listed', path: 'src', entries: ['main.tsx', 'style.css'], directories: [] })
    expect(state.files).toEqual({ path: 'src', entries: ['main.tsx', 'style.css'], directories: [] })

    state = applyServerMessage(state, { type: 'file_contents', path: 'src/main.tsx', contents: 'console.log("hello")' })
    expect(state.activeFile).toEqual({ path: 'src/main.tsx', contents: 'console.log("hello")' })

    state = applyServerMessage(state, { type: 'search_results', query: 'main', matches: ['src/main.tsx'] })
    expect(state.searchResults).toEqual({ query: 'main', matches: ['src/main.tsx'] })

    state = applyServerMessage(state, { type: 'git_status', branch: 'main', entries: [' M src/main.tsx'] })
    expect(state.gitStatus).toEqual({ branch: 'main', entries: [' M src/main.tsx'] })

    state = applyServerMessage(state, { type: 'settings', base_url: 'https://api.openai.com/v1', model: 'gpt-4o', has_api_key: true })
    expect(state.settings).toEqual({ baseUrl: 'https://api.openai.com/v1', model: 'gpt-4o', hasApiKey: true })

    state = applyServerMessage(state, { type: 'settings_updated', base_url: 'https://api.groq.com/openai/v1', model: 'llama-3.3-70b' })
    expect(state.settings).toEqual({ baseUrl: 'https://api.groq.com/openai/v1', model: 'llama-3.3-70b', hasApiKey: true })
    expect(state.status).toBe('设置已更新')

    state = applyServerMessage(state, { type: 'diff_preview', session_id: 's1', preview: { proposal_id: 'p1', path: 'file.txt', before: 'old', after: 'new' } })
    expect(state.diffPreview).toEqual({ proposal_id: 'p1', path: 'file.txt', before: 'old', after: 'new' })

    state = applyServerMessage(state, { type: 'diff_resolved', proposal_id: 'p1', action: 'accepted', sequence: 2 })
    expect(state.diffPreview).toBeUndefined()
    expect(state.status).toBe('Diff accepted')

    state = applyServerMessage(state, {
      type: 'marketplace_scan',
      catalog_loaded: true,
      entries: [{
        name: 'test-plugin',
        version: '1.0.0',
        description: 'a test plugin',
        category: 'tool',
        author: 'chaos',
        tags: ['code'],
        keywords: ['dev'],
        domains: ['editor'],
        homepage: null,
        relative_path: 'plugins/test',
        skill_count: 2,
        has_hooks: false,
        has_agents: true,
        has_mcp: false,
      }],
    })
    expect(state.marketplaceEntries).toHaveLength(1)
    expect(state.marketplaceEntries?.[0].name).toBe('test-plugin')

    state = applyServerMessage(state, { type: 'terminal_result', session_id: 's1', output: 'build succeeded', exit_code: 0 })
    expect(state.terminalResult).toEqual({ output: 'build succeeded', exit_code: 0 })
    expect(state.status).toBe('终端执行完成（退出码：0）')

    state = applyServerMessage(state, { type: 'git_mutation_result', session_id: 's1', operation: 'stage', result: 'staged 1 file' })
    expect(state.gitMutationResult).toEqual({ operation: 'stage', result: 'staged 1 file' })
    expect(state.status).toBe('Git stage 执行完成')

    state = applyServerMessage(state, { type: 'provider_validation', base_url: 'https://api.openai.com/v1', model: 'gpt-4o', reachable: false, error_code: 'network_not_attempted' })
    expect(state.providerValidation).toEqual({ baseUrl: 'https://api.openai.com/v1', model: 'gpt-4o', reachable: false, errorCode: 'network_not_attempted' })
    expect(state.status).toBe('Provider 验证未通：network_not_attempted')

    state = applyServerMessage(state, { type: 'tui_session_import', session_id: 'tui-1', cwd: '/app', title: 'Imported', message_count: 5, source_unchanged: true })
    expect(state.tuiImport).toEqual({ sessionId: 'tui-1', cwd: '/app', title: 'Imported', messageCount: 5, sourceUnchanged: true })

    state = applyServerMessage(state, { type: 'usage', session_id: 's1', input_tokens: 150, output_tokens: 320, sequence: 3 })
    expect(state.usage).toEqual({ inputTokens: 150, outputTokens: 320 })

    state = applyServerMessage(state, { type: 'file_written', session_id: 's1', path: 'README.md', bytes: 1024 })
    expect(state.status).toBe('文件已写入：README.md（1024 字节）')
  })
})

describe('attachment upload projection', () => {
  const validating = {
    ...initialSessionState,
    sessionId: 's1',
    upload: { filename: 'note.txt', byteLen: 100, sentBytes: 0, status: 'validating' as const },
  }

  it('walks validate, begin, slices, approval and the completed write', () => {
    let state = applyServerMessage(validating, { type: 'attachment_validated', filename: 'note.txt', byte_len: 100, content_type: 'text/plain' })
    expect(state.upload?.status).toBe('beginning')

    state = applyServerMessage(state, { type: 'attachment_started', session_id: 's1', upload_id: 'u1', filename: 'note.txt' })
    expect(state.upload).toEqual({ filename: 'note.txt', byteLen: 100, uploadId: 'u1', sentBytes: 0, status: 'uploading' })
    expect(state.status).toBe('正在上传 note.txt')

    state = applyServerMessage(state, { type: 'attachment_progress', upload_id: 'u1', received: 64 })
    expect(state.upload?.sentBytes).toBe(64)
    state = applyServerMessage(state, { type: 'attachment_progress', upload_id: 'u1', received: 100 })
    expect(state.upload?.sentBytes).toBe(100)

    state = applyServerMessage(state, { type: 'tool_approval_requested', session_id: 's1', request_id: 'r1', tool: 'workspace.attach_attachment', summary: '请求将已上传附件写入 workspace', sequence: 3 })
    expect(state.upload?.status).toBe('awaiting_approval')
    expect(state.approval?.tool).toBe('workspace.attach_attachment')

    state = applyServerMessage(state, { type: 'approval_resolved', session_id: 's1', request_id: 'r1', approved: true, sequence: 4 })
    expect(state.upload?.status).toBe('awaiting_approval')

    state = applyServerMessage(state, { type: 'attachment_completed', session_id: 's1', upload_id: 'u1', path: 'docs/note.txt', bytes: 100 })
    expect(state.upload).toEqual({ filename: 'note.txt', byteLen: 100, uploadId: 'u1', sentBytes: 100, status: 'done', path: 'docs/note.txt', bytes: 100 })
    expect(state.status).toBe('附件已写入 docs/note.txt（100 字节）')
  })

  it('keeps progress and cancellation tied to the upload they name', () => {
    const uploading = { ...validating, upload: { ...validating.upload, uploadId: 'u1', status: 'uploading' as const } }
    expect(applyServerMessage(uploading, { type: 'attachment_progress', upload_id: 'other', received: 99 })).toBe(uploading)
    expect(applyServerMessage(uploading, { type: 'attachment_cancelled', upload_id: 'other' })).toBe(uploading)
    const cancelled = applyServerMessage(uploading, { type: 'attachment_cancelled', upload_id: 'u1' })
    expect(cancelled.upload?.status).toBe('cancelled')
    expect(cancelled.status).toBe('上传已取消')
  })

  it('fails the upload when the host refuses a slice', () => {
    const uploading = { ...validating, upload: { ...validating.upload, uploadId: 'u1', status: 'uploading' as const } }
    const failed = applyServerMessage(uploading, { type: 'error', code: 'attachment_quota_exceeded', message: '附件超过声明大小' })
    expect(failed.upload?.status).toBe('failed')
    expect(failed.upload?.error).toBe('附件超过声明大小')
    expect(failed.status).toBe('上传失败')
  })

  it('only reads a shared path error as an upload failure after the bytes are staged', () => {
    const uploading = { ...validating, upload: { ...validating.upload, uploadId: 'u1', status: 'uploading' as const } }
    expect(applyServerMessage(validating, { type: 'error', code: 'path_escape', message: '路径越界' }).upload).toBe(validating.upload)
    expect(applyServerMessage(uploading, { type: 'error', code: 'path_escape', message: '路径越界' }).upload?.status).toBe('failed')
  })

  it('leaves an unrelated error off the upload', () => {
    const uploading = { ...validating, upload: { ...validating.upload, uploadId: 'u1', status: 'uploading' as const } }
    const state = applyServerMessage({ ...uploading, busy: true }, { type: 'error', code: 'git_failed', message: 'git 失败' })
    expect(state.upload?.status).toBe('uploading')
    expect(state.busy).toBe(false)
  })

  it('treats a rejected approval as a cancelled upload', () => {
    const awaiting = { ...validating, upload: { ...validating.upload, uploadId: 'u1', status: 'awaiting_approval' as const } }
    const state = applyServerMessage(awaiting, { type: 'approval_resolved', session_id: 's1', request_id: 'r1', approved: false, sequence: 5 })
    expect(state.upload?.status).toBe('cancelled')
  })

  it('ignores attachment events belonging to another session', () => {
    expect(applyServerMessage(validating, { type: 'attachment_started', session_id: 'other', upload_id: 'u1', filename: 'note.txt' })).toBe(validating)
  })
})

const reported: HostInfo = {
  host_version: '0.4.0',
  protocol_version: 9,
  bind_addr: '127.0.0.1:8787',
  state_backend: 'sqlite',
  safe_web_mode: true,
  workspace_root: null,
  token_required: false,
  public_origin: null,
  preview_proxy: 'disabled',
  preview_ports: [],
  update_mode: 'external',
  safe_mode_refusals: [{ message: 'approve', capability: '批准待审操作' }],
}

describe('host self-report', () => {
  it('stores the report even before a session exists', () => {
    const state = applyServerMessage(initialSessionState, { type: 'host_info', info: reported })
    expect(state.hostInfo).toEqual(reported)
    expect(state.sessionId).toBeUndefined()
  })

  it('replaces the previous report so the newest answer wins', () => {
    const first = applyServerMessage(initialSessionState, { type: 'host_info', info: { ...reported, bind_addr: '127.0.0.1:1111' } })
    const second = applyServerMessage(first, { type: 'host_info', info: { ...reported, bind_addr: '127.0.0.1:2222' } })
    expect(second.hostInfo?.bind_addr).toBe('127.0.0.1:2222')
  })

  it('leaves the report alone for every other host message', () => {
    const reportedState = applyServerMessage(initialSessionState, { type: 'host_info', info: reported })
    const withSession = applyServerMessage(reportedState, { type: 'session_created', session_id: 's1', workspace_id: 'w1' })
    expect(withSession.hostInfo).toEqual(reported)
    expect(applyServerMessage(withSession, { type: 'settings', base_url: null, model: 'gpt-4o', has_api_key: true }).hostInfo).toEqual(reported)
  })

  it('starts with no report so the panel cannot describe a host it never asked', () => {
    expect(initialSessionState.hostInfo).toBeUndefined()
  })
})
