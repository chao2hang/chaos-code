import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { applyServerMessage, fileChangeAffectsVisibleDirectory, initialSessionState, sessionLossRecoveryMessage, workspaceReconnectMessage, type ServerMessage, type SessionState } from './session'
import { selectWorkspaceSession } from './workspace-ui'
import { webSocketUrl } from './transport'
import { getComposerSuggestions, initialComposerHistory, moveSuggestionIndex, navigatePromptHistory, recordPrompt, shouldSubmitOnKey, type ComposerSuggestion } from './composer'
import { defaultLayoutState, loadLayoutState, saveLayoutState, type LayoutState } from './layout'
import { attachmentChunkMessages, beginAttachmentMessage, cancelAttachmentMessage, describeUpload, finalizeAttachmentMessage, MAX_ATTACHMENT_BYTES, validateAttachmentMessage, type AttachmentSource } from './attachments'
import './style.css'

type Tab = 'chat' | 'files' | 'git' | 'terminal' | 'settings' | 'marketplace' | 'diff'

function safeMarkdownHref(href: string | undefined): { href: string; external: boolean } | undefined {
  if (!href) return undefined
  if (href.startsWith('#')) return { href, external: false }
  if (/^https?:\/\//i.test(href)) return { href, external: true }
  if (/^mailto:/i.test(href)) return { href, external: false }
  return undefined
}

function ZCodeWhaleLogo() {
  return (
    <svg width="24" height="18" viewBox="0 0 24 18" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
      <path
        d="M22.403 0.567C22.145 0.477 22.068 0.718 21.939 0.85C21.895 0.893 21.86 0.947 21.824 0.997C21.515 1.421 21.13 1.721 20.591 1.77C19.829 1.867 19.221 2.244 18.712 2.958C18.535 2.227 18.116 1.839 17.516 1.626C17.203 1.506 16.887 1.379 16.663 1.064C16.508 0.839 16.462 0.581 16.383 0.329C16.332 0.176 16.283 0.02 16.121 -0.002C15.944 -0.029 15.875 0.133 15.805 0.269C15.52 0.822 15.408 1.43 15.42 2.046C15.449 3.432 16.031 4.532 17.202 5.274C17.337 5.356 17.374 5.445 17.335 5.582C17.261 5.862 17.169 6.134 17.086 6.413C17.032 6.59 16.952 6.63 16.764 6.558C16.118 6.301 15.562 5.909 15.074 5.433C14.248 4.633 13.5 3.751 12.568 3.06C12.349 2.898 12.13 2.748 11.903 2.605C10.952 1.682 12.028 0.923 12.277 0.833C12.537 0.739 12.367 0.416 11.526 0.42C10.684 0.424 9.914 0.706 8.933 1.081C8.789 1.138 8.638 1.179 8.484 1.213C7.593 1.044 6.668 1.006 5.702 1.115C3.883 1.318 2.43 2.178 1.362 3.646C0.079 5.41 -0.223 7.415 0.147 9.506C0.535 11.71 1.66 13.535 3.389 14.962C5.181 16.441 7.246 17.166 9.601 17.027C11.032 16.944 12.624 16.753 14.421 15.232C14.874 15.458 15.35 15.548 16.138 15.615C16.746 15.672 17.331 15.585 17.784 15.491C18.493 15.341 18.444 14.684 18.188 14.564C16.108 13.595 16.565 13.989 16.15 13.67C17.206 12.42 18.82 10.198 19.363 7.086C19.421 6.709 19.484 6.171 19.469 5.866C19.458 5.681 19.493 5.604 19.681 5.556C20.199 5.412 20.691 5.172 21.125 4.806C22.366 3.824 22.758 2.554 22.708 1.1C22.7 0.878 22.649 0.654 22.403 0.567Z"
        fill="currentColor"
      />
    </svg>
  )
}

function App() {
  const [session, setSession] = useState(initialSessionState)
  const [activeTab, setActiveTab] = useState<Tab>('chat')
  const [prompt, setPrompt] = useState('')
  const [promptHistory, setPromptHistory] = useState(initialComposerHistory)
  const [layout, setLayout] = useState<LayoutState>(() => {
    if (typeof window === 'undefined') return defaultLayoutState
    try { return loadLayoutState(window.localStorage) } catch { return defaultLayoutState }
  })

  // IDE view state
  const [dirPath, setDirPath] = useState('.')
  const [fileSearchQuery, setFileSearchQuery] = useState('')
  const [editingFileContent, setEditingFileContent] = useState('')
  const [gitOp, setGitOp] = useState<'stage' | 'unstage' | 'commit' | 'checkout_branch' | 'discard'>('stage')
  const [gitArg, setGitArg] = useState('.')
  const [terminalCmd, setTerminalCmd] = useState('git status')
  const [settingsBaseUrl, setSettingsBaseUrl] = useState('')
  const [settingsModel, setSettingsModel] = useState('')
  const [tuiSessionId, setTuiSessionId] = useState('')
  const [tuiRoot, setTuiRoot] = useState('.')
  const [marketplaceRoot, setMarketplaceRoot] = useState('.')
  const [manualProposalId, setManualProposalId] = useState('')

  // Autocomplete suggestions
  const [suggestions, setSuggestions] = useState<ComposerSuggestion[]>([])
  const [activeSuggestionIndex, setActiveSuggestionIndex] = useState(0)
  const [workspaceWriteError, setWorkspaceWriteError] = useState<string>()
  const [workspaceWriteState, setWorkspaceWriteState] = useState<'idle' | 'pending' | 'saved'>('idle')
  const [workspaceWriteApprovalId, setWorkspaceWriteApprovalId] = useState<string>()
  const [uploadPick, setUploadPick] = useState<File | null>(null)
  const [uploadTargetPath, setUploadTargetPath] = useState('')
  const [uploadPickError, setUploadPickError] = useState<string>()

  const socket = useRef<WebSocket | null>(null)
  const timelineRef = useRef<HTMLElement | null>(null)
  const timelineAnchorRef = useRef<{ atBottom: boolean; scrollTop: number } | null>({ atBottom: true, scrollTop: 0 })
  const timelineWorkspaceIdRef = useRef(session.activeWorkspaceId)
  const timelineActiveTabRef = useRef(activeTab)
  const timelineAnchorEpochRef = useRef(0)
  const sessionStateRef = useRef(session)
  const reconnectTimer = useRef<number | undefined>(undefined)
  const workspaceWriteApprovalIdRef = useRef<string | undefined>(undefined)
  // The bytes stay out of SessionState on purpose: a 10 MiB file copied on every
  // reducer step would dominate the timeline's update cost. It is dropped once
  // the slices are on the wire.
  const uploadSourceRef = useRef<(AttachmentSource & { targetPath: string }) | null>(null)

  useEffect(() => { sessionStateRef.current = session }, [session])

  // The message handler reads `sessionStateRef`, and a functional `setSession`
  // only reaches that ref after the next render. A host reply that arrives in
  // between would then be applied to a stale state, and its own write would
  // clobber the queued one, so every write goes through here instead.
  const updateSession = useCallback((update: (current: SessionState) => SessionState) => {
    const next = update(sessionStateRef.current)
    sessionStateRef.current = next
    setSession(next)
  }, [])
  useLayoutEffect(() => {
    timelineAnchorRef.current = {
      atBottom: true,
      scrollTop: timelineRef.current?.scrollTop ?? 0,
    }
  }, [session.sessionId])
  useLayoutEffect(() => {
    const timeline = timelineRef.current
    if (timelineWorkspaceIdRef.current !== session.activeWorkspaceId) {
      timelineWorkspaceIdRef.current = session.activeWorkspaceId
      timelineAnchorRef.current = { atBottom: true, scrollTop: 0 }
      if (timelineActiveTabRef.current === 'chat' && timeline) timeline.scrollTop = 0
    }
    if (timelineActiveTabRef.current !== activeTab) {
      timelineActiveTabRef.current = activeTab
      if (activeTab === 'chat') timelineAnchorRef.current = { atBottom: true, scrollTop: timeline?.scrollTop ?? 0 }
    }
    const anchor = timelineAnchorRef.current
    if (!timeline || !anchor) return
    timeline.style.scrollBehavior = 'auto'
    timeline.scrollTop = anchor.atBottom
      ? timeline.scrollHeight
      : anchor.scrollTop
    timeline.style.removeProperty('scroll-behavior')
    timelineAnchorRef.current = null
  }, [activeTab, session.activeWorkspaceId, session.messages, session.toolActivities, session.approval, session.question])
  useEffect(() => {
    if (typeof window === 'undefined') return
    try { saveLayoutState(window.localStorage, layout) } catch { updateSession((current) => ({ ...current, status: '布局未保存（本地存储不可用）' })) }
  }, [layout])
  useEffect(() => {
    document.documentElement.dataset.theme = layout.theme
    if (layout.theme === 'dark') {
      document.body.setAttribute('data-ds-dark-theme', '')
    } else {
      document.body.removeAttribute('data-ds-dark-theme')
    }
    document.documentElement.style.setProperty('--sidebar-width', `${layout.sidebarWidth}px`)
    document.documentElement.style.setProperty('--composer-height', `${layout.composerHeight}px`)
  }, [layout])

  // Sync settings when loaded from server
  useEffect(() => {
    if (session.settings) {
      if (session.settings.baseUrl) setSettingsBaseUrl(session.settings.baseUrl)
      if (session.settings.model) setSettingsModel(session.settings.model)
    }
  }, [session.settings])

  // Sync editing file content when active file changes
  useEffect(() => {
    if (session.activeFile) {
      setEditingFileContent(session.activeFile.contents)
    }
  }, [session.activeFile])

  // Sync composer autocomplete
  useEffect(() => {
    const entries = [...new Set([
      ...(session.files?.entries ?? []),
      ...(session.searchResults?.matches ?? []),
    ])]
    const list = getComposerSuggestions(prompt, entries)
    setSuggestions(list)
    setActiveSuggestionIndex(0)
  }, [prompt, session.files?.entries, session.searchResults?.matches])

  const send = useCallback((message: object) => {
    if (socket.current?.readyState === WebSocket.OPEN) socket.current.send(JSON.stringify(message))
  }, [])

  const connect = useCallback(() => {
    const ws = new WebSocket(webSocketUrl(location, import.meta.env.VITE_CHAOS_E2E_BACKEND_PORT || import.meta.env.VITE_CHAOS_WS_PORT || '8787', import.meta.env.VITE_CHAOS_E2E_ORIGIN_PATH))
    socket.current = ws
    ws.onopen = () => {
      updateSession((current) => ({ ...current, status: '已连接' }))
      send({ type: 'list_workspaces', client_msg_id: crypto.randomUUID() })
      send(workspaceReconnectMessage(sessionStateRef.current))
    }
    ws.onmessage = (event) => {
      const message = JSON.parse(event.data) as ServerMessage
      if (message.type === 'session_created') (window as Window & { __chaosDevWorkspaceCreated?: boolean }).__chaosDevWorkspaceCreated = true
      const previous = sessionStateRef.current
      const next = applyServerMessage(previous, message)
      sessionStateRef.current = next
      setSession(next)

      if (message.type === 'tool_approval_requested' && message.tool === 'workspace.write_file') {
        workspaceWriteApprovalIdRef.current = message.request_id
        setWorkspaceWriteApprovalId(message.request_id)
        setWorkspaceWriteState('pending')
      }
      if (message.type === 'attachment_validated' && uploadSourceRef.current) {
        const sessionId = sessionStateRef.current.sessionId
        if (sessionId) send(beginAttachmentMessage(crypto.randomUUID(), sessionId, uploadSourceRef.current))
      }
      if (message.type === 'attachment_started' && uploadSourceRef.current) pumpUpload(message.upload_id)
      if (fileChangeAffectsVisibleDirectory(sessionStateRef.current, message)) {
        refreshFiles(sessionStateRef.current.files?.path ?? '.')
      }
      if (message.type === 'file_written') {
        setWorkspaceWriteError(undefined)
        workspaceWriteApprovalIdRef.current = undefined
        setWorkspaceWriteApprovalId(undefined)
        setWorkspaceWriteState('saved')
      }
      if (message.type === 'error' && ['path_escape', 'write_failed', 'file_too_large'].includes(message.code)) {
        setWorkspaceWriteError(message.message)
        setWorkspaceWriteState('idle')
      }
      if (message.type === 'error' && ['workspace_unavailable', 'approval_not_found'].includes(message.code) && workspaceWriteApprovalIdRef.current) {
        workspaceWriteApprovalIdRef.current = undefined
        setWorkspaceWriteApprovalId(undefined)
        setWorkspaceWriteState('idle')
      }
      const recovery = sessionLossRecoveryMessage(sessionStateRef.current, message)
      if (recovery) send(recovery)
      if (message.type === 'approval_resolved' && message.request_id === workspaceWriteApprovalIdRef.current) {
        workspaceWriteApprovalIdRef.current = undefined
        setWorkspaceWriteApprovalId(undefined)
        if (!message.approved) setWorkspaceWriteState('idle')
      }
    }
    ws.onerror = () => updateSession((current) => ({ ...current, status: '连接错误' }))
    ws.onclose = () => { updateSession((current) => ({ ...current, status: '连接断开，正在重连' })); reconnectTimer.current = window.setTimeout(connect, 500) }
  }, [send, updateSession])

  useEffect(() => { connect(); return () => { if (reconnectTimer.current) window.clearTimeout(reconnectTimer.current); socket.current?.close() } }, [connect])

  function createWorkspace() {
    const name = window.prompt('工作区名称')?.trim()
    if (!name) return
    updateSession((current) => ({ ...current, messages: [], approval: undefined, question: undefined, busy: false, status: '正在创建工作区' }))
    send({ type: 'create_workspace', client_msg_id: crypto.randomUUID(), name })
  }
  function switchWorkspace(workspaceId: string) {
    updateSession((current) => selectWorkspaceSession(current, workspaceId))
    send({ type: 'switch_workspace', client_msg_id: crypto.randomUUID(), workspace_id: workspaceId })
  }
  function archiveWorkspace(workspaceId: string) {
    updateSession((current) => current.activeWorkspaceId === workspaceId
      ? { ...current, messages: [], approval: undefined, question: undefined, busy: false, status: '正在归档工作区' }
      : current)
    send({ type: 'archive_workspace', client_msg_id: crypto.randomUUID(), workspace_id: workspaceId })
  }

  function submit() {
    const value = prompt.trim(); if (!value || session.busy || !session.sessionId) return
    const timeline = timelineRef.current
    if (timeline) {
      timelineAnchorRef.current = {
        atBottom: timeline.scrollHeight - timeline.clientHeight - timeline.scrollTop <= 48,
        scrollTop: timeline.scrollTop,
      }
    }
    updateSession((current) => ({ ...current, messages: [...current.messages, { role: 'user', text: value }, { role: 'assistant', text: '' }], busy: true }))
    setPromptHistory((current) => recordPrompt(current, value))
    setPrompt('')
    setSuggestions([])
    send({ type: 'submit', client_msg_id: crypto.randomUUID(), session_id: session.sessionId, prompt: value })
  }
  function cancel() { if (session.sessionId) send({ type: 'cancel', client_msg_id: crypto.randomUUID(), session_id: session.sessionId }) }
  function resolveApproval(approved: boolean) { if (!session.approval) return; send(approved ? { type: 'approve', client_msg_id: crypto.randomUUID(), request_id: session.approval.requestId } : { type: 'reject', client_msg_id: crypto.randomUUID(), request_id: session.approval.requestId, reason: '用户拒绝' }); updateSession((current) => ({ ...current, approval: undefined })) }
  function answerQuestion(answer: string) { if (!session.question || !answer.trim()) return; send({ type: 'respond_question', client_msg_id: crypto.randomUUID(), question_id: session.question.questionId, answer }); updateSession((current) => ({ ...current, question: undefined })) }

  function handleComposerKeyDown(event: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (event.nativeEvent.isComposing) return
    if (suggestions.length > 0 && (event.key === 'ArrowDown' || event.key === 'ArrowUp')) {
      event.preventDefault()
      setActiveSuggestionIndex((current) => moveSuggestionIndex(current, suggestions.length, event.key === 'ArrowDown' ? 1 : -1))
      return
    }
    if (suggestions.length > 0 && event.key === 'Escape') {
      event.preventDefault()
      setSuggestions([])
      return
    }
    if (suggestions.length > 0 && event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault()
      const selected = suggestions[activeSuggestionIndex]
      if (selected) {
        setPrompt(selected.insert)
        setSuggestions([])
      }
      return
    }
    if (event.key === 'ArrowUp' && !event.shiftKey && event.currentTarget.selectionStart === 0) {
      event.preventDefault()
      const result = navigatePromptHistory(promptHistory, -1, prompt, event.nativeEvent.isComposing)
      setPromptHistory(result.history)
      setPrompt(result.value)
      return
    }
    if (event.key === 'ArrowDown' && !event.shiftKey && event.currentTarget.selectionEnd === event.currentTarget.value.length) {
      event.preventDefault()
      const result = navigatePromptHistory(promptHistory, 1, prompt, event.nativeEvent.isComposing)
      setPromptHistory(result.history)
      setPrompt(result.value)
      return
    }
    if (shouldSubmitOnKey(event.nativeEvent)) {
      event.preventDefault()
      submit()
    }
  }

  const togglePanel = () => setLayout((current) => ({ ...current, panelOpen: !current.panelOpen }))
  const cycleTheme = () => setLayout((current) => ({ ...current, theme: current.theme === 'dark' ? 'light' : current.theme === 'light' ? 'system' : 'dark' }))

  // IDE Actions
  function refreshFiles(path = dirPath) {
    updateSession((current) => ({ ...current, filesLoading: true, filesError: undefined, files: undefined }))
    send({ type: 'list_files', client_msg_id: crypto.randomUUID(), relative_path: path })
  }
  function openFile(path: string) {
    updateSession((current) => ({ ...current, fileLoading: true, fileError: undefined, activeFile: undefined }))
    send({ type: 'read_file', client_msg_id: crypto.randomUUID(), relative_path: path })
  }
  function searchFiles() {
    if (!fileSearchQuery.trim()) return
    updateSession((current) => ({ ...current, searchLoading: true, searchError: undefined, searchResults: undefined }))
    send({ type: 'search_files', client_msg_id: crypto.randomUUID(), query: fileSearchQuery.trim() })
  }

  // The transfer is driven by the host's own answers (see ws.onmessage): validate
  // → begin → attachment_started carries the upload_id → slices → finalize. The
  // write itself is the approval the host asks for after finalize.
  async function startUpload() {
    const file = uploadPick
    if (!file) { setUploadPickError('请先选择文件'); return }
    if (!sessionStateRef.current.sessionId) { setUploadPickError('请先创建会话'); return }
    if (file.size === 0) { setUploadPickError('空文件不能作为附件上传'); return }
    if (file.size > MAX_ATTACHMENT_BYTES) { setUploadPickError(`附件不能超过 ${Math.floor(MAX_ATTACHMENT_BYTES / (1024 * 1024))} MiB`); return }
    setUploadPickError(undefined)
    const bytes = new Uint8Array(await file.arrayBuffer())
    uploadSourceRef.current = { filename: file.name, contentType: file.type, bytes, targetPath: uploadTargetPath.trim() || file.name }
    updateSession((current) => ({ ...current, upload: { filename: file.name, byteLen: bytes.length, sentBytes: 0, status: 'validating' }, status: '正在校验附件' }))
    send(validateAttachmentMessage(crypto.randomUUID(), uploadSourceRef.current))
  }

  function pumpUpload(uploadId: string) {
    const source = uploadSourceRef.current
    if (!source) return
    for (const message of attachmentChunkMessages(uploadId, source.bytes, () => crypto.randomUUID())) send(message)
    send(finalizeAttachmentMessage(crypto.randomUUID(), uploadId, source.targetPath))
    uploadSourceRef.current = null
  }

  function cancelUpload() {
    const uploadId = sessionStateRef.current.upload?.uploadId
    uploadSourceRef.current = null
    if (uploadId) { send(cancelAttachmentMessage(crypto.randomUUID(), uploadId)); return }
    updateSession((current) => ({ ...current, upload: undefined, status: '上传已取消' }))
  }
  function openDirectory(entry: string) {
    const base = session.files?.path ?? dirPath
    const path = base === '.' ? entry : `${base.replace(/\/$/, '')}/${entry}`
    setDirPath(path)
    refreshFiles(path)
  }
  function parentDirectory() {
    const current = session.files?.path ?? dirPath
    const parts = current.split('/').filter((part) => part && part !== '.')
    parts.pop()
    const parent = parts.length ? parts.join('/') : '.'
    setDirPath(parent)
    refreshFiles(parent)
  }
  function proposeFileWrite() {
    if (!session.sessionId || !session.activeFile || workspaceWriteState === 'pending') return
    setWorkspaceWriteError(undefined)
    setWorkspaceWriteState('pending')
    send({
      type: 'propose_file_write',
      client_msg_id: crypto.randomUUID(),
      session_id: session.sessionId,
      relative_path: session.activeFile.path,
      contents: editingFileContent,
    })
  }

  function refreshGitStatus() {
    updateSession((current) => ({ ...current, gitLoading: true, gitError: undefined }))
    send({ type: 'get_git_status', client_msg_id: crypto.randomUUID() })
  }
  function executeGitMutation() {
    if (!session.sessionId || !gitArg.trim()) return
    updateSession((current) => ({ ...current, gitLoading: true, gitError: undefined }))
    send({
      type: 'propose_git_mutation',
      client_msg_id: crypto.randomUUID(),
      session_id: session.sessionId,
      operation: gitOp,
      argument: gitArg.trim(),
    })
  }

  function executeTerminalCommand() {
    if (!session.sessionId || !terminalCmd.trim()) return
    updateSession((current) => ({ ...current, terminalLoading: true, terminalError: undefined }))
    send({
      type: 'propose_terminal',
      client_msg_id: crypto.randomUUID(),
      session_id: session.sessionId,
      command: terminalCmd.trim(),
    })
  }

  function fetchSettings() {
    send({ type: 'get_settings', client_msg_id: crypto.randomUUID() })
  }
  function saveSettings() {
    send({
      type: 'update_settings',
      client_msg_id: crypto.randomUUID(),
      base_url: settingsBaseUrl.trim() || null,
      model: settingsModel.trim() || null,
    })
  }
  function validateProvider() {
    send({
      type: 'validate_provider',
      client_msg_id: crypto.randomUUID(),
      base_url: settingsBaseUrl.trim(),
      model: settingsModel.trim(),
    })
  }
  function importTuiSession() {
    if (!tuiSessionId.trim()) return
    send({
      type: 'import_tui_session',
      client_msg_id: crypto.randomUUID(),
      root: tuiRoot.trim() || '.',
      session_id: tuiSessionId.trim(),
    })
  }

  function scanMarketplace() {
    send({ type: 'scan_marketplace', client_msg_id: crypto.randomUUID(), root: marketplaceRoot.trim() || '.' })
  }
  function previewDiff(proposalId: string) {
    if (!proposalId.trim()) return
    send({ type: 'preview_diff', client_msg_id: crypto.randomUUID(), proposal_id: proposalId.trim() })
  }
  function acceptDiff(proposalId: string) {
    send({ type: 'accept_diff', client_msg_id: crypto.randomUUID(), proposal_id: proposalId })
  }
  function rollbackDiff(proposalId: string) {
    send({ type: 'rollback_diff', client_msg_id: crypto.randomUUID(), proposal_id: proposalId })
  }

  const activeWorkspace = session.workspaces.find((w) => w.id === session.activeWorkspaceId)
  const activeWorkspaceName = activeWorkspace ? activeWorkspace.name : '默认工作区'
  const isRightbarOpen = activeTab !== 'chat' || Boolean(session.activeFile)
  const rightbarWidth = isRightbarOpen ? 420 : 0

  return (
    <main
      className="shell"
      data-testid="app-shell"
      data-theme={layout.theme}
      data-ds-dark-theme={layout.theme === 'dark' ? '' : undefined}
      data-sidebar-collapsed={!layout.panelOpen ? 'true' : 'false'}
      style={{
        ['--sidebar-width' as string]: `${layout.panelOpen ? layout.sidebarWidth : 0}px`,
        ['--rightbar-width' as string]: `${rightbarWidth}px`,
      }}
    >
      {/* =========================================================
          1. Left Column: ZCode Workspace & Session Sidebar
          ========================================================= */}
      <aside
        className="sidebar-col"
        data-shell-column="sidebar"
        aria-label="工作区侧边栏"
        style={{ display: layout.panelOpen ? 'flex' : 'none' }}
      >
        <div className="sidebar-header">
          <div className="brand-identity" onClick={() => setActiveTab('chat')}>
            <span className="brand-icon">
              <ZCodeWhaleLogo />
            </span>
            <h1 className="brand-text">Chaos</h1>
          </div>
          <button
            type="button"
            className="sidebar-toggle-btn"
            aria-label="收起侧边栏"
            title="收起侧边栏"
            onClick={togglePanel}
          >
            ◧
          </button>
        </div>

        <div className="sidebar-new-session">
          <button type="button" className="btn-new-workspace" onClick={createWorkspace}>
            + 新工作区
          </button>
        </div>

        <div className="sidebar-content">
          <div className="sidebar-section-title">工作区与会话</div>
          <ul className="workspace-list">
            {session.workspaces
              .filter((workspace) => !workspace.archived)
              .map((workspace) => {
                const isActive = workspace.id === session.activeWorkspaceId
                return (
                  <li key={workspace.id} className="workspace-item-wrap">
                    <button
                      type="button"
                      data-testid={`workspace-${workspace.id}`}
                      className={`workspace-item ${isActive ? 'active' : ''}`}
                      onClick={() => switchWorkspace(workspace.id)}
                    >
                      <span aria-hidden="true">{isActive ? '📁' : '📂'}</span>
                      <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                        {workspace.name}
                      </span>
                    </button>
                    <button
                      type="button"
                      className="btn-archive"
                      aria-label={`归档工作区 ${workspace.name}`}
                      title="归档此工作区"
                      onClick={(e) => {
                        e.stopPropagation()
                        archiveWorkspace(workspace.id)
                      }}
                    >
                      ✕
                    </button>
                  </li>
                )
              })}
          </ul>
        </div>

        <div className="sidebar-footer">
          <div className="sidebar-footer-row">
            <button type="button" className="sidebar-footer-btn" onClick={cycleTheme}>
              主题：{layout.theme}
            </button>
            <label className="sidebar-width-control">
              面板宽度
              <input
                type="number"
                aria-label="面板宽度"
                className="sidebar-width-input"
                value={layout.sidebarWidth}
                onChange={(e) => {
                  const val = Number(e.target.value) || 280
                  setLayout((current) => ({ ...current, sidebarWidth: Math.max(200, Math.min(600, val)) }))
                }}
              />
            </label>
          </div>
          <div className="sidebar-footer-row">
            <span data-testid="session-status" role="status" aria-live="polite" className="session-status-badge">
              {session.status}
            </span>
          </div>
        </div>
      </aside>

      {/* =========================================================
          2. Center Column: ZCode Conversation Stream & InputBar
          ========================================================= */}
      <div className={`center-col${suggestions.length > 0 ? ' has-composer-suggestions' : ''}`} data-shell-column="center">
        {/* Top Conversation Header */}
        <header className="conversation-header">
          <div className="header-left">
            {!layout.panelOpen && (
              <button
                type="button"
                className="sidebar-toggle-btn"
                aria-label="展开侧边栏"
                title="展开侧边栏"
                onClick={togglePanel}
              >
                ◨
              </button>
            )}
            <div className="header-breadcrumbs">
              <strong>{activeWorkspaceName}</strong>
              <span>/</span>
              <span className="header-branch-pill">🌿 {session.gitStatus?.branch ?? 'main'}</span>
              <span>/</span>
              <span>{session.sessionId ? session.sessionId.slice(0, 8) : '新会话'}</span>
            </div>
          </div>

          <div className="header-right-tools">
            <button
              type="button"
              className={`header-tab-btn ${activeTab === 'chat' && !session.activeFile ? 'active' : ''}`}
              onClick={() => {
                setActiveTab('chat')
                updateSession((c) => ({ ...c, activeFile: undefined }))
              }}
            >
              💬 对话
            </button>
            <button
              type="button"
              className={`header-tab-btn ${activeTab === 'files' ? 'active' : ''}`}
              onClick={() => {
                const next = activeTab === 'files' ? 'chat' : 'files'
                setActiveTab(next)
                if (next === 'files') refreshFiles(dirPath)
              }}
            >
              📁 文件
            </button>
            <button
              type="button"
              className={`header-tab-btn ${activeTab === 'git' ? 'active' : ''}`}
              onClick={() => {
                const next = activeTab === 'git' ? 'chat' : 'git'
                setActiveTab(next)
                if (next === 'git') refreshGitStatus()
              }}
            >
              🌿 Git
            </button>
            <button
              type="button"
              className={`header-tab-btn ${activeTab === 'terminal' ? 'active' : ''}`}
              onClick={() => {
                const next = activeTab === 'terminal' ? 'chat' : 'terminal'
                setActiveTab(next)
              }}
            >
              💻 终端
            </button>
            <button
              type="button"
              className={`header-tab-btn ${activeTab === 'settings' ? 'active' : ''}`}
              onClick={() => {
                const next = activeTab === 'settings' ? 'chat' : 'settings'
                setActiveTab(next)
                if (next === 'settings') fetchSettings()
              }}
            >
              ⚙️ 设置
            </button>
            <button
              type="button"
              className={`header-tab-btn ${activeTab === 'marketplace' ? 'active' : ''}`}
              onClick={() => {
                const next = activeTab === 'marketplace' ? 'chat' : 'marketplace'
                setActiveTab(next)
                if (next === 'marketplace') scanMarketplace()
              }}
            >
              🧩 插件
            </button>
            <button
              type="button"
              className={`header-tab-btn ${activeTab === 'diff' ? 'active' : ''}`}
              onClick={() => {
                const next = activeTab === 'diff' ? 'chat' : 'diff'
                setActiveTab(next)
              }}
            >
              🔍 差异{session.diffPreview ? ' (1)' : ''}
            </button>
          </div>
        </header>

        {/* Diff Banner Notification */}
        {session.diffPreview && (
          <div className="diff-alert" role="alert">
            <span>待审查差异提案：<code>{session.diffPreview.path}</code></span>
            <div style={{ display: 'flex', gap: '8px' }}>
              <button type="button" onClick={() => acceptDiff(session.diffPreview!.proposal_id)}>接受</button>
              <button type="button" onClick={() => rollbackDiff(session.diffPreview!.proposal_id)}>回滚</button>
            </div>
          </div>
        )}

        {/* Conversation Stream Timeline */}
        <section
          ref={timelineRef}
          className="timeline"
          aria-label="会话时间线"
          data-testid="session-timeline"
          tabIndex={0}
          onScroll={(event) => {
            const timeline = event.currentTarget
            timelineAnchorRef.current = {
              atBottom: timeline.scrollHeight - timeline.clientHeight - timeline.scrollTop <= 48,
              scrollTop: timeline.scrollTop,
            }
          }}
        >
          <div className="timeline-content-centered">
            {session.messages.length === 0 && (
              <div className="empty-hero">
                <div className="hero-fish-wrap">
                  <ZCodeWhaleLogo />
                </div>
                <h2 className="hero-title">有什么可以帮你的？</h2>
                <p className="empty">创建会话后，在下方输入 Prompt。</p>
                <div className="hero-workspace-chip">
                  📁 <span>{activeWorkspaceName}</span>
                </div>
                <div className="hero-starters">
                  <button type="button" className="hero-starter-card" onClick={() => setPrompt('/explain ')}>
                    💡 /explain 解释代码
                  </button>
                  <button type="button" className="hero-starter-card" onClick={() => setPrompt('/review ')}>
                    🔍 /review 代码审查
                  </button>
                  <button type="button" className="hero-starter-card" onClick={() => setPrompt('/test ')}>
                    🧪 /test 生成测试
                  </button>
                </div>
              </div>
            )}

            {session.messages.map((message, index) => {
              const isUser = message.role === 'user'
              return (
                <article className={isUser ? 'user' : 'assistant'} key={index}>
                  <small>{isUser ? '你' : 'Chaos'}</small>
                  {isUser ? (
                    <div className="user-bubble markdown-body">
                      <ReactMarkdown
                        remarkPlugins={[remarkGfm]}
                        components={{
                          a: ({ href, children }) => {
                            const safe = safeMarkdownHref(href)
                            return safe ? (
                              <a href={safe.href} target={safe.external ? '_blank' : undefined} rel={safe.external ? 'noopener noreferrer' : undefined}>
                                {children}
                              </a>
                            ) : (
                              <span>{children}</span>
                            )
                          },
                          img: ({ alt }) => <span>{alt}</span>,
                        }}
                      >
                        {message.text}
                      </ReactMarkdown>
                    </div>
                  ) : (
                    <>
                      {message.text && (
                        <details className="reasoning-row" open>
                          <summary className="reasoning-summary">
                            <span>🧠 思考过程</span>
                            <span>·</span>
                            <small>{session.busy ? '正在深度思考…' : '已完成推理'}</small>
                          </summary>
                          <div className="reasoning-body">
                            已分析工作区上下文，规划执行步骤。
                          </div>
                        </details>
                      )}
                      <div className="markdown-body">
                        <ReactMarkdown
                          remarkPlugins={[remarkGfm]}
                          components={{
                            a: ({ href, children }) => {
                              const safe = safeMarkdownHref(href)
                              return safe ? (
                                <a href={safe.href} target={safe.external ? '_blank' : undefined} rel={safe.external ? 'noopener noreferrer' : undefined}>
                                  {children}
                                </a>
                              ) : (
                                <span>{children}</span>
                              )
                            },
                            img: ({ alt }) => <span>{alt}</span>,
                          }}
                        >
                          {message.text || '正在生成…'}
                        </ReactMarkdown>
                      </div>
                    </>
                  )}
                </article>
              )
            })}

            {session.toolActivities.length > 0 && (
              <ol className="tool-activity-list" aria-label="工具执行活动">
                {session.toolActivities.map((activity) => (
                  <li key={activity.id} className="tool-activity" aria-label={`工具 ${activity.tool} ${activity.status === 'running' ? '执行中' : '已完成'}`}>
                    <strong>{activity.tool}</strong>
                    <span>{activity.status === 'running' ? '执行中' : '已完成'}</span>
                    {activity.progress && <p>{activity.progress}</p>}
                    {activity.result && <pre>{activity.result}</pre>}
                  </li>
                ))}
              </ol>
            )}

            {session.approval && activeTab === 'chat' && (
              <article className="approval" aria-label="工具审批" data-request-id={session.approval.requestId}>
                <strong>需要审批：{session.approval.tool}</strong>
                {session.approval.summary === '破坏性 Git 操作需要再次确认' && <p role="status">这是第二次确认；再次允许后才会执行。</p>}
                <p>{session.approval.summary}</p>
                <div>
                  <button type="button" onClick={() => resolveApproval(true)}>允许</button>
                  <button type="button" onClick={() => resolveApproval(false)}>拒绝</button>
                </div>
              </article>
            )}

            {session.question && (
              <article className="approval" aria-label="问题">
                <strong>需要回答</strong>
                <p>{session.question.prompt}</p>
                <div>
                  <button type="button" onClick={() => answerQuestion('是')}>是</button>
                  <button type="button" onClick={() => answerQuestion('否')}>否</button>
                </div>
              </article>
            )}
          </div>
        </section>

        {/* Floating Capsule InputBar Dock */}
        <div className="composer-dock">
          <div className="composer-capsule">
            {suggestions.length > 0 && (
              <div className="composer-suggestions" role="listbox" aria-label="命令与文件建议" id="composer-suggestion-list">
                {suggestions.map((item, idx) => (
                  <button
                    type="button"
                    key={idx}
                    id={`composer-suggestion-${idx}`}
                    role="option"
                    aria-selected={idx === activeSuggestionIndex}
                    className={`suggestion-item${idx === activeSuggestionIndex ? ' active' : ''}`}
                    onMouseEnter={() => setActiveSuggestionIndex(idx)}
                    onClick={() => {
                      setPrompt(item.insert)
                      setSuggestions([])
                    }}
                  >
                    <span>{item.label}</span>
                    <small>{item.description}</small>
                  </button>
                ))}
              </div>
            )}

            <div className="composer-chips">
              <button type="button" className="chip-btn" onClick={() => setPrompt('/explain ')}>💡 /explain</button>
              <button type="button" className="chip-btn" onClick={() => setPrompt('/review ')}>🔍 /review</button>
              <button type="button" className="chip-btn" onClick={() => setPrompt('/test ')}>🧪 /test</button>
            </div>

            <form
              className="composer-form"
              onSubmit={(event) => {
                event.preventDefault()
                submit()
              }}
            >
              <textarea
                className="composer-textarea"
                data-testid="composer-input"
                aria-label="Prompt"
                aria-autocomplete={suggestions.length > 0 ? 'list' : undefined}
                aria-controls={suggestions.length > 0 ? 'composer-suggestion-list' : undefined}
                aria-activedescendant={suggestions.length > 0 ? `composer-suggestion-${activeSuggestionIndex}` : undefined}
                value={prompt}
                onChange={(event) => setPrompt(event.target.value)}
                onKeyDown={handleComposerKeyDown}
                placeholder="输入 Prompt（Enter 发送，Shift+Enter 换行，支持 / 命令与 @ 文件引用）"
              />

              <div className="composer-actions-row">
                <div className="composer-actions-left">
                  <button
                    type="button"
                    className="btn-circle-add"
                    title="添加命令或文件"
                    onClick={() => setPrompt((p) => (p ? `${p} /` : '/'))}
                  >
                    +
                  </button>
                  <span className="composer-mode-pill">通用模式</span>
                </div>

                <div className="composer-actions-right">
                  <span className="composer-model-pill">{session.settings?.model || 'gpt-4o'}</span>
                  <span className="composer-token-pill">
                    {session.usage ? `${session.usage.inputTokens + session.usage.outputTokens} tokens` : '0 tokens'}
                  </span>
                  <button
                    data-testid="composer-submit"
                    className="btn-submit-circle"
                    type="submit"
                    disabled={session.busy || !prompt.trim()}
                    title="发送"
                  >
                    ↑
                  </button>
                  {session.busy && (
                    <button type="button" className="btn-stop-square" onClick={cancel}>
                      停止
                    </button>
                  )}
                </div>
              </div>
            </form>
          </div>
        </div>
      </div>

      {/* =========================================================
          3. Right Column: ZCode Inspector Details Drawer
          ========================================================= */}
      <aside
        className="rightbar-col"
        data-shell-column="details"
        style={{ display: isRightbarOpen ? 'flex' : 'none' }}
        aria-label="工具面板"
      >
        <div className="rightbar-header">
          <div className="rightbar-title">
            {activeTab === 'files' && '📁 文件浏览器'}
            {activeTab === 'git' && '🌿 Git 控制台'}
            {activeTab === 'terminal' && '💻 终端控制台'}
            {activeTab === 'settings' && '⚙️ 设置面板'}
            {activeTab === 'marketplace' && '🧩 插件市场'}
            {activeTab === 'diff' && '🔍 代码差异审查'}
            {activeTab === 'chat' && session.activeFile && `📄 ${session.activeFile.path}`}
          </div>
          <button
            type="button"
            className="rightbar-close-btn"
            aria-label="关闭侧栏"
            title="关闭侧栏"
            onClick={() => {
              setActiveTab('chat')
              updateSession((current) => ({ ...current, activeFile: undefined }))
            }}
          >
            ✕
          </button>
        </div>

        <div className="rightbar-content" tabIndex={0} aria-label="工具面板内容">
          {session.approval && activeTab !== 'chat' && (
            <article className="approval" aria-label="工具审批" data-request-id={session.approval.requestId}>
              <strong>需要审批：{session.approval.tool}</strong>
              {session.approval.summary === '破坏性 Git 操作需要再次确认' && <p role="status">这是第二次确认；再次允许后才会执行。</p>}
              <p>{session.approval.summary}</p>
              {session.pendingGitOperation && <p>本次 Git 操作：{session.pendingGitOperation.operation}（参数：{gitOp === session.pendingGitOperation.operation ? gitArg : '单次操作'}）</p>}
              <div>
                <button type="button" onClick={() => resolveApproval(true)}>允许</button>
                <button type="button" onClick={() => resolveApproval(false)}>拒绝</button>
              </div>
            </article>
          )}
          {/* Active File Editor View */}
          {session.activeFile && (
            <section className="panel-section" aria-label="代码编辑器">
              <div className="panel-header">
                <h2>文件编辑：{session.activeFile.path}</h2>
                <button type="button" onClick={proposeFileWrite} disabled={workspaceWriteState === 'pending'}>{workspaceWriteState === 'pending' ? '等待审批/写入…' : '提议保存修改'}</button>
              </div>
              {workspaceWriteError && <p role="alert">保存失败：{workspaceWriteError}</p>}
              {workspaceWriteState === 'saved' && <p role="status">文件已保存，目录已刷新。</p>}
              {workspaceWriteState === 'pending' && !workspaceWriteApprovalId && <p role="status">等待服务器确认写入请求…</p>}
              {workspaceWriteApprovalId && activeTab !== 'chat' && <p role="status">写入请求待审批，可在对话页批准或拒绝。</p>}
              <textarea
                className="file-editor-textarea"
                aria-label={`编辑文件内容 ${session.activeFile.path}`}
                value={editingFileContent}
                onChange={(e) => setEditingFileContent(e.target.value)}
              />
            </section>
          )}

          {/* Tab: 文件 (Files) */}
          {activeTab === 'files' && (
            <section className="panel-view" aria-label="文件浏览器">
              <div className="panel-header">
                <h2>工作区文件</h2>
                <button type="button" onClick={() => refreshFiles(dirPath)}>刷新目录</button>
              </div>
              <div className="panel-section">
                <nav aria-label="文件路径导航" className="file-breadcrumbs">
                  <button type="button" onClick={() => { setDirPath('.'); refreshFiles('.') }}>workspace</button>
                  {(session.files?.path ?? dirPath).split('/').filter((part) => part && part !== '.').map((part, index, parts) => (
                    <span key={`${part}-${index}`}>
                      <span aria-hidden="true"> / </span>
                      <button type="button" onClick={() => {
                        const path = parts.slice(0, index + 1).join('/')
                        setDirPath(path)
                        refreshFiles(path)
                      }}>{part}</button>
                    </span>
                  ))}
                </nav>
                <button type="button" onClick={parentDirectory} disabled={(session.files?.path ?? dirPath) === '.'}>上级目录</button>
                <label>
                  路径：
                  <input
                    className="panel-input"
                    aria-label="目录路径"
                    value={dirPath}
                    onChange={(e) => setDirPath(e.target.value)}
                  />
                </label>
                <label>
                  搜索：
                  <input
                    className="panel-input"
                    aria-label="文件搜索关键词"
                    value={fileSearchQuery}
                    placeholder="关键词..."
                    onChange={(e) => setFileSearchQuery(e.target.value)}
                  />
                </label>
                <button type="button" onClick={searchFiles}>搜索文件</button>
              </div>

              {session.searchLoading && <p role="status">正在搜索文件…</p>}
              {session.searchError && <p role="alert">搜索失败：{session.searchError}</p>}
              {session.searchResults && (
                <div className="panel-section">
                  <h2>搜索结果（关键词：{session.searchResults.query}）</h2>
                  {session.searchResults.matches.length === 0 ? (
                    <p>未找到匹配的文件</p>
                  ) : (
                    <ul className="file-tree-list">
                      {session.searchResults.matches.map((match) => (
                        <li key={match}>
                          <button type="button" className="file-item-btn" onClick={() => openFile(match)}>
                            📄 {match}
                          </button>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              )}

              <div className="panel-section" aria-label="附件上传">
                <h2>附件上传</h2>
                <label>
                  选择文件：
                  <input
                    type="file"
                    className="panel-input"
                    aria-label="选择附件文件"
                    data-testid="upload-file-input"
                    onChange={(e) => { setUploadPick(e.target.files?.[0] ?? null); setUploadTargetPath(''); setUploadPickError(undefined) }}
                  />
                </label>
                <label>
                  写入路径：
                  <input
                    className="panel-input"
                    aria-label="附件写入路径"
                    data-testid="upload-target-path"
                    value={uploadTargetPath}
                    placeholder={uploadPick?.name ?? 'workspace 内的相对路径'}
                    onChange={(e) => setUploadTargetPath(e.target.value)}
                  />
                </label>
                <button
                  type="button"
                  data-testid="upload-submit"
                  onClick={startUpload}
                  disabled={!uploadPick || (session.upload !== undefined && !['done', 'failed', 'cancelled'].includes(session.upload.status))}
                >
                  上传附件
                </button>
                {session.upload && !['done', 'failed', 'cancelled'].includes(session.upload.status) && (
                  <button type="button" data-testid="upload-cancel" onClick={cancelUpload}>取消上传</button>
                )}
                {uploadPickError && <p role="alert">{uploadPickError}</p>}
                {session.upload && (
                  <>
                    <p role="status" data-testid="upload-status">{describeUpload(session.upload)}</p>
                    {session.upload.error && <p role="alert">上传失败：{session.upload.error}</p>}
                  </>
                )}
              </div>

              {session.fileLoading && <p role="status">正在读取文件…</p>}
              {session.fileError && <p role="alert">文件读取失败：{session.fileError}</p>}
              <div className="panel-section">
                <h2>目录列表（{session.files?.path ?? dirPath}）</h2>
                {session.filesLoading && <p role="status">正在读取目录…</p>}
                {session.filesError && <p role="alert">目录读取失败：{session.filesError}</p>}
                {session.files?.entries && session.files.entries.length > 0 ? (
                  <ul className="file-tree-list">
                    {session.files.entries.map((entry) => {
                      const isDirectory = session.files?.directories.includes(entry) ?? false
                      const path = (session.files?.path ?? dirPath) === '.' ? entry : `${(session.files?.path ?? dirPath).replace(/\/$/, '')}/${entry}`
                      return (
                        <li key={entry}>
                          {isDirectory ? (
                            <button type="button" className="file-item-btn" onClick={() => openDirectory(entry)} aria-label={`打开目录 ${entry}`}>
                              📁 {entry}
                            </button>
                          ) : (
                            <button type="button" className="file-item-btn" onClick={() => openFile(path)}>
                              📄 {entry}
                            </button>
                          )}
                        </li>
                      )
                    })}
                  </ul>
                ) : !session.filesLoading && !session.filesError ? (
                  <p>目录为空</p>
                ) : null}
              </div>
            </section>
          )}

          {/* Tab: Git */}
          {activeTab === 'git' && (
            <section className="panel-view" aria-label="Git 状态">
              <div className="panel-header">
                <h2>Git 控制台</h2>
                <button type="button" onClick={refreshGitStatus}>刷新 Git 状态</button>
              </div>
              <div className="panel-section">
                {session.gitLoading && <p role="status">Git 请求处理中…</p>}
                {session.gitError && <p role="alert">Git 请求失败：{session.gitError}</p>}
                <p><strong>当前分支：</strong><code>{session.gitStatus?.branch ?? '无 (未初始化或未知)'}</code></p>
                <label>
                  操作类型：
                  <select
                    className="panel-select"
                    aria-label="Git 操作类型"
                    value={gitOp}
                    onChange={(e) => setGitOp(e.target.value as typeof gitOp)}
                  >
                    <option value="stage">stage (暂存)</option>
                    <option value="unstage">unstage (取消暂存)</option>
                    <option value="commit">commit (提交)</option>
                    <option value="checkout_branch">checkout_branch (切换分支)</option>
                    <option value="discard">discard (放弃变更)</option>
                  </select>
                </label>
                <label>
                  参数：
                  <input
                    className="panel-input"
                    aria-label="Git 参数"
                    placeholder="路径或 commit message"
                    value={gitArg}
                    onChange={(e) => setGitArg(e.target.value)}
                  />
                </label>
                <button type="button" onClick={executeGitMutation} disabled={session.gitLoading}>执行 Git 操作</button>
                {session.gitMutationResult && (
                  <p>
                    <strong>操作结果：</strong>
                    <code>{session.gitMutationResult.result}</code>
                  </p>
                )}
              </div>

              <div className="panel-section">
                <h2>变更文件列表</h2>
                {session.gitStatus?.entries && session.gitStatus.entries.length > 0 ? (
                  <ul className="git-entries-list" tabIndex={0} aria-label="变更文件列表">
                    {session.gitStatus.entries.map((entry, idx) => (
                      <li key={idx} className="git-entry-item">
                        <code>{entry}</code>
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p>工作区干净，无未提交的变更。</p>
                )}
              </div>
            </section>
          )}

          {/* Tab: 终端 (Terminal) */}
          {activeTab === 'terminal' && (
            <section className="panel-view" aria-label="终端控制台">
              <div className="panel-header">
                <h2>命令行执行</h2>
                <button type="button" onClick={executeTerminalCommand} disabled={session.terminalLoading}>运行命令</button>
              </div>
              {session.terminalLoading && <p role="status">命令执行中…</p>}
              {session.terminalError && <p role="alert">终端执行失败：{session.terminalError}</p>}
              <div className="panel-section">
                <label>
                  终端命令：
                  <input
                    className="panel-input"
                    aria-label="终端执行命令"
                    placeholder="git status / ls -la"
                    value={terminalCmd}
                    onChange={(e) => setTerminalCmd(e.target.value)}
                  />
                </label>
              </div>

              {session.terminalResult && (
                <section className="panel-section" aria-label="终端控制台输出">
                  <h2>终端输出</h2>
                  <p>
                    <strong>退出码：</strong>
                    <span className={`validation-badge ${session.terminalResult.exit_code === 0 ? 'success' : 'fail'}`}>
                      {session.terminalResult.exit_code}
                    </span>
                  </p>
                  <pre className="terminal-window">
                    <code>{session.terminalResult.output || '（命令输出为空）'}</code>
                  </pre>
                </section>
              )}
            </section>
          )}

          {/* Tab: 设置 (Settings) */}
          {activeTab === 'settings' && (
            <section className="panel-view" aria-label="设置面板">
              <div className="panel-header">
                <h2>模型与服务设置</h2>
                <button type="button" onClick={fetchSettings}>重新获取设置</button>
              </div>
              <div className="panel-section">
                <label>
                  Provider Base URL:
                  <input
                    className="panel-input"
                    aria-label="Provider Base URL"
                    placeholder="https://api.openai.com/v1"
                    value={settingsBaseUrl}
                    onChange={(e) => setSettingsBaseUrl(e.target.value)}
                  />
                </label>
                <label>
                  模型 (Model):
                  <input
                    className="panel-input"
                    aria-label="Provider Model"
                    placeholder="gpt-4o / llama-3.3-70b"
                    value={settingsModel}
                    onChange={(e) => setSettingsModel(e.target.value)}
                  />
                </label>
                <p>
                  API Key 状态：
                  <strong>{session.settings?.hasApiKey ? '已配置 API Key' : '未检测到 API Key'}</strong>
                </p>
                {session.providerValidation && (
                  <p>
                    Provider 状态：
                    <span className={`validation-badge ${session.providerValidation.reachable ? 'success' : 'fail'}`}>
                      {session.providerValidation.reachable ? '连通正常' : `校验未通过 (${session.providerValidation.errorCode ?? 'unknown'})`}
                    </span>
                  </p>
                )}
                <div style={{ display: 'flex', gap: '8px' }}>
                  <button type="button" onClick={saveSettings}>保存设置</button>
                  <button type="button" onClick={validateProvider}>验证 Provider 连接</button>
                </div>
              </div>

              <div className="panel-section">
                <h2>导入已有 TUI 会话</h2>
                <label>
                  TUI 会话 ID：
                  <input
                    className="panel-input"
                    aria-label="TUI 会话 ID"
                    placeholder="session uuid / id"
                    value={tuiSessionId}
                    onChange={(e) => setTuiSessionId(e.target.value)}
                  />
                </label>
                <label>
                  目录：
                  <input
                    className="panel-input"
                    aria-label="TUI 工作目录"
                    value={tuiRoot}
                    onChange={(e) => setTuiRoot(e.target.value)}
                  />
                </label>
                <button type="button" onClick={importTuiSession}>导入会话</button>
                {session.tuiImport && (
                  <p>
                    <strong>导入成功：</strong>
                    <span>会话 ID: {session.tuiImport.sessionId}，消息条数: {session.tuiImport.messageCount}</span>
                  </p>
                )}
              </div>
            </section>
          )}

          {/* Tab: 插件 (Marketplace) */}
          {activeTab === 'marketplace' && (
            <section className="panel-view" aria-label="插件市场">
              <div className="panel-header">
                <h2>插件生态与 Skills</h2>
                <button type="button" onClick={scanMarketplace}>扫描插件生态</button>
              </div>
              <div className="panel-section">
                <label>
                  扫描根目录：
                  <input
                    className="panel-input"
                    aria-label="插件根目录"
                    value={marketplaceRoot}
                    onChange={(e) => setMarketplaceRoot(e.target.value)}
                  />
                </label>
              </div>
              <div className="panel-section">
                <h2>已发现插件 ({session.marketplaceEntries?.length ?? 0})</h2>
                {session.marketplaceEntries && session.marketplaceEntries.length > 0 ? (
                  <ul className="plugin-list">
                    {session.marketplaceEntries.map((plugin) => (
                      <li key={plugin.name} className="plugin-card">
                        <div className="plugin-title">
                          <strong>{plugin.name}</strong>
                          {plugin.version && <span className="plugin-badge">v{plugin.version}</span>}
                          {plugin.has_agents && <span className="plugin-badge">Agent</span>}
                          {plugin.has_mcp && <span className="plugin-badge">MCP</span>}
                          {plugin.has_hooks && <span className="plugin-badge">Hook</span>}
                        </div>
                        {plugin.description && <p style={{ margin: 0, fontSize: '13px' }}>{plugin.description}</p>}
                        <small style={{ color: 'var(--dsw-alias-label-tertiary)' }}>
                          作者: {plugin.author ?? '官方'} · 路径: {plugin.relative_path}
                        </small>
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p>暂无扫描到的插件，点击“扫描插件生态”以加载扩展。</p>
                )}
              </div>
            </section>
          )}

          {/* Tab: 差异 (Diff) */}
          {activeTab === 'diff' && (
            <section className="panel-view" aria-label="代码差异审查">
              <div className="panel-header">
                <h2>代码差异审查 (Diff Review)</h2>
                <button type="button" onClick={() => previewDiff(manualProposalId)}>加载差异</button>
              </div>
              <div className="panel-section">
                <label>
                  手动输入提案 ID：
                  <input
                    className="panel-input"
                    aria-label="提案 ID"
                    value={manualProposalId}
                    placeholder="proposal-..."
                    onChange={(e) => setManualProposalId(e.target.value)}
                  />
                </label>
              </div>

              {session.diffPreview && (
                <section className="panel-section" aria-label="Diff 详细比对">
                  <div className="panel-header">
                    <div>
                      <strong>目标文件：{session.diffPreview.path}</strong>
                      <p style={{ margin: '2px 0 0', fontSize: '12px' }}>提案 ID: {session.diffPreview.proposal_id}</p>
                    </div>
                    <div style={{ display: 'flex', gap: '8px' }}>
                      <button type="button" onClick={() => acceptDiff(session.diffPreview!.proposal_id)}>
                        接受变更 (Accept)
                      </button>
                      <button type="button" onClick={() => rollbackDiff(session.diffPreview!.proposal_id)}>
                        回滚变更 (Rollback)
                      </button>
                    </div>
                  </div>
                  <div className="diff-viewer">
                    <div className="diff-pane">
                      <h3>变更前 (Before)</h3>
                      <pre><code>{session.diffPreview.before ?? '（新创建的文件）'}</code></pre>
                    </div>
                    <div className="diff-pane">
                      <h3>变更后 (After)</h3>
                      <pre><code>{session.diffPreview.after}</code></pre>
                    </div>
                  </div>
                </section>
              )}
            </section>
          )}
        </div>
      </aside>
    </main>
  )
}

createRoot(document.getElementById('root')!).render(<App />)
