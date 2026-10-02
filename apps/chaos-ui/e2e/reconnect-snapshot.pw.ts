import { expect, test, type Page } from '@playwright/test'
import { spawn, type ChildProcess } from 'node:child_process'
import { once } from 'node:events'
import { mkdir, mkdtemp, rm } from 'node:fs/promises'
import { createServer } from 'node:net'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'

type Frame = Record<string, unknown> & { type?: string; code?: string }

// Structural stand-in for Playwright's WebSocketRoute, so the capture list can be
// typed without reaching for a type @playwright/test does not re-export.
type SocketLeg = { close: (options?: { code?: number; reason?: string }) => Promise<void> }

type CapturedLeg = { outbound: Frame[]; inbound: Frame[]; pageSide?: SocketLeg }

async function availablePort(): Promise<number> {
  const server = createServer()
  server.listen(0, '127.0.0.1')
  await once(server, 'listening')
  const address = server.address()
  if (!address || typeof address === 'string') throw new Error('Could not determine an ephemeral TCP port')
  const { port } = address
  await new Promise<void>((close, fail) => server.close((error) => (error ? fail(error) : close())))
  return port
}

function parseFrame(raw: string | Buffer): Frame {
  try {
    return JSON.parse(String(raw)) as Frame
  } catch {
    return { type: `unparseable:${String(raw).slice(0, 40)}` }
  }
}

function framesOfType(frames: Frame[], type: string): Frame[] {
  return frames.filter((frame) => frame.type === type)
}

// Recovery flips the badge through 连接断开，正在重连 -> 已连接 -> 历史已恢复 inside
// about half a second, so polling the rendered text can miss the middle of it.
// Recording every value the badge ever held turns the transient state into a fact
// the test reads instead of a race it has to win.
async function recordStatusBadge(page: Page) {
  await page.addInitScript(() => {
    const seen: string[] = []
    ;(window as unknown as { __chaosStatusSeen: string[] }).__chaosStatusSeen = seen
    const watched = new WeakSet<Element>()
    const watchBadge = () => {
      const badge = document.querySelector('[data-testid="session-status"]')
      if (!badge || watched.has(badge)) return
      watched.add(badge)
      const record = () => {
        const text = (badge.textContent ?? '').trim()
        if (text && seen[seen.length - 1] !== text) seen.push(text)
      }
      new MutationObserver(record).observe(badge, { childList: true, subtree: true, characterData: true })
      record()
    }
    new MutationObserver(watchBadge).observe(document, { childList: true, subtree: true })
    watchBadge()
  })
}

function statusSequence(page: Page): Promise<string[]> {
  return page.evaluate(() => (window as unknown as { __chaosStatusSeen: string[] }).__chaosStatusSeen)
}

async function sendPrompt(page: Page, marker: string) {
  await page.getByTestId('composer-input').fill(marker)
  await page.getByTestId('composer-submit').click()
  await expect(page.locator('.user p').last()).toContainText(marker)
  await expect(page.locator('.assistant p').last()).toContainText(marker)
}

// Playwright holds the browser's side of every routed WebSocket. Closing that
// side is what the page observes when the host goes away -- closing the
// server-side leg instead does not reach the page at all, which was checked
// before writing this. The page is never reloaded, so everything after the close
// is the application's own recovery.
async function captureSockets(page: Page, rewriteResumeTo?: string) {
  const legs: CapturedLeg[] = []
  await page.routeWebSocket((url) => url.pathname === '/ws', (route) => {
    const leg: CapturedLeg = { outbound: [], inbound: [] }
    legs.push(leg)
    const server = route.connectToServer()
    if (legs.length === 1) leg.pageSide = route
    route.onMessage((message) => {
      const frame = parseFrame(message)
      if (legs.length > 1 && rewriteResumeTo && frame.type === 'resume') {
        leg.outbound.push({ ...frame, session_id: rewriteResumeTo })
        server.send(JSON.stringify({ ...frame, session_id: rewriteResumeTo }))
        return
      }
      leg.outbound.push(frame)
      server.send(String(message))
    })
    server.onMessage((message) => {
      leg.inbound.push(parseFrame(message))
      route.send(String(message))
    })
  })
  return legs
}

async function waitForHost(request: Page['request'], origin: string, stderr: () => string) {
  let lastError: unknown
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try {
      const response = await request.get(`${origin}/health`, { timeout: 500 })
      if (response.ok()) return
      lastError = `status ${response.status()}`
    } catch (error) {
      lastError = error
    }
    await new Promise((delay) => setTimeout(delay, 100))
  }
  throw new Error(`Web host at ${origin} never became healthy: ${String(lastError)} ${stderr()}`)
}

test('a dropped browser socket reconnects and restores the transcript from a server snapshot', async ({ page }) => {
  const marker = `reconnect marker ${Date.now().toString(36)}`
  await recordStatusBadge(page)
  const legs = await captureSockets(page)

  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
  await sendPrompt(page, marker)

  await expect.poll(() => framesOfType(legs[0].inbound, 'session_created').length).toBe(1)
  const sessionId = framesOfType(legs[0].inbound, 'session_created')[0].session_id
  expect(typeof sessionId).toBe('string')

  await legs[0].pageSide?.close({ code: 1011, reason: 'simulated host restart' })
  await expect.poll(() => statusSequence(page)).toContain('连接断开，正在重连')

  await expect.poll(() => legs.length).toBe(2)
  await expect(page.getByTestId('session-status')).toHaveText('历史已恢复')

  // Recovery asks the host to resume the session it already had rather than
  // quietly starting a fresh one.
  const resumes = framesOfType(legs[1].outbound, 'resume')
  expect(resumes, 'the browser must resume the session it held before the drop').toHaveLength(1)
  expect(resumes[0].session_id).toBe(sessionId)
  expect(framesOfType(legs[1].outbound, 'create_session')).toHaveLength(0)

  // The transcript shown after the reconnect came back from the host, not from
  // whatever the page happened to keep in memory.
  const snapshots = framesOfType(legs[1].inbound, 'session_snapshot')
  expect(snapshots, 'the host answered the resume with a snapshot').toHaveLength(1)
  const restored = snapshots[0].messages as { role: string; text: string }[]
  expect(restored.map((message) => message.text)).toContain(marker)
  await expect(page.locator('.user p').first()).toContainText(marker)
  await expect(page.locator('.assistant p').first()).toContainText(marker)

  // The reconnected socket is usable, and it is still the same session.
  const followUp = `follow-up after reconnect ${Date.now().toString(36)}`
  await sendPrompt(page, followUp)
  expect(framesOfType(legs[1].outbound, 'submit').map((frame) => frame.session_id)).toEqual([sessionId])
  await expect.poll(() => framesOfType(legs[1].inbound, 'completed').length).toBe(1)
  expect(framesOfType(legs[1].outbound, 'create_session')).toHaveLength(0)
})

test('a resume the host cannot answer is refused visibly and leaves a usable session behind', async ({ page }) => {
  // The browser holding a session id the (restarted) host no longer knows: the
  // reconnect is genuine, only the id it carries is stale.
  const staleSessionId = '00000000-0000-4000-8000-000000000001'
  await recordStatusBadge(page)
  const legs = await captureSockets(page, staleSessionId)

  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
  const sessionId = framesOfType(legs[0].inbound, 'session_created')[0]?.session_id
  expect(typeof sessionId).toBe('string')

  await legs[0].pageSide?.close({ code: 1011, reason: 'simulated host restart' })
  await expect.poll(() => legs.length).toBe(2)

  const resumes = framesOfType(legs[1].outbound, 'resume')
  expect(resumes).toHaveLength(1)
  expect(resumes[0].session_id).toBe(staleSessionId)

  // The host really answered, and an unknown session is a refusal rather than
  // an empty history.
  await expect
    .poll(() => framesOfType(legs[1].inbound, 'error').map((frame) => frame.code))
    .toContain('session_not_found')
  expect(framesOfType(legs[1].inbound, 'session_snapshot')).toHaveLength(0)
  await expect.poll(() => statusSequence(page)).toContain('请求错误')

  // And the refusal is not a dead end: the client asks for a session the new
  // host can answer, so the composer keeps working instead of pointing at an id
  // nothing will ever reply to.
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
  const replacement = framesOfType(legs[1].inbound, 'session_created')[0]?.session_id
  expect(typeof replacement).toBe('string')
  expect(replacement).not.toBe(sessionId)

  const marker = `prompt after a refused resume ${Date.now().toString(36)}`
  await sendPrompt(page, marker)
  expect(framesOfType(legs[1].outbound, 'submit').map((frame) => frame.session_id)).toEqual([replacement])
})

test('a real Web host restart reconnects, reports the lost session and becomes usable again', async ({ page, request }) => {
  const marker = `host restart marker ${Date.now().toString(36)}`
  const repositoryRoot = resolve('..', '..')
  const tempRoot = await mkdtemp(join(tmpdir(), 'chaos-host-restart-'))
  const port = await availablePort()
  const origin = `http://127.0.0.1:${port}`
  const children: ChildProcess[] = []
  let output = ''

  const startHost = () => {
    const child = spawn(resolve(repositoryRoot, 'target/debug/chaos-web'), [], {
      cwd: repositoryRoot,
      env: {
        ...process.env,
        CHAOS_WEB_PORT: String(port),
        CHAOS_WEB_DEV_ORIGIN: origin,
        CHAOS_WEB_ALLOW_DYNAMIC_DEV_ORIGIN: '1',
        CHAOS_WEB_TOKEN: '',
        CHAOS_WEB_ASSETS_DIR: resolve('dist'),
        CHAOS_WORKSPACE_ROOT: join(tempRoot, 'workspace'),
        CHAOS_SAFE_WEB_MODE: '0',
      },
      stdio: ['ignore', 'ignore', 'pipe'],
    })
    child.stderr?.on('data', (chunk: Buffer) => { output += chunk.toString() })
    children.push(child)
    return child
  }

  try {
    await recordStatusBadge(page)
    await mkdir(join(tempRoot, 'workspace'), { recursive: true })
    const firstHost = startHost()
    await waitForHost(request, origin, () => output)

    await page.goto(origin)
    await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
    await sendPrompt(page, marker)

    // The fault is a real one: the process, its sockets and its in-memory
    // sessions all go away. Nothing is mocked here.
    firstHost.kill('SIGKILL')
    await once(firstHost, 'exit')
    await expect(page.getByTestId('session-status')).toHaveText('连接断开，正在重连')

    const secondHost = startHost()
    await waitForHost(request, origin, () => output)
    expect(secondHost.pid).not.toBe(firstHost.pid)

    // The page retries on its own and the new host refuses the session the old
    // one had, which is reported rather than passed off as an empty history.
    await expect.poll(() => statusSequence(page)).toContain('请求错误')
    expect(await statusSequence(page)).not.toContain('历史已恢复')
    await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

    // Usable again against the restarted host.
    const followUp = `prompt after restart ${Date.now().toString(36)}`
    await sendPrompt(page, followUp)
    // The reply above can only have come from the replacement process: the
    // original one was reaped before the page ever sent the prompt. A process
    // stopped by a signal reports exitCode null and signalCode set, so "has an
    // exitCode" would prove nothing here.
    expect(firstHost.signalCode, 'the original host stayed alive').toBe('SIGKILL')
    expect(secondHost.exitCode, 'the replacement host died').toBeNull()
    expect(secondHost.signalCode, 'the replacement host died').toBeNull()
  } finally {
    for (const child of children) if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL')
    await rm(tempRoot, { recursive: true, force: true })
  }
})
