import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import './style.css'

type Message = { role: 'user' | 'assistant'; text: string }

function App() {
  const [messages, setMessages] = useState<Message[]>([])
  const [prompt, setPrompt] = useState('')
  const [busy, setBusy] = useState(false)

  function submit() {
    const value = prompt.trim()
    if (!value || busy) return
    setMessages((current) => [...current, { role: 'user', text: value }])
    setPrompt('')
    setBusy(true)
    window.setTimeout(() => {
      setMessages((current) => [...current, { role: 'assistant', text: `演示响应：${value}` }])
      setBusy(false)
    }, 120)
  }

  return <main className="shell">
    <header><strong>Chaos</strong><span>Web / Desktop walking skeleton</span></header>
    <section className="timeline" aria-label="会话时间线">
      {messages.length === 0 && <p className="empty">创建会话后，在下方输入 Prompt。</p>}
      {messages.map((message, index) => <article className={message.role} key={index}><small>{message.role === 'user' ? '你' : 'Chaos'}</small><p>{message.text}</p></article>)}
      {busy && <article className="assistant"><small>Chaos</small><p>正在生成…</p></article>}
    </section>
    <form onSubmit={(event) => { event.preventDefault(); submit() }}>
      <textarea aria-label="Prompt" value={prompt} onChange={(event) => setPrompt(event.target.value)} placeholder="输入 Prompt" />
      <button type="submit" disabled={busy || !prompt.trim()}>{busy ? '生成中' : '发送'}</button>
    </form>
  </main>
}

createRoot(document.getElementById('root')!).render(<App />)
