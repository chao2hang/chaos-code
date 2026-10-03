import { expect, test, type Page } from '@playwright/test'

const backendPort = process.env.CHAOS_E2E_BACKEND_PORT || '8787'
const workspaceFixtureName = 'workspace'

async function openSettings(page: Page) {
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
  // Ctrl+5 is the settings binding in src/shortcuts.ts; using it here also proves
  // the listener, the request for the host's self-report and the panel are wired.
  await page.keyboard.press('Control+5')
  await expect(page.getByTestId('settings-panel')).toBeVisible()
  await expect(page.getByTestId('settings-host-pending')).toHaveCount(0)
}

// A locked row renders its reason as a second <dd>, so the value is always the
// first one and the note is the last.
function rowValue(page: Page, id: string) {
  return page.getByTestId(id).locator('dd').first()
}

test('the settings panel reports the process it is actually talking to', async ({ page }) => {
  await openSettings(page)

  await expect(rowValue(page, 'general-version')).toHaveText(/^\d+\.\d+\.\d+/)
  // The page shows the socket it opened, not the port it was configured with.
  await expect(rowValue(page, 'general-bind')).toHaveText(`127.0.0.1:${backendPort}`)
  await expect(rowValue(page, 'general-protocol')).toHaveText(/^[1-9]\d*$/)
  // No CHAOS_WEB_STATE in this fixture, so the honest answer is "memory only".
  await expect(rowValue(page, 'general-state')).toContainText('仅内存')
  await expect(page.getByTestId('general-state')).toHaveClass(/warn/)
  await expect(rowValue(page, 'permissions-workspace')).toContainText(workspaceFixtureName)
  await expect(rowValue(page, 'permissions-workspace')).toContainText('/')
  // No token was configured for the fixture, and the panel has to say so.
  await expect(rowValue(page, 'security-token')).toContainText('未设置')
  await expect(page.getByTestId('security-token')).toHaveClass(/warn/)
  await expect(rowValue(page, 'security-preview')).toContainText('未开启')
  await expect(rowValue(page, 'updates-mode')).toContainText('启动它的方式')

  // Safe Web Mode is off here, and the inventory is worded as what would happen.
  await expect(page.getByTestId('safe-mode-refusal-summary')).toContainText('未开启')
  // The count is whatever the serving process says it is; the point is that the
  // page renders the whole list it was handed, with both columns filled in.
  const refusals = page.getByTestId('safe-mode-refusals').locator('li')
  expect(await refusals.count()).toBeGreaterThanOrEqual(15)
  const announced = Number(((await page.getByTestId('safe-mode-refusal-summary').textContent()) ?? '').match(/(\d+) 类操作/)?.[1] ?? -1)
  expect(await refusals.count()).toBe(announced)
  await expect(refusals.first().locator('code')).toHaveText(/^[a-z_]+$/)
  await expect(refusals.first().locator('span')).not.toHaveText('')
  await expect(page.getByTestId('safe-mode-refusals')).toContainText('propose_file_write')
  await expect(page.getByTestId('safe-mode-refusals')).toContainText('把文件写进工作区')

  // Every category the checklist names is on screen in the order it names them.
  const order = await page.locator('[data-testid^="settings-category-"]').evaluateAll((nodes) => nodes.map((node) => node.getAttribute('data-testid')))
  expect(order).toEqual([
    'settings-category-general', 'settings-category-appearance', 'settings-category-model',
    'settings-category-provider', 'settings-category-permissions', 'settings-category-security',
    'settings-category-shortcuts', 'settings-category-remote', 'settings-category-updates',
  ])

  // The panel has to stay inside the viewport at phone width, where the right bar
  // becomes a full-width sheet and the row grid collapses.
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)
  expect(overflow).toBeLessThanOrEqual(0)
})

test('the theme control changes the page and outlives a reload', async ({ page }) => {
  await openSettings(page)
  expect(await page.evaluate(() => document.documentElement.dataset.theme)).toBe('dark')

  await page.getByTestId('theme-light').click()
  await expect(page.evaluate(() => document.documentElement.dataset.theme)).resolves.toBe('light')
  await expect(page.getByTestId('theme-light')).toHaveAttribute('aria-pressed', 'true')
  await expect(page.getByTestId('theme-dark')).toHaveAttribute('aria-pressed', 'false')
  await expect(rowValue(page, 'appearance-theme')).toHaveText('浅色')

  await page.keyboard.press('Control+Shift+l')
  await expect(page.evaluate(() => document.documentElement.dataset.theme)).resolves.toBe('system')
  await expect(rowValue(page, 'appearance-theme')).toHaveText('跟随系统')

  const stored = await page.evaluate(() => window.localStorage.getItem('chaos-ui-layout'))
  expect(stored).toContain('"theme":"system"')

  await page.reload()
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
  expect(await page.evaluate(() => document.documentElement.dataset.theme)).toBe('system')
  await page.keyboard.press('Control+5')
  await expect(page.getByTestId('settings-panel')).toBeVisible()
  await expect(rowValue(page, 'appearance-theme')).toHaveText('跟随系统')
})

test('the model and provider controls still write through to the host', async ({ page }) => {
  await openSettings(page)
  const model = `settings-panel-model-${Date.now().toString(36)}`
  await page.getByLabel('Provider Model').fill(model)
  await page.getByLabel('Provider Base URL').fill('https://api.example.test/v1')
  await page.getByRole('button', { name: '保存设置' }).click()
  await expect(page.getByTestId('session-status')).toHaveText('设置已更新')
  // The rows are derived from the host's reply, so they only move if the write
  // actually round-tripped.
  await expect(rowValue(page, 'model-current')).toHaveText(model)
  await expect(rowValue(page, 'provider-base-url')).toContainText('api.example.test')
  await expect(rowValue(page, 'provider-api-key')).toHaveText('host 侧未配置')
})

test('the number keys reach the panels and the composer keeps its own keys', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

  await expect(page.getByRole('button', { name: '⚙️ 设置' })).toHaveAttribute('aria-keyshortcuts', /5$/)
  await expect(page.getByRole('button', { name: '📁 文件' })).toHaveAttribute('aria-keyshortcuts', /2$/)

  await page.keyboard.press('Control+2')
  await expect(page.locator('section[aria-label="文件浏览器"]')).toBeVisible()
  await page.keyboard.press('Control+3')
  await expect(page.locator('section[aria-label="Git 状态"]')).toBeVisible()
  await page.keyboard.press('Control+7')
  await expect(page.locator('section[aria-label="代码差异审查"]')).toBeVisible()
  await page.keyboard.press('Control+1')
  await expect(page.locator('section[aria-label="文件浏览器"]')).toHaveCount(0)

  await page.keyboard.press('Control+k')
  await expect(page.getByTestId('composer-input')).toBeFocused()

  // Bare digits belong to the composer, not to the shell.
  await page.getByTestId('composer-input').press('2')
  await expect(page.getByTestId('composer-input')).toHaveValue('2')
  await expect(page.locator('section[aria-label="文件浏览器"]')).toHaveCount(0)

  // Alt is the window manager's, so a modified-but-Alt press changes nothing.
  await page.keyboard.press('Alt+Control+5')
  await expect(page.getByTestId('settings-panel')).toHaveCount(0)
})
