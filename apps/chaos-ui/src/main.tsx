import { useCallback, useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { applyServerMessage, initialSessionState, workspaceReconnectMessage, type Approval, type Message, type Question, type ServerMessage } from './session'
import { selectWorkspaceSession } from './workspace-ui'
import { webSocketUrl } from './transport'
import { initialComposerHistory, navigatePromptHistory, recordPrompt, shouldSubmitOnKey } from './composer'
import { defaultLayoutState, loadLayoutState, saveLayoutState, type LayoutState } from './layout'
import './style.css'

function safeMarkdownHref(href: string | undefined): { href: string; external: boolean } | undefined {
  if (!href) return undefined
  if (href.startsWith('#')) return { href, external: false }
  if (/^https?:\/\//i.test(href)) return { href, external: true }
  if (/^mailto:/i.test(href)) return { href, external: false }
  return undefined
}

function App() {
  const [session, setSession] = useState(initialSessionState)
  const [prompt, setPrompt] = useState('')
  const [promptHistory, setPromptHistory] = useState(initialComposerHistory)
  const [layout, setLayout] = useState<LayoutState>(() => {
    if (typeof window === 'undefined') return defaultLayoutState
    try { return loadLayoutState(window.localStorage) } catch { return defaultLayoutState }
  })
  const socket = useRef<WebSocket | null>(null)
  const sessionStateRef = useRef(session)
  const reconnectTimer = useRef<number | undefined>(undefined)

  useEffect(() => { sessionStateRef.current = session }, [session])
  useEffect(() => {
    if (typeof window === 'undefined') return
    try { saveLayoutState(window.localStorage, layout) } catch { setSession((current) => ({ ...current, status: '布局未保存（本地存储不可用）' })) }
  }, [layout])
  useEffect(() => {
    document.documentElement.dataset.theme = layout.theme
    document.documentElement.style.setProperty('--sidebar-width', `${layout.sidebarWidth}px`)
    document.documentElement.style.setProperty('--composer-height', `${layout.composerHeight}px`)
  }, [layout])

  const send = useCallback((message: object) => {
    if (socket.current?.readyState === WebSocket.OPEN) socket.current.send(JSON.stringify(message))
  }, [])

  const connect = useCallback(() => {
    const ws = new WebSocket(webSocketUrl(location))
    socket.current = ws
    ws.onopen = () => {
      setSession((current) => ({ ...current, status: '已连接' }))
      send({ type: 'list_workspaces', client_msg_id: crypto.randomUUID() })
      send(workspaceReconnectMessage(sessionStateRef.current))
    }
    ws.onmessage = (event) => {
      const message = JSON.parse(event.data) as ServerMessage
      if (message.type === 'session_created') (window as Window & { __chaosDevWorkspaceCreated?: boolean }).__chaosDevWorkspaceCreated = true
      setSession((current) => applyServerMessage(current, message))
    }
    ws.onerror = () => setSession((current) => ({ ...current, status: '连接错误' }))
    ws.onclose = () => { setSession((current) => ({ ...current, status: '连接断开，正在重连' })); reconnectTimer.current = window.setTimeout(connect, 500) }
  }, [send])

  useEffect(() => { connect(); return () => { if (reconnectTimer.current) window.clearTimeout(reconnectTimer.current); socket.current?.close() } }, [connect])

  function createWorkspace() {
    const name = window.prompt('工作区名称')?.trim()
    if (!name) return
    setSession((current) => ({ ...current, messages: [], approval: undefined, question: undefined, busy: false, status: '正在创建工作区' }))
    send({ type: 'create_workspace', client_msg_id: crypto.randomUUID(), name })
  }
  function switchWorkspace(workspaceId: string) {
    setSession((current) => selectWorkspaceSession(current, workspaceId))
    send({ type: 'switch_workspace', client_msg_id: crypto.randomUUID(), workspace_id: workspaceId })
  }
  function archiveWorkspace(workspaceId: string) {
    setSession((current) => current.activeWorkspaceId === workspaceId
      ? { ...current, messages: [], approval: undefined, question: undefined, busy: false, status: '正在归档工作区' }
      : current)
    send({ type: 'archive_workspace', client_msg_id: crypto.randomUUID(), workspace_id: workspaceId })
  }

  function submit() {
    const value = prompt.trim(); if (!value || session.busy || !session.sessionId) return
    setSession((current) => ({ ...current, messages: [...current.messages, { role: 'user', text: value }, { role: 'assistant', text: '' }], busy: true })); setPromptHistory((current) => recordPrompt(current, value)); setPrompt('')
    send({ type: 'submit', client_msg_id: crypto.randomUUID(), session_id: session.sessionId, prompt: value })
  }
  function cancel() { if (session.sessionId) send({ type: 'cancel', client_msg_id: crypto.randomUUID(), session_id: session.sessionId }) }
  function resolveApproval(approved: boolean) { if (!session.approval) return; send(approved ? { type: 'approve', client_msg_id: crypto.randomUUID(), request_id: session.approval.requestId } : { type: 'reject', client_msg_id: crypto.randomUUID(), request_id: session.approval.requestId, reason: '用户拒绝' }); setSession((current) => ({ ...current, approval: undefined })) }
  function answerQuestion(answer: string) { if (!session.question || !answer.trim()) return; send({ type: 'respond_question', client_msg_id: crypto.randomUUID(), question_id: session.question.questionId, answer }); setSession((current) => ({ ...current, question: undefined })) }

  function handleComposerKeyDown(event: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (event.nativeEvent.isComposing) return
    if (event.key === 'ArrowUp' && !event.shiftKey && !event.nativeEvent.isComposing && event.currentTarget.selectionStart === 0) {
      event.preventDefault()
      const result = navigatePromptHistory(promptHistory, -1, prompt)
      setPromptHistory(result.history)
      setPrompt(result.value)
      return
    }
    if (event.key === 'ArrowDown' && !event.shiftKey && !event.nativeEvent.isComposing && event.currentTarget.selectionEnd === event.currentTarget.value.length) {
      event.preventDefault()
      const result = navigatePromptHistory(promptHistory, 1, prompt)
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

  return <main className="shell" data-testid="app-shell"><header><strong>Chaos</strong><span>{session.status} · Web / Desktop <button type="button" onClick={togglePanel}>{layout.panelOpen ? '隐藏面板' : '显示面板'}</button> <button type="button" onClick={cycleTheme}>主题：{layout.theme}</button></span><label>面板宽度 <input aria-label="面板宽度" type="range" min="160" max="480" step="10" value={layout.sidebarWidth} onChange={(event) => setLayout((current) => ({ ...current, sidebarWidth: Number(event.target.value) }))} /></label><label>输入区高度 <input aria-label="输入区高度" type="range" min="88" max="480" step="8" value={layout.composerHeight} onChange={(event) => setLayout((current) => ({ ...current, composerHeight: Number(event.target.value) }))} /></label></header><aside hidden={!layout.panelOpen} className="workspaces" aria-label="工作区" data-testid="workspace-list"><button type="button" onClick={createWorkspace}>+ 新工作区</button>{session.workspaces.filter((workspace) => !workspace.archived).map((workspace) => <button type="button" key={workspace.id} data-testid={`workspace-${workspace.id}`} className={workspace.id === session.activeWorkspaceId ? 'active' : ''} onClick={() => switchWorkspace(workspace.id)}>{workspace.name}<span><small>{workspace.id === session.activeWorkspaceId ? '当前' : '切换'}</small><small role="button" tabIndex={0} onClick={(event) => { event.stopPropagation(); archiveWorkspace(workspace.id) }} onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); event.stopPropagation(); archiveWorkspace(workspace.id) } }}>归档</small></span></button>)}</aside><section className="timeline" aria-label="会话时间线" data-testid="session-timeline">{session.messages.length === 0 && <p className="empty">创建会话后，在下方输入 Prompt。</p>}{session.messages.map((message, index) => <article className={message.role} key={index}><small>{message.role === 'user' ? '你' : 'Chaos'}</small><div className="markdown-body"><ReactMarkdown remarkPlugins={[remarkGfm]} components={{ a: ({ href, children }) => {
  const safe = safeMarkdownHref(href)
  return safe ? <a href={safe.href} target={safe.external ? '_blank' : undefined} rel={safe.external ? 'noopener noreferrer' : undefined}>{children}</a> : <span>{children}</span>
} }}>{message.text || '正在生成…'}</ReactMarkdown></div></article>)}{session.approval && <article className="approval" aria-label="工具审批"><strong>需要审批：{session.approval.tool}</strong><p>{session.approval.summary}</p><div><button type="button" onClick={() => resolveApproval(true)}>允许</button><button type="button" onClick={() => resolveApproval(false)}>拒绝</button></div></article>}{session.question && <article className="approval" aria-label="问题"><strong>需要回答</strong><p>{session.question.prompt}</p><div><button type="button" onClick={() => answerQuestion('是')}>是</button><button type="button" onClick={() => answerQuestion('否')}>否</button></div></article>}</section><form onSubmit={(event) => { event.preventDefault(); submit() }}><textarea data-testid="composer-input" aria-label="Prompt" value={prompt} onChange={(event) => setPrompt(event.target.value)} onKeyDown={handleComposerKeyDown} placeholder="输入 Prompt（Enter 发送，Shift+Enter 换行）" /><button data-testid="composer-submit" type="submit" disabled={session.busy || !prompt.trim()}>发送</button>{session.busy && <button type="button" onClick={cancel}>停止</button>}</form></main>
}
createRoot(document.getElementById('root')!).render(<App />)
