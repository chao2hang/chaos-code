import { expect, test, type APIRequestContext, type Page } from '@playwright/test'
import { spawn, type ChildProcess } from 'node:child_process'
import { createServer } from 'node:net'
import { once } from 'node:events'
import { existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { git, gitAlert, gitPanel, gitTab, messageBox, resetRepository, suggestButton } from './support/commit-form'
import { gitWorkspace, holdFile, promptLog, providerKey, providerModel, providerPort, providerReply } from './support/paths'
import { dismissDrawer, headerTab } from './support/shell'

// Safe Web Mode refuses a workspace-touching request in the socket, before the
// engine ever sees it, with one generic answer that carries no request id. Every
// panel that was waiting for that answer has to stop waiting anyway -- otherwise
// the mode does not just block the action, it freezes the control that asked for
// it. These tests start a real host with `CHAOS_SAFE_WEB_MODE=1`, click each
// refused control, and then read the disk to confirm the refusal was real.

const refusal = 'Safe Web Mode 禁止此操作'

async function availablePort(): Promise<number> {
  const server = createServer()
  server.listen(0, '127.0.0.1')
  await once(server, 'listening')
  const address = server.address()
  if (!address || typeof address === 'string') throw new Error('Could not determine an ephemeral TCP port')
  const { port } = address
  await new Promise<void>((resolveClose, reject) => server.close((error) => (error ? reject(error) : resolveClose())))
  return port
}

async function waitForHost(request: APIRequestContext, origin: string, output: () => string) {
  for (let attempt = 0; attempt < 50; attempt += 1) {
    try {
      const response = await request.get(`${origin}/health`, { timeout: 500 })
      if (response.ok()) return
    } catch (error) {
      if (attempt === 49) throw new Error(`health never answered: ${String(error)}\n${output()}`)
    }
    await new Promise((resolveDelay) => setTimeout(resolveDelay, 100))
  }
}

/**
 * Starts a host in Safe Web Mode, loads the page from it, and runs the test body
 * against that origin.
 *
 * The mode is set by the process environment, so it cannot be shared with the host
 * the other commit spec uses; the endpoint and the repository can be, and are.
 */
async function withSafeModeHost(page: Page, request: APIRequestContext, run: () => Promise<void>) {
  const repositoryRoot = resolve('..', '..')
  const assetsDir = resolve('dist')
  if (!existsSync(resolve(assetsDir, 'index.html'))) {
    throw new Error(`${assetsDir}/index.html is missing; run \`npm run build\` before the browser E2E`)
  }
  const port = await availablePort()
  const origin = `http://127.0.0.1:${port}`
  let output = ''
  let host: ChildProcess | undefined
  try {
    host = spawn(resolve(repositoryRoot, 'target/debug/chaos-web'), [], {
      cwd: repositoryRoot,
      env: {
        ...process.env,
        CHAOS_WEB_PORT: String(port),
        CHAOS_WEB_TOKEN: '',
        CHAOS_WEB_DEV_ORIGIN: origin,
        CHAOS_WEB_ALLOW_DYNAMIC_DEV_ORIGIN: '1',
        CHAOS_WEB_ASSETS_DIR: assetsDir,
        CHAOS_WEB_SQLITE: undefined,
        CHAOS_WEB_STATE: undefined,
        CHAOS_SAFE_WEB_MODE: '1',
        CHAOS_WORKSPACE_ROOT: gitWorkspace,
        CHAOS_PROVIDER_BASE_URL: `http://127.0.0.1:${providerPort}/v1`,
        CHAOS_PROVIDER_MODEL: providerModel,
        CHAOS_PROVIDER_API_KEY: providerKey,
      },
      stdio: ['ignore', 'ignore', 'pipe'],
    })
    host.stderr?.on('data', (chunk: Buffer) => {
      output += chunk.toString()
    })
    await waitForHost(request, origin, () => {
      if (host?.exitCode !== null) throw new Error(`Safe Web Mode host exited early: ${output}`)
      return output
    })

    await page.goto(origin)
    // Opening the page asks the host for the workspace list on its own, and this
    // mode refuses that too. The badge has to say which mode refused it rather than
    // sit on a generic 请求错误 above a session that was created without trouble.
    await expect(page.getByTestId('session-status')).toHaveText('安全模式已拒绝')
    await dismissDrawer(page)
    await run()
  } finally {
    if (host && host.exitCode === null) {
      host.kill('SIGTERM')
      await Promise.race([once(host, 'exit'), new Promise((resolveDelay) => setTimeout(resolveDelay, 3_000))])
    }
  }
}

const promptsReceived = () => readFileSync(promptLog, 'utf8').trim()

async function openCommitForm(page: Page) {
  await gitTab(page).click()
  await expect(gitPanel(page)).toBeVisible()
  // Switching to this tab asks for the status on its own, so a refused status read
  // is the first thing the panel has to survive: it must say so and stop showing
  // work in progress, not sit at 「Git 请求处理中…」 with 执行 Git 操作 greyed out.
  await expect(gitAlert(page, 'Git 请求失败')).toContainText(refusal)
  await expect(gitPanel(page).getByRole('status')).toHaveCount(0)
  await page.getByLabel('Git 操作类型').selectOption('commit')
  await expect(messageBox(page)).toHaveValue('')
}

test.beforeEach(() => {
  resetRepository()
  rmSync(holdFile, { force: true })
  writeFileSync(promptLog, '')
})

test('a refused suggestion ends the wait instead of freezing the button', async ({ page, request }) => {
  await withSafeModeHost(page, request, async () => {
    await openCommitForm(page)
    await suggestButton(page).click()

    await expect(gitAlert(page, '提交信息建议失败')).toContainText(refusal)
    // The whole point of the check: a refusal is an answer, so the control has to
    // go back to being usable rather than staying at 建议生成中 forever.
    await expect(suggestButton(page)).toBeEnabled()
    await expect(suggestButton(page)).toHaveText('建议提交信息')
    await expect(gitPanel(page).getByRole('status')).toHaveCount(0)
    await expect(page.getByTestId('session-status')).toHaveText('提交信息建议失败')
    await expect(messageBox(page)).toHaveValue('')

    // And the refusal was real, not a client-side excuse: the endpoint was never
    // asked, and the staged edit is exactly as it was left.
    expect(promptsReceived()).toBe('')
    expect(git(['log', '-1', '--format=%s'])).toBe('base')
    expect(git(['status', '--porcelain=v1'])).toContain('M  note.txt')
  })
})

test('every refused panel reports the refusal and stops showing work in progress', async ({ page, request }) => {
  await withSafeModeHost(page, request, async () => {
    await gitTab(page).click()
    await expect(gitPanel(page)).toBeVisible()
    await page.getByRole('button', { name: '刷新 Git 状态' }).click()
    await expect(gitAlert(page, 'Git 请求失败')).toContainText(refusal)
    await expect(gitPanel(page).getByRole('status')).toHaveCount(0)
    await expect(page.getByRole('button', { name: '执行 Git 操作' })).toBeEnabled()

    const terminal = page.locator('section[aria-label="终端控制台"]')
    await headerTab(page, /终端/).click()
    // A command with a side effect, so the assertion below is about whether the
    // host ran it rather than about what the panel chose to render.
    await page.getByLabel('终端执行命令').fill('touch refused-by-safe-mode.txt')
    await terminal.getByRole('button', { name: '运行命令' }).click()
    await expect(terminal.getByRole('alert')).toContainText(`终端执行失败：${refusal}`)
    await expect(terminal.getByRole('status')).toHaveCount(0)
    await expect(terminal.getByRole('button', { name: '运行命令' })).toBeEnabled()
    expect(existsSync(resolve(gitWorkspace, 'refused-by-safe-mode.txt'))).toBe(false)

    const upload = page.locator('[aria-label="附件上传"]')
    await headerTab(page, /文件/).click()
    await upload.getByTestId('upload-file-input').setInputFiles({ name: 'refused.txt', mimeType: 'text/plain', buffer: Buffer.from('安全模式不该写入\n') })
    await upload.getByTestId('upload-submit').click()
    await expect(upload.getByRole('alert')).toContainText(`上传失败：${refusal}`)
    await expect(upload.getByTestId('upload-submit')).toBeEnabled()
    await expect(upload.getByTestId('upload-cancel')).toBeHidden()
    expect(existsSync(resolve(gitWorkspace, 'refused.txt'))).toBe(false)

    expect(promptsReceived()).toBe('')
    expect(git(['log', '-1', '--format=%s'])).toBe('base')
  })
})

test('what Safe Web Mode allows still reaches the same endpoint', async ({ page, request }) => {
  const marker = `safe mode prompt ${Date.now().toString(36)}`
  await withSafeModeHost(page, request, async () => {
    // The refusals above must not be read as a dead host: a prompt is allowed, and
    // it is answered by the very endpoint the suggestion was refused from.
    await page.getByTestId('composer-input').fill(marker)
    await page.getByTestId('composer-submit').click()
    await expect(page.locator('.user p').last()).toContainText(marker)
    await expect(page.locator('.assistant .markdown-body').last()).toContainText(providerReply)
    expect(promptsReceived()).toContain(marker)
  })
})
