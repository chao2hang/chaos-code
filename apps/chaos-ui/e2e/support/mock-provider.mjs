// A self-built OpenAI-compatible inference endpoint for the browser E2E.
//
// Anything the host asks an inference endpoint for -- a commit-form suggestion or
// the answer to a prompt -- comes from whatever provider it was started with, so
// the E2E needs an endpoint of its own. This one speaks the real
// wire format the shipped `HttpPromptAdapter` parses (SSE deltas on
// `/v1/chat/completions`, bearer auth), records the prompts it received so a spec
// can assert what the browser actually caused to be sent, and can hold a reply
// until a spec says otherwise -- which is how the "user typed while waiting"
// branch is reached without sleeping.
//
// It can also answer slowly, one small frame at a time, at a spec's request (see
// `paceFile` in `support/paths.ts`), and records an answer it had to stop writing
// because the host hung up (`cutLog`). Together those two are what let a spec
// follow one streamed turn from the keystroke that started it to the socket the
// producer was reading from.
import { createServer } from 'node:http'
import { appendFileSync, writeFileSync } from 'node:fs'

const port = Number(process.env.CHAOS_E2E_PROVIDER_PORT || 8791)
const model = process.env.CHAOS_E2E_PROVIDER_MODEL || 'e2e-commit-model'
const apiKey = process.env.CHAOS_E2E_PROVIDER_KEY || 'sk-e2e-commit-key'
const promptLog = process.env.CHAOS_E2E_PROVIDER_PROMPT_LOG
const holdFile = process.env.CHAOS_E2E_PROVIDER_HOLD_FILE
const paceFile = process.env.CHAOS_E2E_PROVIDER_PACE_FILE
const cutLog = process.env.CHAOS_E2E_PROVIDER_CUT_LOG
const replyText = process.env.CHAOS_E2E_PROVIDER_REPLY || 'docs: 按暂存差异补充 note.txt 的说明'
// Wrapped the way a chatty model wraps an answer, so the E2E proves the host
// strips it rather than proving the fixture was already clean.
const replyFrames = ['```text\n', replyText, '\n```']
const defaultFrameGapMs = 15

if (promptLog) writeFileSync(promptLog, '')
if (cutLog) writeFileSync(cutLog, '')

const readJson = async (path) => {
  const { readFile } = await import('node:fs/promises')
  try {
    return JSON.parse(await readFile(path, 'utf8'))
  } catch {
    return null
  }
}

const exists = async (path) => {
  const { access } = await import('node:fs/promises')
  try {
    await access(path)
    return true
  } catch {
    return false
  }
}

const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

async function waitForRelease() {
  if (!holdFile || !(await exists(holdFile))) return
  const deadline = Date.now() + 20_000
  while (await exists(holdFile)) {
    if (Date.now() > deadline) throw new Error('the held request was never released')
    await delay(25)
  }
}

function readBody(request) {
  return new Promise((resolve, reject) => {
    let body = ''
    request.on('data', (chunk) => {
      body += chunk
      if (body.length > 4 * 1024 * 1024) reject(new Error('request body too large'))
    })
    request.on('end', () => resolve(body))
    request.on('error', reject)
  })
}

const server = createServer(async (request, response) => {
  const url = new URL(request.url || '/', `http://127.0.0.1:${port}`)
  if (request.method === 'GET' && url.pathname === '/health') {
    response.writeHead(200, { 'content-type': 'text/plain' })
    response.end('ok')
    return
  }
  if (request.method === 'GET' && url.pathname === '/v1/models') {
    response.writeHead(200, { 'content-type': 'application/json' })
    response.end(JSON.stringify({ object: 'list', data: [{ id: model, object: 'model' }] }))
    return
  }
  if (request.method !== 'POST' || !url.pathname.endsWith('/chat/completions')) {
    response.writeHead(404, { 'content-type': 'application/json' })
    response.end(JSON.stringify({ error: { message: `no route ${url.pathname}` } }))
    return
  }
  if (request.headers.authorization !== `Bearer ${apiKey}`) {
    response.writeHead(401, { 'content-type': 'application/json' })
    response.end(JSON.stringify({ error: { message: 'no valid API Key' } }))
    return
  }
  const body = JSON.parse((await readBody(request)) || '{}')
  if (promptLog) appendFileSync(promptLog, `${JSON.stringify(body)}\n`)
  if (body.model !== model) {
    response.writeHead(400, { 'content-type': 'application/json' })
    response.end(JSON.stringify({ error: { message: `unknown model ${body.model}` } }))
    return
  }
  await waitForRelease()
  // A spec that wants to watch an answer arrive over time, or to be still waiting
  // when the user presses 停止, leaves a pace file naming the text and the frame
  // shape. Nothing about it is special to the endpoint: it is the same SSE a real
  // provider produces, just spread out.
  const pace = paceFile ? await readJson(paceFile) : null
  const frames = pace?.text
    ? pace.text.match(new RegExp(`[\\s\\S]{1,${Math.max(1, Number(pace.frameChars) || 4)}}`, 'g')) || []
    : replyFrames
  const gapMs = pace?.text ? Math.max(0, Number(pace.gapMs) || 0) : defaultFrameGapMs
  response.writeHead(200, {
    'content-type': 'text/event-stream',
    'cache-control': 'no-cache',
    connection: 'keep-alive',
  })
  // The host aborts the request when the turn it was streaming gets cancelled, and a
  // write into a socket the other end already closed is an error event on this
  // response. Without a handler that error is thrown at the top of the socket.
  response.once('error', () => {})
  let sent = ''
  let finished = false
  response.once('close', () => {
    if (finished || !cutLog) return
    appendFileSync(cutLog, `${JSON.stringify({ model: body.model, sent })}\n`)
  })
  for (const delta of frames) {
    if (response.destroyed) return
    response.write(`data: ${JSON.stringify({ choices: [{ delta: { content: delta } }] })}\n\n`)
    sent += delta
    if (gapMs) await delay(gapMs)
  }
  if (response.destroyed) return
  response.write('data: [DONE]\n\n')
  finished = true
  response.end()
})

server.listen(port, '127.0.0.1', () => {
  console.log(`mock provider listening on http://127.0.0.1:${port}/v1 as ${model}`)
})
