import { expect, type Page } from '@playwright/test'

/**
 * Shared access to the workspace sidebar. Below `COMPACT_QUERY` the sidebar is an
 * overlay drawer that starts closed, so any spec that reaches a workspace control has to
 * open it first -- exactly what a person on a phone does. Keeping this in one place means
 * a spec cannot silently start asserting a flow that no longer exists at phone width.
 *
 * The desktop helpers deliberately do nothing: collapsing a docked sidebar is a persisted
 * preference, so a spec that only wanted the overlay out of the way must not flip it.
 */
export const COMPACT_QUERY = '(max-width: 768px)'

export const sidebar = (page: Page) => page.locator('[data-shell-column="sidebar"]')
export const expandButton = (page: Page) => page.getByRole('button', { name: '展开侧边栏' })
export const collapseButton = (page: Page) => page.getByRole('button', { name: '收起侧边栏' })
export const backdrop = (page: Page) => page.getByTestId('sidebar-backdrop')
export const workspaceButton = (page: Page, name: string) => page.locator('button[data-testid^="workspace-"]').filter({ hasText: name })

/**
 * One of the header tabs, scoped to the header. A panel control named 终端 or Git
 * exists inside the panel that tab opens, so an unscoped locator could silently
 * drive the wrong element.
 */
export const headerTab = (page: Page, name: RegExp | string) => page.locator('header.conversation-header').getByRole('button', { name })

export async function isCompact(page: Page) {
  return page.evaluate((query) => window.matchMedia(query).matches, COMPACT_QUERY)
}

export async function boxOf(page: Page, selector: string) {
  const box = await page.locator(selector).boundingBox()
  if (!box) throw new Error(`${selector} has no bounding box`)
  return { x: Math.round(box.x), y: Math.round(box.y), width: Math.round(box.width), height: Math.round(box.height) }
}

export async function openSidebar(page: Page) {
  if (await isCompact(page)) {
    if (!(await sidebar(page).isVisible())) await expandButton(page).click()
  }
  await expect(sidebar(page)).toBeVisible()
}

/** Puts the phone drawer away. A docked sidebar is left exactly as the user set it. */
export async function dismissDrawer(page: Page) {
  if (!(await isCompact(page)) || !(await sidebar(page).isVisible())) return
  // The drawer covers most of a phone screen, so the scrim has to be tapped beside it;
  // clicking its centre would land on the drawer itself.
  const drawer = await sidebar(page).boundingBox()
  const viewport = page.viewportSize()
  if (!drawer || !viewport) throw new Error('the drawer or the viewport is unavailable')
  await backdrop(page).click({ position: { x: drawer.x + drawer.width + 20, y: viewport.height - 40 } })
  await expect(sidebar(page)).toBeHidden()
}

/** Opens the sidebar when it is a drawer, runs the action, then puts the drawer away. */
export async function withSidebar<T>(page: Page, run: () => Promise<T>): Promise<T> {
  await openSidebar(page)
  try {
    return await run()
  } finally {
    await dismissDrawer(page)
  }
}

export async function createWorkspace(page: Page, name: string) {
  await withSidebar(page, async () => {
    page.once('dialog', (dialog) => dialog.accept(name))
    await page.getByRole('button', { name: '+ 新工作区' }).click()
    await expect(workspaceButton(page, name)).toBeVisible()
    await expect(workspaceButton(page, name)).toHaveClass(/active/)
  })
}
