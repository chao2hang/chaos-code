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
  await page.getByTestId('composer-input').fill(prompt)
  await page.getByTestId('composer-submit').click()
  await expect(page.locator('.user p').last()).toContainText(prompt)
  await expect(page.locator('.assistant p').last()).toContainText(response)
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
  await expect(page.locator('.user p').last()).toContainText(`alpha marker ${suffix}`)
  await expect(page.locator('.user p')).not.toContainText(`beta marker ${suffix}`)
  await workspaceButton(page, second).click()
  await expect(page.locator('.user p').last()).toContainText(`beta marker ${suffix}`)
  await expect(page.locator('.user p')).not.toContainText(`alpha marker ${suffix}`)

  await page.getByRole('button', { name: /^主题：/ }).click()
  await page.getByLabel('面板宽度').fill('320')
  await page.reload()
  await expect(page.locator('[data-testid="app-shell"] header')).toContainText('会话已创建')
  await expect(page.getByRole('button', { name: /^主题：light$/ })).toBeVisible()
  await expect(page.getByLabel('面板宽度')).toHaveValue('320')
  await expect(workspaceButton(page, second)).toHaveClass(/active/)
  await expect(page.locator('.empty')).toBeVisible()
  await sendPrompt(page, `beta after reload ${suffix}`, `beta after reload ${suffix}`)

  if (isMobile) {
    const composer = page.getByTestId('composer-input')
    await composer.fill('mobile line one')
    await composer.press('Shift+Enter')
    await composer.type('mobile line two')
    await expect(composer).toHaveValue('mobile line one\nmobile line two')
    await composer.press('Enter')
    await expect(page.locator('.user p').last()).toContainText('mobile line one\nmobile line two')
    await expect(page.locator('.assistant p').last()).toContainText('mobile line one')
  }

  await workspaceButton(page, first).getByText('归档').click()
  await expect(workspaceButton(page, first)).toHaveCount(0)
  await expect(workspaceButton(page, second)).toBeVisible()
  await expect(workspaceButton(page, second)).toHaveClass(/active/)
  await expect(page.locator('.user p').filter({ hasText: `beta after reload ${suffix}` })).toBeVisible()
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
