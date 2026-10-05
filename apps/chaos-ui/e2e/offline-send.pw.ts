import { expect, test, type Page } from '@playwright/test'
import { headerTab, recordStatusBadge, statusSequence } from './support/shell'

// Structural stand-in for Playwright's WebSocketRoute, so a held leg can be typed without
// reaching for a type @playwright/test does not re-export.
type SocketLeg = { close: (options?: { code?: number; reason?: string }) => Promise<void> }

// The wording a refused send is allowed to land on. With the socket down it depends on which
// reconnect attempt the click lands inside, and the page retries every 500 ms; the property
// under test is that a reason appears at all, because before this the badge kept reading as
// though nothing had been asked of it.
const socketDownReasons = ['连接已断开，消息未发送', '正在连接，消息未发送', '尚未连接，消息未发送']
const noSessionReasons = ['会话尚未就绪，消息未发送']

async function refusedSockets(page: Page, { firstLegHolds = false } = {}) {
  const legs: SocketLeg[] = []
  await page.routeWebSocket((url) => url.pathname === '/ws', (route) => {
    if (!firstLegHolds || legs.length > 0) {
      route.close({ code: 1013, reason: 'simulated host outage' })
      return
    }
    legs.push(route)
    const server = route.connectToServer()
    route.onMessage((message) => server.send(String(message)))
    server.onMessage((message) => route.send(String(message)))
  })
  return legs
}

async function expectARefusedSendIsVisible(page: Page, marker: string, allowed: string[]) {
  // At phone width the details panel is a full-screen drawer covering the composer, so a person
  // goes back to 对话 before typing. The refusal is about to be asserted on the composer, and the
  // drawer has to be out of the way for the tap to be one a phone can actually make -- measured:
  // at 390x844 the panel is `position: fixed`, 390x844, z-index 100, and the composer's centre
  // point hit-tests to a `.file-item-btn` inside it.
  await headerTab(page, '💬 对话').click()
  await expect(page.locator('[data-shell-column="details"]')).toBeHidden()
  await page.getByTestId('composer-input').fill(marker)
  await page.getByTestId('composer-submit').click()
  await expect
    .poll(async () => (await statusSequence(page)).some((text) => allowed.includes(text)), { timeout: 10_000 })
    .toBe(true)
  // Nothing reached the host, so the transcript must not pretend otherwise. The draft staying
  // put is what lets the person send it again once the host is back.
  await expect(page.locator('.user p')).toHaveCount(0)
  await expect(page.getByTestId('composer-input')).toHaveValue(marker)
}

test('a prompt sent while the host has no session for the page is refused out loud', async ({ page }) => {
  await recordStatusBadge(page)
  await refusedSockets(page)

  await page.goto('/')
  await expect.poll(() => statusSequence(page)).toContain('连接断开，正在重连')
  await expectARefusedSendIsVisible(page, `prompt during an outage ${Date.now().toString(36)}`, noSessionReasons)
})

test('a prompt sent after the socket drops says the message did not go', async ({ page }) => {
  await recordStatusBadge(page)
  const legs = await refusedSockets(page, { firstLegHolds: true })

  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

  // Playwright holds the browser's side of the routed socket, and closing that side is what the
  // page observes when the host goes away; the retries are refused, so the socket stays down.
  await legs[0].close({ code: 1011, reason: 'simulated host restart' })
  await expect.poll(() => statusSequence(page)).toContain('连接断开，正在重连')

  // A request the page makes on its own, not from the composer: opening the file tab asks the
  // host for a directory listing. That send is refused too and has to say so.
  await headerTab(page, '📁 文件').click()
  await expect
    .poll(async () => (await statusSequence(page)).some((text) => socketDownReasons.includes(text)), { timeout: 10_000 })
    .toBe(true)

  await expectARefusedSendIsVisible(page, `prompt after a drop ${Date.now().toString(36)}`, socketDownReasons)
})
