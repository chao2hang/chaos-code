import { expect, test, type Page } from '@playwright/test'

type Outgoing = { type?: string; client_msg_id?: string }

const uuidV4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/

// `crypto.randomUUID` is an accessor inherited from `Crypto.prototype`, so shadowing it with an
// own property on the instance is what removes it from the page's view, and `addInitScript` runs
// before any script the app ships, so the app never gets to observe the method existing.
// `getRandomValues` is deliberately left working, because a plain-HTTP origin over a routable
// address keeps it while losing `randomUUID`; that combination is the shape of the real bug.
async function hideRandomUuid(page: Page, { alsoHideGetRandomValues = false, throwOnCall = false } = {}) {
  await page.addInitScript(({ hideFiller, throws }) => {
    Object.defineProperty(window.crypto, 'randomUUID', {
      configurable: true,
      value: throws ? () => { throw new Error('randomUUID refused by the embedder') } : undefined,
    })
    if (hideFiller) Object.defineProperty(window.crypto, 'getRandomValues', { configurable: true, value: undefined })
  }, { hideFiller: alsoHideGetRandomValues, throws: throwOnCall })
}

function captureOutgoing(page: Page, sink: Outgoing[]) {
  page.on('websocket', (webSocket) => webSocket.on('framesent', ({ payload }) => {
    if (typeof payload !== 'string') return
    try {
      sink.push(JSON.parse(payload) as Outgoing)
    } catch {
      // A non-JSON frame is not a client message; the assertion reads the ones that are.
    }
  }))
}

test('a page with no crypto.randomUUID still sends the prompt and settles the turn', async ({ page }) => {
  // 127.0.0.1 is a secure context, so the missing method has to be manufactured here; the
  // routable plain-HTTP origin this stands in for was measured to report
  // `isSecureContext=false randomUUID=undefined getRandomValues=function`.
  const outgoing: Outgoing[] = []
  const errors: string[] = []
  page.on('pageerror', (error) => errors.push(String(error)))
  await hideRandomUuid(page)
  captureOutgoing(page, outgoing)

  await page.goto('/')
  expect(await page.evaluate(() => typeof globalThis.crypto.randomUUID)).toBe('undefined')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

  const booted = outgoing.filter((message) => message.type === 'list_workspaces' || message.type === 'create_session')
  expect(booted.length, 'the boot sequence must get its messages out').toBeGreaterThanOrEqual(2)
  for (const message of booted) expect(message.client_msg_id).toMatch(uuidV4)

  const marker = `insecure origin ${Date.now().toString(36)}`
  await page.getByTestId('composer-input').fill(marker)
  await page.getByTestId('composer-submit').click()
  await expect(page.locator('.user p').first()).toContainText(marker)
  await expect(page.locator('.assistant').last()).toContainText(marker)

  const submitted = outgoing.filter((message) => message.type === 'submit')
  expect(submitted).toHaveLength(1)
  expect(submitted[0].client_msg_id).toMatch(uuidV4)
  expect(errors, `the page must not throw: ${errors.join('\n')}`).toEqual([])
})

test('a randomUUID that exists but throws still delivers the message', async ({ page }) => {
  // An embedder (privacy extension, hardened browser build) can define the method and refuse it.
  // Losing the message while the badge reads connected is the failure this guards.
  const outgoing: Outgoing[] = []
  const errors: string[] = []
  page.on('pageerror', (error) => errors.push(String(error)))
  await hideRandomUuid(page, { throwOnCall: true })
  captureOutgoing(page, outgoing)

  await page.goto('/')
  expect(await page.evaluate(() => typeof globalThis.crypto.randomUUID)).toBe('function')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

  const marker = `throwing randomUUID ${Date.now().toString(36)}`
  await page.getByTestId('composer-input').fill(marker)
  await page.getByTestId('composer-submit').click()
  await expect(page.locator('.assistant').last()).toContainText(marker)
  expect(outgoing.filter((message) => message.type === 'submit')).toHaveLength(1)
  expect(errors, `the page must not throw: ${errors.join('\n')}`).toEqual([])
})

test('a host with neither WebCrypto method still sends distinct ids', async ({ page }) => {
  // No WebCrypto at all: the sequenced id is the last path left, and two messages in one page
  // must still be tellable apart by the host.
  const outgoing: Outgoing[] = []
  await hideRandomUuid(page, { alsoHideGetRandomValues: true })
  captureOutgoing(page, outgoing)

  await page.goto('/')
  expect(await page.evaluate(() => [typeof globalThis.crypto.randomUUID, typeof globalThis.crypto.getRandomValues])).toEqual(['undefined', 'undefined'])
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

  const marker = `no webcrypto ${Date.now().toString(36)}`
  await page.getByTestId('composer-input').fill(marker)
  await page.getByTestId('composer-submit').click()
  await expect(page.locator('.assistant').last()).toContainText(marker)
  await page.getByTestId('composer-input').fill(`${marker} again`)
  await page.getByTestId('composer-submit').click()
  await expect(page.locator('.assistant').last()).toContainText(`${marker} again`)

  const ids = outgoing.flatMap((message) => typeof message.client_msg_id === 'string' ? [message.client_msg_id] : [])
  expect(ids.length).toBeGreaterThanOrEqual(4)
  expect(new Set(ids).size, 'every message needs its own id').toBe(ids.length)
  for (const id of ids) expect(id).toMatch(uuidV4)
})
