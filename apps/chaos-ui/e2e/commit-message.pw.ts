import { expect, test, type Page } from '@playwright/test'
import { readFileSync, rmSync, writeFileSync } from 'node:fs'
import { git, gitAlert, gitPanel, gitTab, messageBox, resetRepository, suggestButton } from './support/commit-form'
import { holdFile, promptLog, providerReply } from './support/paths'
import { withSidebar, workspaceButton } from './support/shell'

// The commit form on the Git tab, driven against a real repository, a real Web
// host and a self-built inference endpoint. The disk is read back after every
// step: what a commit form is worth is measured in `git log`, not in a screenshot.

async function openCommitForm(page: Page) {
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
  // A commit form that does not say which repository it will write to is a
  // guessing game, so the session's own workspace has to be in the sidebar first.
  await withSidebar(page, async () => {
    await expect(workspaceButton(page, '默认工作区')).toBeVisible()
  })
  await gitTab(page).click()
  await expect(gitPanel(page)).toBeVisible()
  await page.getByLabel('Git 操作类型').selectOption('commit')
  await expect(messageBox(page)).toHaveValue('')
}

function promptsReceived() {
  return readFileSync(promptLog, 'utf8')
    .split('\n')
    .filter((line) => line.trim())
    .map((line) => JSON.parse(line) as { model: string; messages: { role: string; content: string }[] })
}

test.beforeEach(() => {
  resetRepository()
  rmSync(holdFile, { force: true })
  writeFileSync(promptLog, '')
})

test('a suggested message fills the commit form and only an approved commit writes', async ({ page }) => {
  await openCommitForm(page)

  await suggestButton(page).click()
  // The endpoint answers in fenced deltas, so reaching this text means the host
  // stitched the stream and stripped the wrapping before offering it.
  await expect(messageBox(page)).toHaveValue(providerReply)
  await expect(page.getByTestId('apply-commit-suggestion')).toBeDisabled()
  expect(git(['log', '-1', '--format=%s'])).toBe('base')
  expect(git(['status', '--porcelain=v1'])).toContain('M  note.txt')

  const prompts = promptsReceived()
  expect(prompts).toHaveLength(1)
  const sent = prompts[0].messages[0].content
  expect(sent).toContain('+第二版说明')
  expect(sent).toContain('---START STAGED DIFF---')
  expect(sent).toContain('当前分支：')

  // The commit is still the approval-gated path, and it commits what the user
  // left in the box rather than what the Provider proposed.
  await messageBox(page).fill('chore: 我自己写的提交信息')
  await page.getByRole('button', { name: '执行 Git 操作' }).click()
  const approval = page.locator('article[aria-label="工具审批"]')
  await expect(approval).toContainText('需要审批：git.commit')
  await approval.getByRole('button', { name: '允许' }).click()
  await expect(approval).toContainText('这是第二次确认')
  await approval.getByRole('button', { name: '允许' }).click()
  await expect(page.getByTestId('session-status')).toHaveText('Git commit 执行完成')
  expect(git(['log', '-1', '--format=%s'])).toBe('chore: 我自己写的提交信息')
})

test('an empty staged area is refused before the endpoint is asked', async ({ page }) => {
  git(['restore', '--staged', '--', 'note.txt'])
  await openCommitForm(page)

  await suggestButton(page).click()
  await expect(gitAlert(page)).toContainText('暂存区为空')
  expect(readFileSync(promptLog, 'utf8')).toBe('')
  await expect(messageBox(page)).toHaveValue('')
  expect(git(['log', '-1', '--format=%s'])).toBe('base')
})

test('the commit form fits the screen it is shown on', async ({ page }) => {
  await openCommitForm(page)
  await suggestButton(page).click()
  await expect(messageBox(page)).toHaveValue(providerReply)

  // Horizontal page overflow is how a form becomes unusable on a phone, so the
  // document may not grow wider than the window and no control may sit off it.
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)
  expect(overflow).toBeLessThanOrEqual(0)
  const width = page.viewportSize()?.width ?? 0
  expect(width).toBeGreaterThan(0)
  for (const control of [
    messageBox(page),
    suggestButton(page),
    page.getByTestId('apply-commit-suggestion'),
    page.getByRole('button', { name: '执行 Git 操作' }),
  ]) {
    const box = await control.boundingBox()
    expect(box, 'control is laid out').not.toBeNull()
    expect(box!.x).toBeGreaterThanOrEqual(0)
    expect(box!.x + box!.width).toBeLessThanOrEqual(width + 1)
    expect(box!.height).toBeGreaterThan(0)
  }
})

test('a suggestion that lands after the user typed is offered beside the box', async ({ page }) => {
  // The endpoint holds its reply until this file is removed, so the user's typing
  // is guaranteed to land between the request and the answer.
  writeFileSync(holdFile, '')
  await openCommitForm(page)

  await suggestButton(page).click()
  await expect(suggestButton(page)).toContainText('建议生成中')
  await messageBox(page).fill('我自己的草稿')
  rmSync(holdFile, { force: true })

  await expect(page.getByTestId('commit-suggestion-offer')).toContainText(providerReply)
  await expect(messageBox(page)).toHaveValue('我自己的草稿')
  await page.getByTestId('apply-commit-suggestion').click()
  await expect(messageBox(page)).toHaveValue(providerReply)
  expect(git(['log', '-1', '--format=%s'])).toBe('base')
})
