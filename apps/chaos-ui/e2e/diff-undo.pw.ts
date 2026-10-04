import { expect, test, type Page } from '@playwright/test'
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises'
import { resolve } from 'node:path'

const workspaceRoot = resolve('e2e/fixtures/workspace')
const target = 'nested/diff-undo.txt'
const original = 'What was on disk before the browser rewrote it.\n'
const rewritten = 'Rewritten in the browser and approved on the host.\n'

const targetPath = () => resolve(workspaceRoot, target)
const readTarget = async () => readFile(targetPath(), 'utf8')

/** Opens the fixture file in the editor and waits for its current disk contents. */
async function openTarget(page: Page) {
  const files = page.locator('section[aria-label="文件浏览器"]')
  if (!(await files.isVisible())) {
    await page.getByRole('button', { name: '📁 文件' }).click()
  }
  // Back to the workspace root first: this helper runs twice in a test, and the
  // second time the listing is still sitting in `nested` from the first.
  await files.getByRole('button', { name: 'workspace', exact: true }).click()
  await files.getByRole('button', { name: '打开目录 nested' }).click()
  await files.getByRole('button', { name: `📄 ${target.split('/')[1]}` }).click()
  return page.getByLabel(`编辑文件内容 ${target}`)
}

/** Rewrites the file through the only path the browser has: propose, then approve. */
async function proposeAndApprove(page: Page, contents: string) {
  await page.getByRole('button', { name: '提议保存修改' }).click()
  const approval = page.locator('article[aria-label="工具审批"]')
  await expect(approval).toContainText('需要审批：workspace.write_file')
  await approval.getByRole('button', { name: '允许' }).click()
  await expect.poll(readTarget).toBe(contents)
}

async function openDiffTab(page: Page) {
  await page.getByRole('button', { name: /🔍 差异/ }).click()
  return page.locator('section[aria-label="代码差异审查"]')
}

test.beforeEach(async () => {
  await mkdir(resolve(workspaceRoot, 'nested'), { recursive: true })
  await writeFile(targetPath(), original)
})

test.afterEach(async () => {
  await rm(targetPath(), { force: true })
})

test('a landed write offers its own undo point and 回滚变更 puts the file back', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

  const editor = await openTarget(page)
  await expect(editor).toHaveValue(original)
  await editor.fill(rewritten)
  await proposeAndApprove(page, rewritten)

  // The write itself is what offers the proposal: nobody typed an id and nobody
  // pressed 加载差异, so an undo point that only existed after a manual lookup
  // would fail this assertion.
  await expect(page.getByRole('button', { name: /🔍 差异 \(1\)/ })).toBeVisible()
  const banner = page.locator('div.diff-alert')
  await expect(banner).toContainText(target)

  const diff = await openDiffTab(page)
  const panes = diff.locator('.diff-pane')
  await expect(diff.getByLabel('Diff 详细比对')).toBeVisible()
  await expect(diff.getByText(`目标文件：${target}`)).toBeVisible()
  await expect(panes.nth(0)).toContainText('before the browser rewrote it')
  await expect(panes.nth(1)).toContainText('Rewritten in the browser')

  await diff.getByRole('button', { name: /回滚变更/ }).click()
  await expect.poll(readTarget).toBe(original)
  // The tab has nothing left to offer, and the banner goes with it.
  await expect(diff.getByLabel('Diff 详细比对')).toHaveCount(0)
  await expect(banner).toHaveCount(0)
  await expect(page.getByRole('button', { name: /^🔍 差异$/ })).toBeVisible()

  // Reopened from disk rather than from the editor buffer: this is the assertion
  // that says the bytes really went back.
  await openTarget(page)
  await expect(page.getByLabel(`编辑文件内容 ${target}`)).toHaveValue(original)
})

test('接受变更 keeps the file and takes the undo point away', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

  const editor = await openTarget(page)
  await expect(editor).toHaveValue(original)
  await editor.fill(rewritten)
  await proposeAndApprove(page, rewritten)

  const diff = await openDiffTab(page)
  const proposalId = (await diff.getByText(/^提案 ID:/).innerText()).replace('提案 ID:', '').trim()
  expect(proposalId).not.toBe('')
  await diff.getByRole('button', { name: /接受变更/ }).click()
  await expect.poll(readTarget).toBe(rewritten)
  await expect(diff.getByLabel('Diff 详细比对')).toHaveCount(0)

  // The proposal this page just accepted is spent: asking for it again is refused
  // in words, and the file the user decided to keep stays exactly as it is.
  await diff.getByLabel('提案 ID').fill(proposalId)
  await diff.getByRole('button', { name: '加载差异' }).click()
  await expect(diff.getByRole('alert')).toContainText('提案不存在或已处理')
  await expect(readTarget()).resolves.toBe(rewritten)
})

test('the host refuses to roll back over an edit made outside the browser, and says why', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

  const editor = await openTarget(page)
  await editor.fill(rewritten)
  await proposeAndApprove(page, rewritten)

  // Somebody else touched the file: a later save, a git checkout, an editor left
  // open in another window. The undo point is no longer theirs to spend.
  await writeFile(targetPath(), 'Edited outside the browser after the write.\n')

  const diff = await openDiffTab(page)
  await diff.getByRole('button', { name: /回滚变更/ }).click()
  await expect(diff.getByRole('alert')).toContainText('在写入之后又被改过')
  await expect(readTarget()).resolves.toBe('Edited outside the browser after the write.\n')
  // The preview stays: the change it describes is still the thing the user is deciding about.
  await expect(diff.getByLabel('Diff 详细比对')).toBeVisible()

  // Fixing the reason lets the same proposal through, so the refusal is not a dead end.
  await writeFile(targetPath(), rewritten)
  await diff.getByRole('button', { name: /回滚变更/ }).click()
  await expect.poll(readTarget).toBe(original)
  await expect(diff.getByRole('alert')).toHaveCount(0)
})
