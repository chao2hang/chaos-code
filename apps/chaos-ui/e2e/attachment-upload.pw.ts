import { expect, test, type Page } from '@playwright/test'
import { UPLOAD_CHUNK_BYTES, UPLOAD_WINDOW } from '../src/attachments'
import { existsSync } from 'node:fs'
import { readFile, rm } from 'node:fs/promises'
import { resolve } from 'node:path'

const workspaceRoot = resolve('e2e/fixtures/workspace')

// Nine slices of the client's frame budget, more than the four the client is
// willing to have unacknowledged at once (`UPLOAD_WINDOW` in `src/attachments.ts`),
// so the transfer only finishes if the client both keeps its slices inside the
// window and refills it from the host's progress reports. Anything that fits in
// one window would pass with a client that ignored `attachment_progress`.
const UPLOAD_BYTES = 9 * 47 * 1024

function payload(token: string, size: number): Buffer {
  const line = `chaos-e2e ${token} attachment payload line\n`
  const repeats = Math.ceil(size / line.length)
  return Buffer.from(line.repeat(repeats).slice(0, size), 'ascii')
}

function token(): string {
  return `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`
}

async function openUploadPanel(page: Page) {
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
  await page.getByRole('button', { name: '📁 文件' }).click()
  await expect(page.getByTestId('upload-file-input')).toBeVisible()
}

/**
 * Records every text the upload status line has shown, in order.
 *
 * `正在上传` is a state the client can pass through and leave before the first
 * poll lands, so asserting on the live DOM would make this test measure how fast
 * the loopback WebSocket is. Each status change is its own task (one await per
 * host round trip), so a MutationObserver's microtask runs between them and sees
 * every distinct value -- the same reason `reconnect-snapshot.pw.ts` records
 * instead of polling.
 */
async function recordUploadStatuses(page: Page) {
  await page.addInitScript(() => {
    const seen: string[] = []
    ;(window as unknown as { __chaosUploadSeen: string[] }).__chaosUploadSeen = seen
    const watched = new WeakSet<Element>()
    const watchStatus = () => {
      const status = document.querySelector('[data-testid="upload-status"]')
      if (!status || watched.has(status)) return
      watched.add(status)
      const record = () => {
        const text = (status.textContent ?? '').trim()
        if (text && seen[seen.length - 1] !== text) seen.push(text)
      }
      new MutationObserver(record).observe(status, { childList: true, subtree: true, characterData: true })
      record()
    }
    new MutationObserver(watchStatus).observe(document, { childList: true, subtree: true })
    watchStatus()
  })
}

/** Status texts the status line has already displayed, oldest first. */
function uploadStatuses(page: Page): Promise<string[]> {
  return page.evaluate(() => (window as unknown as { __chaosUploadSeen: string[] }).__chaosUploadSeen)
}

/** Waits until a status containing `needle` was displayed; matches history, not the DOM. */
async function waitUploadStatusShown(page: Page, needle: string) {
  await expect
    .poll(async () => (await uploadStatuses(page)).some((text) => text.includes(needle)), {
      message: `上传状态里从未出现过「${needle}」`,
    })
    .toBe(true)
}

/**
 * Records the order the client put slices on the wire and the order the host's
 * progress reports came back, by wrapping the socket's own `send` and
 * `onmessage` before the app takes it.
 *
 * `uploadWindowSlices` is what stops a browser from putting a whole 10 MiB
 * attachment -- 218 slices -- on one socket at once; the host answers frames past
 * its in-flight limit with `too_many_in_flight`, so a client that ignored the
 * window would fail a legal upload. Only the wire order can show that, and the
 * host acknowledges each slice it stages, so this observes both directions.
 */
async function recordWireOrder(page: Page) {
  await page.addInitScript(() => {
    const wire: string[] = []
    ;(window as unknown as { __chaosWire: string[] }).__chaosWire = wire
    const note = (text: string, mark: string) => {
      if (text.includes(mark)) wire.push(mark === '"attachment_chunk"' ? 'c' : 'p')
    }
    const socketSend = WebSocket.prototype.send
    WebSocket.prototype.send = function (data: unknown) {
      if (typeof data === 'string') note(data, '"attachment_chunk"')
      return (socketSend as (this: WebSocket, body: unknown) => void).call(this, data)
    }
    const onmessage = Object.getOwnPropertyDescriptor(WebSocket.prototype, 'onmessage')
    if (!onmessage?.set) throw new Error('WebSocket.prototype.onmessage 不是可写属性，无法记录线序')
    Object.defineProperty(WebSocket.prototype, 'onmessage', {
      configurable: true,
      enumerable: onmessage.enumerable,
      get(this: WebSocket) {
        return onmessage.get?.call(this)
      },
      set(this: WebSocket, handler: ((event: MessageEvent) => void) | null) {
        const wrapped = function (this: WebSocket, event: MessageEvent) {
          if (typeof event.data === 'string') note(event.data, '"attachment_progress"')
          return handler?.apply(this, [event])
        }
        onmessage.set!.call(this, wrapped)
      },
    })
  })
}

/** The recorded wire order, one character per slice sent (`c`) and progress report received (`p`). */
function wireOrder(page: Page): Promise<string[]> {
  return page.evaluate(() => (window as unknown as { __chaosWire: string[] }).__chaosWire)
}

test('an approved browser upload writes the file into the workspace', async ({ page }) => {
  const unique = token()
  const name = `upload-${unique}.txt`
  const relativePath = `nested/${name}`
  const contents = payload(unique, UPLOAD_BYTES)
  const onDisk = resolve(workspaceRoot, relativePath)

  await rm(onDisk, { force: true })
  try {
    // The upload has to create the file, otherwise the byte comparison is vacuous.
    expect(existsSync(onDisk)).toBe(false)

    await recordWireOrder(page)
    await openUploadPanel(page)
    await page.getByTestId('upload-file-input').setInputFiles({ name, mimeType: 'text/plain', buffer: contents })
    await page.getByTestId('upload-target-path').fill(relativePath)

    const submit = page.getByTestId('upload-submit')
    await submit.click()
    await expect(submit).toBeDisabled()
    await expect(page.getByTestId('upload-status')).toContainText('附件已传完，等待审批写入')

    const approval = page.locator('article[aria-label="工具审批"]')
    await expect(approval).toContainText('需要审批：workspace.attach_attachment')
    await approval.getByRole('button', { name: '允许' }).click()

    await expect(page.getByTestId('upload-status')).toHaveText(`附件已写入 ${relativePath}（${UPLOAD_BYTES} 字节）`)
    await expect(approval).toHaveCount(0)
    await expect(submit).toBeEnabled()
    expect((await readFile(onDisk)).equals(contents)).toBe(true)

    // The window is what this payload measures. Between the host's progress
    // reports the client never had more than `UPLOAD_WINDOW` slices
    // unacknowledged -- a client that ignored the window would be refused partway
    // by the host's per-connection in-flight limit -- and it did fill the window,
    // so this is the shipped pacing and not one slice per round trip.
    const wire = await wireOrder(page)
    const slices = Math.ceil(UPLOAD_BYTES / UPLOAD_CHUNK_BYTES)
    expect(slices).toBeGreaterThan(UPLOAD_WINDOW)
    expect(wire.filter((mark) => mark === 'c')).toHaveLength(slices)
    // Every slice the host staged is reported, so the client can refill.
    expect(wire.filter((mark) => mark === 'p')).toHaveLength(slices)
    let unacknowledged = 0
    let peak = 0
    for (const mark of wire) {
      unacknowledged += mark === 'c' ? 1 : -1
      // Negative would mean the host reported bytes for a slice it was never sent.
      expect(unacknowledged).toBeGreaterThanOrEqual(0)
      expect(unacknowledged).toBeLessThanOrEqual(UPLOAD_WINDOW)
      peak = Math.max(peak, unacknowledged)
    }
    expect(peak).toBe(UPLOAD_WINDOW)

    // Second code path over the same bytes: content search and the file reader
    // can only report what is really on disk.
    await page.getByLabel('文件搜索关键词').fill(unique)
    await page.getByRole('button', { name: '搜索文件' }).click()
    const hit = page.locator('button.file-item-btn').filter({ hasText: relativePath })
    await expect(hit).toHaveCount(1)
    await hit.click()
    await expect(page.locator('section[aria-label="代码编辑器"] h2')).toHaveText(`文件编辑：${relativePath}`)
    await expect(page.locator('textarea[aria-label^="编辑文件内容"]')).toContainText(unique)
  } finally {
    await rm(onDisk, { force: true })
  }
})

test('a cancelled upload is abandoned before the approval can write it', async ({ page }) => {
  const unique = token()
  const name = `cancel-${unique}.txt`
  const contents = payload(unique, 4 * 1024)
  const onDisk = resolve(workspaceRoot, name)

  await rm(onDisk, { force: true })
  try {
    expect(existsSync(onDisk)).toBe(false)

    await recordUploadStatuses(page)
    await openUploadPanel(page)
    await page.getByTestId('upload-file-input').setInputFiles({ name, mimeType: 'text/plain', buffer: contents })
    await page.getByTestId('upload-submit').click()
    // `正在上传` means the host handed back an upload id, so the cancel below
    // abandons a real transfer rather than a not-yet-started one. Read from the
    // recorded sequence: a 4 KiB file is one slice, and by the time the DOM is
    // next inspected the line has already moved on to `附件已传完`.
    await waitUploadStatusShown(page, '正在上传')
    await page.getByTestId('upload-cancel').click()
    await expect(page.getByTestId('upload-status')).toHaveText(`上传已取消：${name}`)
    await expect(page.getByTestId('upload-cancel')).toHaveCount(0)

    const approval = page.locator('article[aria-label="工具审批"]')
    await expect(approval).toContainText('需要审批：workspace.attach_attachment')
    await approval.getByRole('button', { name: '拒绝' }).click()
    await expect(page.locator('article[aria-label="工具审批"]')).toHaveCount(0)
    await expect(page.getByTestId('upload-status')).toHaveText(`上传已取消：${name}`)

    expect(existsSync(onDisk)).toBe(false)
    await page.waitForTimeout(300)
    expect(existsSync(onDisk)).toBe(false)
  } finally {
    await rm(onDisk, { force: true })
  }
})

test('the host refuses an attachment type it does not accept', async ({ page }) => {
  const unique = token()
  const name = `reject-${unique}.exe`
  const onDisk = resolve(workspaceRoot, name)

  await rm(onDisk, { force: true })
  try {
    expect(existsSync(onDisk)).toBe(false)

    await openUploadPanel(page)
    await page.getByTestId('upload-file-input').setInputFiles({ name, mimeType: 'application/octet-stream', buffer: payload(unique, 1024) })
    await page.getByTestId('upload-submit').click()

    await expect(page.getByTestId('upload-status')).toHaveText(`上传失败：${name}`)
    await expect(page.getByRole('alert')).toContainText('上传失败：')
    await expect(page.getByTestId('session-status')).toHaveText('上传失败')
    await expect(page.getByTestId('upload-submit')).toBeEnabled()
    expect(existsSync(onDisk)).toBe(false)
  } finally {
    await rm(onDisk, { force: true })
  }
})
