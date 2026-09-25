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
  await expect(page.locator('[data-testid="app-shell"] header')).toContainText('会话已创建')

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

  await page.getByRole('button', { name: /^主题：/ }).click()
  await page.getByLabel('面板宽度').fill('320')
  await page.reload()
  await expect(page.locator('[data-testid="app-shell"] header')).toContainText('会话已创建')
  await expect(page.getByRole('button', { name: /^主题：light$/ })).toBeVisible()
  await expect(page.getByLabel('面板宽度')).toHaveValue('320')
  await expect(workspaceButton(page, second)).toHaveClass(/active/)
  await expect(page.locator('.empty')).toBeVisible()
  const markdownPrompt = `beta after reload ${suffix} with **bold**, \`inline code\`, and a list:\n\n- first item\n- second item\n\n<script>document.documentElement.dataset.pwned='true'</script>\n\n[safe link](https://example.com) [blocked link](./config.toml) [bad scheme](javascript:alert(1))`
  await sendPrompt(page, markdownPrompt, markdownPrompt)
  await expect(page.locator('.user ul > li')).toHaveCount(2)
  await expect(page.locator('.assistant strong')).toContainText('bold')
  await expect(page.locator('.assistant code')).toContainText('inline code')
  await expect(page.locator('.assistant li')).toHaveCount(2)
  await expect(page.locator('.assistant script, .assistant img')).toHaveCount(0)
  await expect(page.locator('html')).not.toHaveAttribute('data-pwned')
  const externalLink = page.locator('.assistant a[href="https://example.com"]')
  await expect(externalLink).toHaveAttribute('target', '_blank')
  await expect(externalLink).toHaveAttribute('rel', 'noopener noreferrer')
  await expect(page.locator('.assistant a[href="./config.toml"], .assistant a[href^="javascript:"]')).toHaveCount(0)
  await expect(page.locator('.assistant').getByText('blocked link')).toBeVisible()
  await expect(page.locator('.assistant').getByText('bad scheme')).toBeVisible()

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

  await workspaceButton(page, first).getByText('归档').click()
  await expect(workspaceButton(page, first)).toHaveCount(0)
  await expect(workspaceButton(page, second)).toBeVisible()
  await expect(workspaceButton(page, second)).toHaveClass(/active/)
  await expect(page.locator('.user p').filter({ hasText: `beta after reload ${suffix}` })).toBeVisible()
})

test('approval rejection and question response follow the real WebSocket entry path', async ({ page }) => {
  const suffix = `${Date.now()}-${Math.random().toString(16).slice(2, 8)}`
  await page.goto('/')
  await expect(page.locator('[data-testid="app-shell"] header')).toContainText('会话已创建')

  const composer = page.getByTestId('composer-input')
  await composer.fill(`/approve-tool reject marker ${suffix}`)
  await page.getByTestId('composer-submit').click()
  const approval = page.locator('article[aria-label="工具审批"]')
  await expect(approval).toContainText('需要审批：demo.tool')
  await expect(approval).toContainText(`reject marker ${suffix}`)
  await approval.getByRole('button', { name: '拒绝' }).click()
  await expect(approval).toHaveCount(0)
  await expect(page.locator('[data-testid="app-shell"] header')).toContainText('审批已处理')

  await composer.fill(`/ask question marker ${suffix}`)
  await page.getByTestId('composer-submit').click()
  const question = page.locator('article[aria-label="问题"]')
  await expect(question).toContainText(`question marker ${suffix}`)
  await question.getByRole('button', { name: '是' }).click()
  await expect(question).toHaveCount(0)
  await expect(page.locator('[data-testid="app-shell"] header')).toContainText('回答已提交')
})

test('empty workspace prompt cancellation and empty submission remain safe', async ({ page }) => {
  await page.goto('/')
  await expect(page.locator('[data-testid="app-shell"] header')).toContainText('会话已创建')
  const createButton = page.getByRole('button', { name: '+ 新工作区' })
  page.once('dialog', (dialog) => dialog.dismiss())
  await createButton.click()
  await expect(page.getByTestId('composer-submit')).toBeDisabled()
  await expect(page.getByText('创建会话后，在下方输入 Prompt。')).toBeVisible()
})
