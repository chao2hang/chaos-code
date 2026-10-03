import { expect, test, type Page } from '@playwright/test'
import { createServer } from 'node:net'
import { once } from 'node:events'

// A streamed answer is assembled from many small `text_delta` frames. The page keeps its
// own copy of the newest session state so the socket callback can read it between
// renders, and that copy must never be set back to what React last rendered: a frame can
// arrive after a render but before that render's effects run, and building the next frame
// on the older value silently drops the characters in between. These specs stream a long
// answer one frame at a time while the page is forced to do layout work, then require
// every character the host sent to still be on screen.

// 8-byte deltas, so this prompt arrives as roughly 60 frames.
const BODY = 'integrity probe paragraph 0123456789 '.repeat(16)

type SocketLeg = { close: (options?: { code?: number; reason?: string }) => Promise<void> }

async function availablePort(): Promise<number> {
  const server = createServer()
  server.listen(0, '127.0.0.1')
  await once(server, 'listening')
  const address = server.address()
  if (!address || typeof address === 'string') throw new Error('Could not determine an ephemeral TCP port')
  const { port } = address
  await new Promise<void>((closeServer, fail) => server.close((error) => (error ? fail(error) : closeServer())))
  return port
}

// Every DOM change makes the page read geometry and text back, which is what stretches
// the window between a render and the frame that lands during it.
async function forceLayoutOnEveryMutation(page: Page) {
  await page.addInitScript(() => {
    let scheduled = false
    new MutationObserver(() => {
      if (scheduled) return
      scheduled = true
      requestAnimationFrame(() => {
        scheduled = false
        const timeline = document.querySelector('.timeline')
        const body = document.querySelector('.assistant .markdown-body')
        void timeline?.getBoundingClientRect().height
        void body?.textContent?.length
      })
    }).observe(document, { childList: true, subtree: true, characterData: true })
  })
}

// Delays only the streamed frames, so each one is delivered as its own task and gets its
// own render. Frames that share a task cannot show a lost update.
async function paceStreamedFrames(page: Page) {
  const legs: { pageSide?: SocketLeg }[] = []
  await page.routeWebSocket((url) => url.pathname === '/ws', (route) => {
    const leg: { pageSide?: SocketLeg } = {}
    legs.push(leg)
    const server = route.connectToServer()
    if (legs.length === 1) leg.pageSide = route
    route.onMessage((message) => server.send(String(message)))
    server.onMessage((message) => {
      const raw = String(message)
      if (raw.includes('"text_delta"')) {
        setTimeout(() => route.send(raw), 1)
        return
      }
      route.send(raw)
    })
  })
  return legs
}

async function sendPrompt(page: Page, prompt: string) {
  await page.getByTestId('composer-input').fill(prompt)
  await page.getByTestId('composer-submit').click()
  await expect(page.locator('.user p').last()).toContainText(prompt)
}

async function renderedAnswer(page: Page) {
  return page.locator('.assistant .markdown-body').last().textContent()
}

test('a long streamed answer keeps every character the host sent', async ({ page }) => {
  const prompt = `${BODY}${Date.now().toString(36)}`
  await forceLayoutOnEveryMutation(page)
  await paceStreamedFrames(page)
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

  await sendPrompt(page, prompt)
  // The demo host answers a bare prompt with this exact echo, so the expected text is
  // known instead of reconstructed from what happened to render.
  await expect.poll(() => renderedAnswer(page), { timeout: 20_000 }).toBe(`演示响应：${prompt}`)
})

test('an answer streamed after a reconnect keeps every character the host sent', async ({ page }) => {
  await forceLayoutOnEveryMutation(page)
  const legs = await paceStreamedFrames(page)
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
  await sendPrompt(page, `first answer ${Date.now().toString(36)}`)
  await expect.poll(() => renderedAnswer(page), { timeout: 20_000 }).toContain('first answer')

  await legs[0].pageSide?.close({ code: 1011, reason: 'simulated host restart' })
  await expect(page.getByTestId('session-status')).toHaveText('历史已恢复')

  const prompt = `${BODY}${Date.now().toString(36)}`
  await sendPrompt(page, prompt)
  await expect.poll(() => renderedAnswer(page), { timeout: 20_000 }).toBe(`演示响应：${prompt}`)
})
