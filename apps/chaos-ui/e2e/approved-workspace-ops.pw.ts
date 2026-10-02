import { expect, test } from '@playwright/test'
import { execFileSync } from 'node:child_process'
import { mkdir, rm, writeFile } from 'node:fs/promises'
import { resolve } from 'node:path'

const workspaceRoot = resolve('e2e/fixtures/workspace')
const stagedFile = 'approved-git-stage.txt'

test('approved terminal and Git operations run against the configured workspace root', async ({ page }) => {
  await mkdir(workspaceRoot, { recursive: true })
  execFileSync('git', ['init', '-q', workspaceRoot])
  execFileSync('git', ['-C', workspaceRoot, 'config', 'user.name', 'Chaos E2E'])
  execFileSync('git', ['-C', workspaceRoot, 'config', 'user.email', 'chaos-e2e@example.invalid'])
  await writeFile(resolve(workspaceRoot, stagedFile), 'Approved staging fixture.\n')

  try {
    await page.goto('/')
    await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

    await page.getByRole('button', { name: '💻 终端' }).click()
    await page.getByLabel('终端执行命令').fill('pwd')
    await page.getByRole('button', { name: '运行命令' }).click()
    const terminalApproval = page.locator('article[aria-label="工具审批"]')
    await expect(terminalApproval).toContainText('需要审批：terminal.execute')
    await terminalApproval.getByRole('button', { name: '允许' }).click()
    await expect(page.locator('section[aria-label="终端控制台输出"]')).toContainText(workspaceRoot)
    await expect(page.locator('section[aria-label="终端控制台输出"]')).toContainText('退出码：0')
    await expect(page.getByTestId('session-status')).toHaveText('终端执行完成（退出码：0）')

    await page.getByRole('button', { name: '🌿 Git' }).click()
    await page.getByLabel('Git 参数').fill(stagedFile)
    await page.getByRole('button', { name: '执行 Git 操作' }).click()
    const gitApproval = page.locator('article[aria-label="工具审批"]')
    await expect(gitApproval).toContainText('需要审批：git.stage')
    await gitApproval.getByRole('button', { name: '允许' }).click()
    await expect(page.getByTestId('session-status')).toHaveText('Git stage 执行完成')
    await expect(page.locator('article[aria-label="工具审批"]')).toHaveCount(0)

    await page.getByRole('button', { name: '刷新 Git 状态' }).click()
    await expect(page.locator('.git-entries-list')).toContainText(stagedFile)
    await expect(page.getByTestId('session-status')).toHaveText('Git stage 执行完成')
    await expect(page.locator('.git-entries-list').locator('li').filter({ hasText: stagedFile })).toContainText(stagedFile)
  } finally {
    await rm(resolve(workspaceRoot, stagedFile), { force: true })
    await rm(resolve(workspaceRoot, '.git'), { recursive: true, force: true })
  }
})
