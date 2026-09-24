import { useCallback, useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { applyServerMessage, initialSessionState, workspaceReconnectMessage, type Approval, type Message, type Question, type ServerMessage } from './session'
import { selectWorkspaceSession } from './workspace-ui'
import { webSocketUrl } from './transport'
import './style.css'

function App() {
  const [session, setSession] = useState(initialSessionState)
  const [prompt, setPrompt] = useState('')
  const socket = useRef<WebSocket | null>(null)
  const sessionStateRef = useRef(session)
  const reconnectTimer = useRef<number | undefined>(undefined)

  useEffect(() => { sessionStateRef.current = session }, [session])

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
  function archiveWorkspace(workspaceId: string) { send({ type: 'archive_workspace', client_msg_id: crypto.randomUUID(), workspace_id: workspaceId }) }

  function submit() {
    const value = prompt.trim(); if (!value || session.busy || !session.sessionId) return
    setSession((current) => ({ ...current, messages: [...current.messages, { role: 'user', text: value }, { role: 'assistant', text: '' }], busy: true })); setPrompt('')
    send({ type: 'submit', client_msg_id: crypto.randomUUID(), session_id: session.sessionId, prompt: value })
  }
  function cancel() { if (session.sessionId) send({ type: 'cancel', client_msg_id: crypto.randomUUID(), session_id: session.sessionId }) }
  function resolveApproval(approved: boolean) { if (!session.approval) return; send(approved ? { type: 'approve', client_msg_id: crypto.randomUUID(), request_id: session.approval.requestId } : { type: 'reject', client_msg_id: crypto.randomUUID(), request_id: session.approval.requestId, reason: '用户拒绝' }); setSession((current) => ({ ...current, approval: undefined })) }
  function answerQuestion(answer: string) { if (!session.question || !answer.trim()) return; send({ type: 'respond_question', client_msg_id: crypto.randomUUID(), question_id: session.question.questionId, answer }); setSession((current) => ({ ...current, question: undefined })) }

  return <main className="shell"><header><strong>Chaos</strong><span>{session.status} · Web / Desktop</span></header><aside className="workspaces" aria-label="工作区"><button type="button" onClick={createWorkspace}>+ 新工作区</button>{session.workspaces.filter((workspace) => !workspace.archived).map((workspace) => <button type="button" key={workspace.id} className={workspace.id === session.activeWorkspaceId ? 'active' : ''} onClick={() => switchWorkspace(workspace.id)}>{workspace.name}<span><small>{workspace.id === session.activeWorkspaceId ? '当前' : '切换'}</small><small onClick={(event) => { event.stopPropagation(); archiveWorkspace(workspace.id) }}>归档</small></span></button>)}</aside><section className="timeline" aria-label="会话时间线">{session.messages.length === 0 && <p className="empty">创建会话后，在下方输入 Prompt。</p>}{session.messages.map((message, index) => <article className={message.role} key={index}><small>{message.role === 'user' ? '你' : 'Chaos'}</small><p>{message.text || '正在生成…'}</p></article>)}{session.approval && <article className="approval" aria-label="工具审批"><strong>需要审批：{session.approval.tool}</strong><p>{session.approval.summary}</p><div><button type="button" onClick={() => resolveApproval(true)}>允许</button><button type="button" onClick={() => resolveApproval(false)}>拒绝</button></div></article>}{session.question && <article className="approval" aria-label="问题"><strong>需要回答</strong><p>{session.question.prompt}</p><div><button type="button" onClick={() => answerQuestion('是')}>是</button><button type="button" onClick={() => answerQuestion('否')}>否</button></div></article>}</section><form onSubmit={(event) => { event.preventDefault(); submit() }}><textarea aria-label="Prompt" value={prompt} onChange={(event) => setPrompt(event.target.value)} placeholder="输入 Prompt" /><button type="submit" disabled={session.busy || !prompt.trim()}>发送</button>{session.busy && <button type="button" onClick={cancel}>停止</button>}</form></main>
}
createRoot(document.getElementById('root')!).render(<App />)
