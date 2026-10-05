import { expect, test, type Page } from '@playwright/test'
import { readFileSync, rmSync, writeFileSync } from 'node:fs'
import { cutLog, paceFile } from './support/paths'

// What a turn used to be: the host held the whole answer, so a page waited on one
// silent gap and then got everything at once, and 停止 booked the next turn instead
// of ending the one on screen. These specs drive the shipped page against the
// shipped host against a self-built endpoint that answers slowly on request, and
// read the same answer back from both ends -- the DOM, and the endpoint's own record
// of how much it got to write before the host hung up.
//
// It runs on the provider config (`playwright.git.config.ts`) for that reason: the
// demo responder on the other config answers a prompt in one tight loop, and a
// producer that is already finished cannot be interrupted or watched mid-sentence.

// Plain prose, so the rendered text can be compared to the sent text character for
// character rather than through a guess about how Markdown would reflow it.
const STREAM_TEXT = '第一段说明正在被逐字送到页面上。第二段还在路上没有到。第三段要等前两段都画完才开始。第四段最后到，画完这一轮就结束了。'

/** Puts the endpoint in slow mode for the answers that follow. */
function writePace(text: string, { frameChars, gapMs }: { frameChars: number; gapMs: number }) {
  writeFileSync(paceFile, JSON.stringify({ text, frameChars, gapMs }))
}

/** Answers the endpoint had to stop writing because the host closed the request. */
function cutsTaken() {
  return readFileSync(cutLog, 'utf8')
    .split('\n')
    .filter((line) => line.trim())
    .map((line) => JSON.parse(line) as { model: string; sent: string })
}

// The bubble a send leaves behind reads 正在生成… until the first chunk arrives. That
// placeholder is the page's own words, not part of the answer.
const PLACEHOLDER = '正在生成…'

async function answerText(page: Page) {
  const body = page.locator('.assistant .markdown-body').last()
  if ((await body.count()) === 0) return ''
  const text = (await body.textContent()) ?? ''
  return text === PLACEHOLDER ? '' : text
}

async function sendPrompt(page: Page, prompt: string) {
  await page.getByTestId('composer-input').fill(prompt)
  await page.getByTestId('composer-submit').click()
  await expect(page.locator('.user p').last()).toContainText(prompt)
}

test.beforeEach(() => {
  rmSync(paceFile, { force: true })
  writeFileSync(cutLog, '')
})

test.afterEach(() => {
  // The endpoint and the host outlive one test; a pace file left behind would make
  // the commit specs in this same config wait on a slow answer they never asked for.
  rmSync(paceFile, { force: true })
})

test.beforeEach(async ({ page }) => {
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
})

test('an answer reaches the page while the endpoint is still producing it', async ({ page }) => {
  writePace(STREAM_TEXT, { frameChars: 3, gapMs: 140 })
  await sendPrompt(page, `慢慢说 ${Date.now().toString(36)}`)

  const samples: string[] = []
  let stopOfferedWhilePartial: boolean | undefined
  const deadline = Date.now() + 20_000
  let complete = false
  while (Date.now() < deadline && !complete) {
    const text = await answerText(page)
    if (text && samples[samples.length - 1] !== text) {
      samples.push(text)
      if (stopOfferedWhilePartial === undefined) {
        // Recorded at the first unfinished sample: 停止 has to be there while chunks
        // are still arriving, which is the moment a person would want it.
        stopOfferedWhilePartial = await page.getByTestId('composer-stop').isVisible()
      }
    }
    complete = text === STREAM_TEXT
    await page.waitForTimeout(35)
  }

  expect(complete, `the answer never finished arriving: ${JSON.stringify(samples)}`).toBe(true)
  // Not one paint carrying the whole reply: the page was shown an unfinished answer
  // repeatedly, and each time it was a longer prefix than the one before.
  expect(samples.length, `the answer arrived in ${samples.length} steps: ${JSON.stringify(samples)}`).toBeGreaterThanOrEqual(4)
  for (const sample of samples) expect(STREAM_TEXT.startsWith(sample), sample).toBe(true)
  for (let index = 1; index < samples.length; index += 1) {
    expect(samples[index].length, samples[index]).toBeGreaterThan(samples[index - 1].length)
  }
  expect(stopOfferedWhilePartial, '停止 was not offered while the answer was still arriving').toBe(true)

  // It completed, so the turn closes without an outcome line saying otherwise, and
  // the composer is the user's again.
  await expect(page.getByTestId('composer-stop')).toHaveCount(0)
  await expect(page.locator('.turn-outcome')).toHaveCount(0)
})

test('停止 ends the answer that is still arriving', async ({ page }) => {
  writePace(STREAM_TEXT, { frameChars: 2, gapMs: 200 })
  await sendPrompt(page, `说个长的 ${Date.now().toString(36)}`)

  await expect.poll(() => answerText(page), { timeout: 10_000 }).not.toBe('')
  await expect(page.getByTestId('composer-stop')).toBeVisible()
  await page.getByTestId('composer-stop').click()

  await expect(page.locator('.turn-outcome').last()).toHaveText('本轮已取消。')
  await expect(page.getByTestId('composer-stop')).toHaveCount(0)

  const shown = await answerText(page)
  expect(STREAM_TEXT.startsWith(shown), shown).toBe(true)
  expect(shown.length, 'the whole answer arrived, so nothing was interrupted').toBeLessThan(STREAM_TEXT.length)
  // The turn is closed: nothing further is written into the bubble.
  await page.waitForTimeout(700)
  expect(await answerText(page)).toBe(shown)

  // The other end of the same turn. A stop that only changed what the page shows
  // would leave the endpoint writing an answer nobody is reading.
  const cuts = cutsTaken()
  expect(cuts.length, 'the endpoint never saw the host hang up').toBe(1)
  expect(STREAM_TEXT.startsWith(cuts[0].sent), cuts[0].sent).toBe(true)
  expect(cuts[0].sent.length).toBeGreaterThan(0)
  expect(cuts[0].sent.length, cuts[0].sent).toBeLessThan(STREAM_TEXT.length)

  // The session belongs to the user again: the next prompt starts a real turn.
  const next = `接着说一句 ${Date.now().toString(36)}`
  await page.getByTestId('composer-input').fill(next)
  await expect(page.getByTestId('composer-submit')).toBeEnabled()
  await page.getByTestId('composer-submit').click()
  await expect(page.locator('.user p').last()).toContainText(next)
  await expect(page.getByTestId('composer-stop')).toBeVisible()
})
