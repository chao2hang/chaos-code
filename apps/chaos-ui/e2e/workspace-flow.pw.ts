import AxeBuilder from '@axe-core/playwright'
import { mkdir, rm } from 'node:fs/promises'
import { resolve } from 'node:path'
import { expect, test, type Page } from '@playwright/test'

function workspaceButton(page: Page, name: string) {
  return page.locator('button[data-testid^="workspace-"]').filter({ hasText: name })
}

async function createWorkspace(page: Page, name: string) {
  page.once('dialog', (dialog) => dialog.accept(name))
  await page.getByRole('button', { name: '+ 新工作区' }).click()
  await expect(workspaceButton(page, name)).toBeVisible()
  await expect(workspaceButton(page, name)).toHaveClass(/active/)
}

async function sendPrompt(page: Page, prompt: string, response: string) {
  const firstLine = prompt.split('\n', 1)[0].replace(/[*`_]/g, '')
  await page.getByTestId('composer-input').fill(prompt)
  await page.getByTestId('composer-submit').click()
  await expect(page.locator('.user p').first()).toContainText(firstLine)
  await expect(page.locator('.assistant p').first()).toContainText(firstLine)
  await expect(page.locator('.assistant')).toContainText(response.split('\n', 1)[0].replace(/[*`_]/g, ''))
}

test('workspace sessions stay isolated across create, submit, switch, reload, archive and mobile use', async ({ page, isMobile }) => {
  const suffix = `${Date.now()}-${Math.random().toString(16).slice(2, 8)}`
  const first = `E2E Alpha ${suffix}`
  const second = `E2E Beta ${suffix}`

  await page.goto('/')
  await expect(page.getByTestId('app-shell')).toBeVisible()
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

  const health = await page.request.get('/health')
  expect(health.ok()).toBeTruthy()
  const handshake = await page.request.get('/api/handshake')
  expect(handshake.ok()).toBeTruthy()
  await expect(handshake).toBeOK()
  expect((await handshake.json()).type).toBe('handshake')

  await createWorkspace(page, first)
  await sendPrompt(page, `alpha marker ${suffix}`, `alpha marker ${suffix}`)
  await createWorkspace(page, second)
  await sendPrompt(page, `beta marker ${suffix}`, `beta marker ${suffix}`)

  await workspaceButton(page, first).click()
  await expect(page.locator('.user p').first()).toContainText(`alpha marker ${suffix}`)
  await expect(page.locator('.user p')).not.toContainText(`beta marker ${suffix}`)
  await workspaceButton(page, second).click()
  await expect(page.locator('.user p').first()).toContainText(`beta marker ${suffix}`)
  await expect(page.locator('.user p')).not.toContainText(`alpha marker ${suffix}`)

  const archiveFirst = page.getByRole('button', { name: `归档工作区 ${first}` })
  await archiveFirst.focus()
  await expect(archiveFirst).toBeFocused()
  await expect(archiveFirst).toHaveCSS('outline-style', 'solid')
  await expect(archiveFirst).toHaveCSS('outline-width', '3px')
  await expect(archiveFirst).toHaveAttribute('title', '归档此工作区')

  await page.getByRole('button', { name: /^主题：/ }).click()
  await page.getByLabel('面板宽度').fill('320')
  await page.reload()
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
  await expect(page.getByRole('button', { name: /^主题：light$/ })).toBeVisible()
  await expect(page.getByLabel('面板宽度')).toHaveValue('320')
  await expect(workspaceButton(page, second)).toHaveClass(/active/)
  await expect(page.locator('.empty')).toBeVisible()
  const imageRequestUrls: string[] = []
  page.on('request', (request) => {
    if (request.url() === 'https://example.com/tracker.png') imageRequestUrls.push(request.url())
  })
  const markdownPrompt = `beta after reload ${suffix} with **bold**, \`inline code\`, and a list:\n\n- first item\n- second item\n\n<script>document.documentElement.dataset.pwned='true'</script>\n\n[safe link](https://example.com) [blocked link](./config.toml) [bad scheme](javascript:alert(1)) [network path](//example.com/steal) [data url](data:text/html,hello) [fragment](#note) [email](mailto:support@example.com)

![remote image](https://example.com/tracker.png)`
  await sendPrompt(page, markdownPrompt, markdownPrompt)
  await expect(page.locator('.user ul > li')).toHaveCount(2)
  await expect(page.locator('.assistant strong')).toContainText('bold')
  await expect(page.locator('.assistant code')).toContainText('inline code')
  await expect(page.locator('.assistant li')).toHaveCount(2)
  await expect(page.locator('.assistant script, .assistant img')).toHaveCount(0)
  await expect(page.locator('.assistant').getByText('remote image')).toBeVisible()
  expect(imageRequestUrls).toEqual([])
  await expect(page.locator('html')).not.toHaveAttribute('data-pwned')
  const externalLink = page.locator('.assistant a[href="https://example.com"]')
  await expect(externalLink).toHaveAttribute('target', '_blank')
  await expect(externalLink).toHaveAttribute('rel', 'noopener noreferrer')
  await expect(page.locator('.assistant a[href="./config.toml"], .assistant a[href^="javascript:"], .assistant a[href^="//"], .assistant a[href^="data:"]')).toHaveCount(0)
  await expect(page.locator('.assistant a[href="#note"]')).toBeVisible()
  await expect(page.locator('.assistant a[href="mailto:support@example.com"]')).toBeVisible()
  await expect(page.locator('.assistant').getByText('blocked link')).toBeVisible()
  await expect(page.locator('.assistant').getByText('bad scheme')).toBeVisible()
  await expect(page.locator('.assistant').getByText('network path')).toBeVisible()
  await expect(page.locator('.assistant').getByText('data url')).toBeVisible()

  if (isMobile) {
    const composer = page.getByTestId('composer-input')
    await composer.fill('mobile line one')
    await composer.press('Shift+Enter')
    await composer.type('mobile line two')
    await expect(composer).toHaveValue('mobile line one\nmobile line two')
    await composer.press('Enter')
    await expect(page.locator('.user ul > li')).toHaveCount(2)
    await expect(page.locator('.assistant strong')).toContainText('bold')
    await expect(page.locator('.user p').last()).toContainText('mobile line one\nmobile line two')
    await expect(page.locator('.assistant p').last()).toContainText('mobile line one')
  }

  await page.getByRole('button', { name: `归档工作区 ${first}` }).click()
  await expect(workspaceButton(page, first)).toHaveCount(0)
  await expect(workspaceButton(page, second)).toBeVisible()
  await expect(workspaceButton(page, second)).toHaveClass(/active/)
  await expect(page.locator('.user p').filter({ hasText: `beta after reload ${suffix}` })).toBeVisible()
})

test('timeline follows new responses at the bottom and preserves a reader anchor when scrolled up', async ({ page }) => {
  await page.goto('/')
  const timeline = page.getByTestId('session-timeline')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

  for (let index = 0; index < 8; index += 1) {
    const marker = `scroll-anchor-${index}`
    await page.getByTestId('composer-input').fill(marker)
    await page.getByTestId('composer-submit').click()
    await expect(page.locator('.assistant').last()).toContainText(marker)
  }

  const bottomState = await timeline.evaluate((element) => ({
    scrollTop: element.scrollTop,
    scrollHeight: element.scrollHeight,
    clientHeight: element.clientHeight,
  }))
  expect(bottomState.scrollHeight).toBeGreaterThan(bottomState.clientHeight)
  expect(bottomState.scrollHeight - bottomState.clientHeight - bottomState.scrollTop).toBeLessThanOrEqual(48)

  await timeline.evaluate((element) => { element.scrollTop = 0 })
  await expect.poll(() => timeline.evaluate((element) => element.scrollTop)).toBe(0)
  const visibleBefore = await page.locator('.user').first().evaluate((element) => element.getBoundingClientRect().top)
  await page.getByTestId('composer-input').fill('new response while reading history')
  await page.getByTestId('composer-submit').click()
  await expect(page.locator('.assistant').last()).toContainText('new response while reading history')
  const visibleAfter = await page.locator('.user').first().evaluate((element) => element.getBoundingClientRect().top)
  expect(Math.abs(visibleAfter - visibleBefore)).toBeLessThanOrEqual(2)

})

test('file browser navigates directories, searches, and reads files through the Engine', async ({ page }) => {
  const outgoingMessages: { type?: string; relative_path?: string }[] = []
  page.on('websocket', (webSocket) => webSocket.on('framesent', ({ payload }) => {
    if (typeof payload !== 'string') return
    try {
      const message = JSON.parse(payload) as { type?: string; relative_path?: string }
      outgoingMessages.push(message)
    } catch {
      return
    }
  }))
  const root = resolve('e2e/fixtures/workspace')
  await mkdir(root, { recursive: true })
  await mkdir(resolve(root, 'nested'), { recursive: true })
  await mkdir(resolve(root, 'empty'), { recursive: true })
  await rm(resolve(root, 'empty/.keep'), { force: true })
  await import('node:fs/promises').then(({ writeFile }) => writeFile(resolve(root, 'nested/needle.txt'), 'The fixture search phrase is safely stored in this nested text file.\n'))
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
  await page.getByRole('button', { name: '📁 文件' }).click()

  const files = page.locator('section[aria-label="文件浏览器"]')
  await expect(files.getByRole('button', { name: '打开目录 nested' })).toBeVisible()
  await expect(files.getByRole('button', { name: '打开目录 empty' })).toBeVisible()
  await files.getByRole('button', { name: '打开目录 nested' }).click()
  await expect(files.getByRole('heading', { name: '目录列表（nested）' })).toBeVisible()
  await expect(files.getByRole('button', { name: '📄 needle.txt' })).toBeVisible()

  await files.getByRole('button', { name: '📄 needle.txt' }).click()
  await expect(page.getByRole('heading', { name: '文件编辑：nested/needle.txt' })).toBeVisible()
  await expect(page.getByLabel('编辑文件内容 nested/needle.txt')).toContainText('fixture search phrase')

  await page.getByLabel('编辑文件内容 nested/needle.txt').fill('Approved write fixture content.')
  await page.getByRole('button', { name: '提议保存修改' }).click()
  const writeApproval = page.locator('article[aria-label="工具审批"]')
  await expect(writeApproval).toContainText('需要审批：workspace.write_file')
  await writeApproval.getByRole('button', { name: '允许' }).click()
  await expect.poll(() => outgoingMessages.some((message) => message.type === 'list_files' && message.relative_path === 'nested')).toBe(true)
  await expect(page.locator('li[aria-label="工具 workspace.write_file 已完成"]')).toContainText('已写入 nested/needle.txt（31 字节）')
  await expect(page.getByText('文件已保存，目录已刷新。')).toBeVisible()
  await files.getByRole('button', { name: '📄 needle.txt' }).click()
  const editor = page.getByLabel('编辑文件内容 nested/needle.txt')
  await expect(editor).toHaveValue('Approved write fixture content.')

  await editor.fill('This rejected text must never reach disk.')
  await page.getByRole('button', { name: '提议保存修改' }).click()
  const rejectedWrite = page.locator('article[aria-label="工具审批"]')
  await expect(rejectedWrite).toContainText('需要审批：workspace.write_file')
  await rejectedWrite.getByRole('button', { name: '拒绝' }).click()
  await expect(page.getByTestId('session-status')).toHaveText('审批已拒绝')
  await files.getByRole('button', { name: '📄 needle.txt' }).click()
  await expect(editor).toHaveValue('Approved write fixture content.')

  await files.getByLabel('文件搜索关键词').fill('Approved write fixture content.')
  await files.getByRole('button', { name: '搜索文件' }).click()
  await expect(files.getByRole('heading', { name: '搜索结果（关键词：Approved write fixture content.）' })).toBeVisible()
  await expect(files.getByRole('button', { name: '📄 nested/needle.txt' })).toBeVisible()

  await files.getByRole('button', { name: 'workspace', exact: true }).click()
  await expect(files.getByRole('heading', { name: '目录列表（.）' })).toBeVisible()
  await expect(files.getByRole('button', { name: '打开目录 nested' })).toBeVisible()
  await page.getByRole('button', { name: '关闭侧栏' }).click()
  await page.getByTestId('composer-input').fill('Reference this @needle')
  const composerSuggestions = page.getByRole('listbox', { name: '命令与文件建议' })
  await expect(composerSuggestions.getByRole('option', { name: '@nested/needle.txt' })).toBeVisible()
  await page.getByTestId('composer-input').press('ArrowDown')
  await expect(page.getByTestId('composer-input')).toHaveAttribute('aria-activedescendant', /composer-suggestion-/)
  await page.getByTestId('composer-input').press('Enter')
  await expect(page.getByTestId('composer-input')).toHaveValue('Reference this @nested/needle.txt ')
  await page.getByTestId('composer-input').fill('')

  await page.getByRole('button', { name: '📁 文件' }).click()
  const reopenedFiles = page.locator('section[aria-label="文件浏览器"]')
  await reopenedFiles.getByRole('button', { name: '打开目录 empty' }).click()
  await expect(reopenedFiles.getByText('目录为空')).toBeVisible()
  await reopenedFiles.getByRole('button', { name: '上级目录' }).click()
  await expect(files.getByRole('heading', { name: '目录列表（.）' })).toBeVisible()

  await files.getByLabel('目录路径').fill('missing-folder')
  await files.getByRole('button', { name: '刷新目录' }).click()
  await expect(files.getByRole('alert')).toContainText('目录读取失败')
})

test('approval rejection and question response follow the real WebSocket entry path', async ({ page }) => {
  const suffix = `${Date.now()}-${Math.random().toString(16).slice(2, 8)}`
  await page.goto('/')
  const sessionStatus = page.getByRole('status')
  await expect(sessionStatus).toHaveAttribute('aria-live', 'polite')
  await expect(sessionStatus).toHaveText('会话已创建')

  const composer = page.getByTestId('composer-input')
  await composer.fill(`/approve-tool reject marker ${suffix}`)
  await page.getByTestId('composer-submit').click()
  const approval = page.locator('article[aria-label="工具审批"]')
  await expect(approval).toContainText('需要审批：demo.tool')
  await expect(approval).toContainText(`reject marker ${suffix}`)
  await approval.getByRole('button', { name: '拒绝' }).click()
  await expect(approval).toHaveCount(0)
  await expect(page.getByTestId('session-status')).toHaveText('审批已拒绝')

  await composer.fill(`/ask question marker ${suffix}`)
  await page.getByTestId('composer-submit').click()
  const question = page.locator('article[aria-label="问题"]')
  await expect(question).toContainText(`question marker ${suffix}`)
  await question.getByRole('button', { name: '是' }).click()
  await expect(question).toHaveCount(0)
  await expect(page.getByTestId('session-status')).toHaveText('回答已提交')
})

test('IDE navigation tabs switch across files, git, terminal, settings, marketplace, and diff views cleanly', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

  // Switch to Files tab
  await page.getByRole('button', { name: '📁 文件' }).click()
  const filesPanel = page.locator('section[aria-label="文件浏览器"]')
  await expect(filesPanel).toBeVisible()
  await expect(filesPanel.getByRole('button', { name: '刷新目录' })).toBeVisible()
  await expect(filesPanel.getByRole('button', { name: '搜索文件' })).toBeVisible()
  const accessibilityTags = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa', 'best-practice']
  const expectCurrentPanelAxeClean = async () => {
    const results = await new AxeBuilder({ page }).withTags(accessibilityTags).analyze()
    expect(results.violations.map(({ id, impact }) => ({ id, impact }))).toEqual([])
  }
  await expectCurrentPanelAxeClean()

  // Switch to Git tab
  await page.getByRole('button', { name: '🌿 Git' }).click()
  const gitPanel = page.locator('section[aria-label="Git 状态"]')
  await expect(gitPanel).toBeVisible()
  await expect(gitPanel.getByRole('button', { name: '刷新 Git 状态' })).toBeVisible()
  await expect(gitPanel.getByRole('button', { name: '执行 Git 操作' })).toBeVisible()
  await expectCurrentPanelAxeClean()

  // Switch to Terminal tab
  await page.getByRole('button', { name: '💻 终端' }).click()
  const terminalPanel = page.locator('section[aria-label="终端控制台"]')
  await expect(terminalPanel).toBeVisible()
  await expect(terminalPanel.getByRole('button', { name: '运行命令' })).toBeVisible()
  await expectCurrentPanelAxeClean()

  // Switch to Settings tab
  await page.getByRole('button', { name: '⚙️ 设置' }).click()
  const settingsPanel = page.locator('section[aria-label="设置面板"]')
  await expect(settingsPanel).toBeVisible()
  await expect(settingsPanel.getByRole('button', { name: '保存设置' })).toBeVisible()
  await expect(settingsPanel.getByRole('button', { name: '验证 Provider 连接' })).toBeVisible()
  await expect(settingsPanel.getByRole('button', { name: '导入会话' })).toBeVisible()
  await settingsPanel.getByLabel('Provider Base URL').fill('https://api.example.test/v1#unsafe-fragment')
  await settingsPanel.getByLabel('Provider Model').fill('fixture-model')
  const settingsPromise = page.waitForResponse((response) => response.url().endsWith('/ws') || response.request().resourceType() === 'websocket').catch(() => undefined)
  await settingsPanel.getByRole('button', { name: '验证 Provider 连接' }).click()
  await expect(page.getByTestId('session-status')).toHaveText('Provider 验证未通：invalid_base_url')
  await expect(settingsPanel).toContainText('校验未通过 (invalid_base_url)')
  await expect(settingsPanel).not.toContainText('API Key：已配置 API Key')
  await expectCurrentPanelAxeClean()

  // Switch to Marketplace tab
  await page.getByRole('button', { name: '🧩 插件' }).click()
  const marketplacePanel = page.locator('section[aria-label="插件市场"]')
  await expect(marketplacePanel).toBeVisible()
  await expect(marketplacePanel.getByRole('button', { name: '扫描插件生态' })).toBeVisible()
  await expectCurrentPanelAxeClean()

  // Switch to Diff tab
  await page.getByRole('button', { name: /^🔍 差异/ }).click()
  const diffPanel = page.locator('section[aria-label="代码差异审查"]')
  await expect(diffPanel).toBeVisible()
  await expect(diffPanel.getByRole('button', { name: '加载差异' })).toBeVisible()
  await expectCurrentPanelAxeClean()

  // Return to Chat tab
  await page.getByRole('button', { name: '💬 对话' }).click()
  await expect(page.getByTestId('session-timeline')).toBeVisible()
  await expect(page.getByTestId('composer-input')).toBeVisible()
})

test('approving a demo tool reports the missing adapter without claiming execution', async ({ page }) => {
  const suffix = `${Date.now()}-${Math.random().toString(16).slice(2, 8)}`
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

  const composer = page.getByTestId('composer-input')
  await composer.fill(`/approve-tool allow marker ${suffix}`)
  await page.getByTestId('composer-submit').click()
  const approval = page.locator('article[aria-label="工具审批"]')
  await expect(approval).toContainText(`allow marker ${suffix}`)
  await approval.getByRole('button', { name: '允许' }).click()

  await expect(approval).toHaveCount(0)
  await expect(page.getByRole('status')).toHaveAttribute('aria-live', 'polite')
  await expect(page.getByRole('status')).toHaveText('工具执行失败')
})

test('the empty app shell has no serious or critical axe violations', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

  const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa', 'best-practice']).analyze()
  expect(results.violations.map(({ id, impact, nodes }) => ({ id, impact, targets: nodes.map(({ target }) => target) }))).toEqual([])
})

test('the workspace, composer and rendered message remain axe-clean after real interaction', async ({ page }) => {
  const suffix = `${Date.now()}-${Math.random().toString(16).slice(2, 8)}`
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
  await createWorkspace(page, `Axe ${suffix}`)
  await sendPrompt(page, `Read **this** safely ${suffix}`, `Read **this** safely ${suffix}`)

  const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa', 'best-practice']).analyze()
  expect(results.violations.map(({ id, impact, nodes }) => ({ id, impact, targets: nodes.map(({ target }) => target) }))).toEqual([])
})

test('keyboard focus is visible and composer submission works without a pointer', async ({ page }) => {
  const suffix = `${Date.now()}-${Math.random().toString(16).slice(2, 8)}`
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

  const createWorkspace = page.getByRole('button', { name: '+ 新工作区' })
  await createWorkspace.focus()
  await expect(createWorkspace).toBeFocused()
  await expect(createWorkspace).toHaveCSS('outline-style', 'solid')
  await expect(createWorkspace).toHaveCSS('outline-width', '3px')
  await page.keyboard.press('Tab')

  const composer = page.getByTestId('composer-input')
  await composer.fill('/')
  const suggestions = page.getByRole('listbox', { name: '命令与文件建议' })
  await expect(suggestions.getByRole('option', { name: '/approve-tool 发起工具审批请求' })).toBeVisible()
  await composer.press('ArrowDown')
  await expect(suggestions.getByRole('option', { name: '/ask 向用户发起确认问题' })).toHaveAttribute('aria-selected', 'true')
  await composer.press('Enter')
  await expect(composer).toHaveValue('/ask ')
  await composer.fill('/')
  await composer.press('Escape')
  await expect(page.getByRole('listbox', { name: '命令与文件建议' })).toHaveCount(0)

  await composer.fill(`keyboard only ${suffix}`)
  const submit = page.getByTestId('composer-submit')
  await submit.focus()
  await expect(submit).toBeFocused()
  await expect(submit).toHaveCSS('outline-style', 'solid')
  await page.keyboard.press('Enter')

  await expect(page.locator('.user')).toContainText(`keyboard only ${suffix}`)
  await expect(page.locator('.assistant')).toContainText(`keyboard only ${suffix}`)
})

test('empty workspace prompt cancellation and empty submission remain safe', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
  const createButton = page.getByRole('button', { name: '+ 新工作区' })
  page.once('dialog', (dialog) => dialog.dismiss())
  await createButton.click()
  await expect(page.getByTestId('composer-submit')).toBeDisabled()
  await expect(page.getByText('创建会话后，在下方输入 Prompt。')).toBeVisible()
})
