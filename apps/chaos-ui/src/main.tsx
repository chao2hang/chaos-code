import { useCallback, useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import './style.css'

type Message = { role: 'user' | 'assistant'; text: string }
type ServerMessage = { type: string; session_id?: string; text?: string; messages?: Message[]; protocol_version?: number }

function App() {
  const [messages, setMessages] = useState<Message[]>([])
  const [prompt, setPrompt] = useState('')
  const [busy, setBusy] = useState(false)
  const [status, setStatus] = useState('连接中')
  const [sessionId, setSessionId] = useState<string>()
  const socket = useRef<WebSocket | null>(null)
  const sessionRef = useRef<string | undefined>(undefined)
  const reconnectTimer = useRef<number | undefined>(undefined)

  const connect = useCallback(() => {
    const ws = new WebSocket(`ws://${location.hostname || '127.0.0.1'}:8787/ws`)
    socket.current = ws
    ws.onopen = () => {
      setStatus('已连接')
      if (sessionRef.current) {
        ws.send(JSON.stringify({ type: 'resume', client_msg_id: crypto.randomUUID(), session_id: sessionRef.current }))
      } else {
        ws.send(JSON.stringify({ type: 'create_session', client_msg_id: crypto.randomUUID() }))
      }
    }
    ws.onmessage = (event) => {
      const message = JSON.parse(event.data) as ServerMessage
      if (message.type === 'session_created' && message.session_id) {
        sessionRef.current = message.session_id
        setSessionId(message.session_id)
        setStatus('会话已创建')
      }
      if (message.type === 'session_snapshot' && message.messages) {
        setMessages(message.messages)
        setStatus('历史已恢复')
      }
      if (message.type === 'text_delta') {
        setMessages((current) => {
          const last = current[current.length - 1]
          if (!last || last.role !== 'assistant') return [...current, { role: 'assistant', text: message.text ?? '' }]
          return [...current.slice(0, -1), { ...last, text: `${last.text}${message.text ?? ''}` }]
        })
      }
      if (message.type === 'completed' || message.type === 'cancelled') setBusy(false)
      if (message.type === 'error') { setBusy(false); setStatus('请求错误') }
    }
    ws.onerror = () => setStatus('连接错误')
    ws.onclose = () => {
      setStatus('连接断开，正在重连')
      reconnectTimer.current = window.setTimeout(connect, 500)
    }
  }, [])

  useEffect(() => {
    connect()
    return () => { if (reconnectTimer.current) window.clearTimeout(reconnectTimer.current); socket.current?.close() }
  }, [connect])

  function submit() {
    const value = prompt.trim()
    if (!value || busy || !sessionId || socket.current?.readyState !== WebSocket.OPEN) return
    setMessages((current) => [...current, { role: 'user', text: value }, { role: 'assistant', text: '' }])
    setPrompt(''); setBusy(true)
    socket.current.send(JSON.stringify({ type: 'submit', client_msg_id: crypto.randomUUID(), session_id: sessionId, prompt: value }))
  }
  function cancel() {
    if (sessionId && socket.current?.readyState === WebSocket.OPEN) socket.current.send(JSON.stringify({ type: 'cancel', client_msg_id: crypto.randomUUID(), session_id: sessionId }))
  }

  return <main className="shell"><header><strong>Chaos</strong><span>{status} · Web / Desktop</span></header><section className="timeline" aria-label="会话时间线">{messages.length === 0 && <p className="empty">创建会话后，在下方输入 Prompt。</p>}{messages.map((message, index) => <article className={message.role} key={index}><small>{message.role === 'user' ? '你' : 'Chaos'}</small><p>{message.text || '正在生成…'}</p></article>)}</section><form onSubmit={(event) => { event.preventDefault(); submit() }}><textarea aria-label="Prompt" value={prompt} onChange={(event) => setPrompt(event.target.value)} placeholder="输入 Prompt" /><button type="submit" disabled={busy || !prompt.trim()}>发送</button>{busy && <button type="button" onClick={cancel}>停止</button>}</form></main>
}
createRoot(document.getElementById('root')!).render(<App />)
