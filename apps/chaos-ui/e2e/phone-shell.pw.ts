import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'
import { backdrop, boxOf, collapseButton, COMPACT_QUERY, createWorkspace, dismissDrawer, expandButton, isCompact, openSidebar, sidebar, workspaceButton } from './support/shell'

/**
 * Phone-width shell. Below this width the sidebar cannot sit beside the conversation,
 * and until now the whole shell turned into one scrolling column instead: the sidebar
 * stacked on top (measured `scrollHeight` 1344 in an 844 viewport), the composer fell
 * below the fold, and the sidebar's own controls were the only way to reach a
 * workspace. These specs pin the replacement -- a fixed-height shell whose timeline is
 * the only scroller, plus a sidebar that opens over the conversation.
 */

test('the shell keeps its own height, the timeline is the only scroller and the composer stays on screen', async ({ page }) => {
  const suffix = `${Date.now()}-${Math.random().toString(16).slice(2, 8)}`
  await page.goto('/')
  await expect(page.getByTestId('app-shell')).toBeVisible()
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

  // The workspace list lives in the sidebar, so on a phone this reaches it the way a
  // person would: open the drawer, create, dismiss it again.
  await createWorkspace(page, `Shell ${suffix}`)

  // Long enough that the transcript overflows the timeline at desktop width too, which
  // is what makes "the timeline is the scroller" a claim rather than a tautology.
  const prompt = `shell probe ${suffix} ${'这里补足一段足够长的中文回答内容用来把对话撑高。'.repeat(80)}`
  await page.getByTestId('composer-input').fill(prompt)
  await page.getByTestId('composer-submit').click()
  await expect(page.locator('.user').first()).toContainText(`shell probe ${suffix}`)
  await expect
    .poll(() => page.locator('[data-testid="session-timeline"]').innerText(), { timeout: 20_000 })
    .toContain(prompt.slice(-12))

  const metrics = await page.evaluate(() => {
    const shell = document.querySelector('[data-testid="app-shell"]')
    const timeline = document.querySelector('[data-testid="session-timeline"]')
    const composer = document.querySelector('[data-testid="composer-input"]')
    if (!(shell instanceof HTMLElement) || !(timeline instanceof HTMLElement) || !(composer instanceof HTMLElement)) {
      throw new Error('the shell, the timeline or the composer is missing')
    }
    const composerBox = composer.getBoundingClientRect()
    return {
      shellOverflow: shell.scrollHeight - shell.clientHeight,
      shellScrollTop: Math.round(shell.scrollTop),
      documentOverflow: document.documentElement.scrollHeight - document.documentElement.clientHeight,
      timelineOverflow: timeline.scrollHeight - timeline.clientHeight,
      timelineHorizontalOverflow: timeline.scrollWidth - timeline.clientWidth,
      composerTop: Math.round(composerBox.top),
      composerBottom: Math.round(composerBox.bottom),
      viewportHeight: window.innerHeight,
    }
  })

  expect(metrics.shellOverflow, 'the shell must not scroll; only its regions do').toBeLessThanOrEqual(1)
  expect(metrics.shellScrollTop).toBe(0)
  expect(metrics.documentOverflow).toBeLessThanOrEqual(1)
  expect(metrics.timelineOverflow, 'the conversation is long enough that something has to scroll').toBeGreaterThan(0)
  // The DOM walker in `workspace-flow.pw.ts` excuses an element that can scroll, which
  // is right for a scroller but wrong for the transcript: text that has to be dragged
  // sideways to be read is a layout defect, not a scroll region.
  expect(metrics.timelineHorizontalOverflow, 'the conversation must fit the viewport width').toBeLessThanOrEqual(1)
  expect(metrics.composerTop, 'the composer must be fully on the first screen').toBeGreaterThan(0)
  expect(metrics.composerBottom).toBeLessThanOrEqual(metrics.viewportHeight)
})

test('on a phone the sidebar opens over the conversation and can be dismissed', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
  test.skip(!(await isCompact(page)), 'the drawer only exists at phone width')

  // A docked sidebar would be open on load (the persisted preference says so); a drawer
  // that covered the conversation on every load would be worse than the old layout.
  await expect(sidebar(page)).toBeHidden()
  await expect(expandButton(page)).toBeVisible()
  const conversationBefore = await boxOf(page, '.center-col')

  await expandButton(page).click()
  await expect(sidebar(page)).toBeVisible()
  await expect(backdrop(page)).toBeVisible()
  const drawer = await boxOf(page, '[data-shell-column="sidebar"]')
  const viewport = page.viewportSize()
  if (!viewport) throw new Error('no viewport')
  expect(drawer.x).toBeGreaterThanOrEqual(0)
  expect(drawer.width).toBeLessThan(viewport.width)
  expect(drawer.height).toBe(viewport.height)
  expect(await boxOf(page, '.center-col'), 'the drawer overlays rather than pushing the conversation').toEqual(conversationBefore)

  await dismissDrawer(page)
  await expect(backdrop(page)).toBeHidden()

  // The drawer carries its own collapse control, and on a phone that control must close
  // the drawer rather than flip the docked preference it shares a name with.
  await expandButton(page).click()
  await expect(sidebar(page)).toBeVisible()
  await collapseButton(page).click()
  await expect(sidebar(page)).toBeHidden()
  await expect(expandButton(page)).toBeVisible()

  await expandButton(page).click()
  await expect(sidebar(page)).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(sidebar(page)).toBeHidden()
})

test('picking a workspace in the drawer hands the screen back to the conversation', async ({ page }) => {
  const suffix = `${Date.now()}-${Math.random().toString(16).slice(2, 8)}`
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
  test.skip(!(await isCompact(page)), 'a docked sidebar has nothing to dismiss')

  await createWorkspace(page, `Handback ${suffix}`)
  await createWorkspace(page, `Handback Two ${suffix}`)
  await openSidebar(page)
  await workspaceButton(page, `Handback ${suffix}`).click()

  // The drawer is an overlay on top of the conversation; leaving it open after a pick
  // would mean a second deliberate dismissal before anything could be read.
  await expect(sidebar(page)).toBeHidden()
  await expect(expandButton(page)).toBeVisible()
  await expect(page.getByTestId('composer-input')).toBeVisible()
  await expect(workspaceButton(page, `Handback Two ${suffix}`)).toBeHidden()
})

test('a wide window still docks the sidebar and persists that preference', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
  test.skip(await isCompact(page), 'the docked sidebar only exists at desktop width')

  await expect(sidebar(page)).toBeVisible()
  await collapseButton(page).click()
  await expect(sidebar(page)).toBeHidden()
  await expect(expandButton(page)).toBeVisible()
  await page.reload()
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
  await expect(sidebar(page), 'the collapsed dock must survive a reload').toBeHidden()

  await expandButton(page).click()
  await expect(sidebar(page)).toBeVisible()
  await page.reload()
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
  await expect(sidebar(page)).toBeVisible()
  await expect(expandButton(page)).toBeHidden()
})

test('the connection state stays on screen at phone width with the drawer closed', async ({ page }) => {
  await page.goto('/')
  const status = page.getByTestId('session-status')
  await expect(status).toHaveText('会话已创建')
  test.skip(!(await isCompact(page)), 'the docked sidebar already shows the state at desktop width')

  // The state used to live in the sidebar footer. Once the sidebar became a drawer that
  // starts closed, a phone user had no visible sign that the socket had dropped.
  await expect(sidebar(page)).toBeHidden()
  await expect(status).toBeVisible()
  const badge = await boxOf(page, '[data-testid="session-status"]')
  const header = await boxOf(page, '.conversation-header')
  const viewport = page.viewportSize()
  if (!viewport) throw new Error('no viewport')
  expect(badge.y, 'the state belongs in the header, which never scrolls away').toBeGreaterThanOrEqual(header.y)
  expect(badge.y + badge.height).toBeLessThanOrEqual(header.y + header.height + 1)
  expect(badge.x).toBeGreaterThanOrEqual(0)
  expect(badge.x + badge.width, 'the state must not be pushed off the narrow header').toBeLessThanOrEqual(viewport.width)
})

test('the open drawer keeps the keyboard: focus starts inside it and never reaches what the scrim covers', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
  test.skip(!(await isCompact(page)), 'the drawer only exists at phone width')

  const covered = page.locator('[data-shell-column="center"]')
  // `inert` is not an inherited attribute: it is set on the covered region and everything
  // inside it stops being reachable. So the region is what gets asserted, plus the one
  // behaviour that matters to a user -- a control inside it cannot be focused.
  const focusable = (testId: string) =>
    page.getByTestId(testId).evaluate((element) => {
      if (!(element instanceof HTMLElement)) throw new Error(`${testId} is not focusable markup`)
      element.focus()
      return document.activeElement === element
    })

  await expect(covered).toHaveJSProperty('inert', false)
  expect(await focusable('composer-input'), 'the composer is reachable before the drawer opens').toBe(true)

  await expandButton(page).click()
  await expect(sidebar(page)).toBeVisible()
  // Opening an overlay without moving focus into it hands the next keystroke to whatever
  // had focus before, which here is the conversation the scrim just covered.
  await expect(sidebar(page)).toBeFocused()
  await expect(covered).toHaveJSProperty('inert', true)
  expect(await focusable('composer-input'), 'the composer behind the scrim must not take focus').toBe(false)

  const reached: string[] = []
  for (let step = 0; step < 15; step += 1) {
    await page.keyboard.press('Tab')
    const where = await page.evaluate(() => {
      const active = document.activeElement
      if (!(active instanceof Element)) return 'nothing'
      if (active.closest('[data-shell-column="sidebar"]')) return 'sidebar'
      // The scrim is a real dismiss control, so it may hold focus.
      if (active.closest('[data-testid="sidebar-backdrop"]')) return 'backdrop'
      const column = active.closest('[data-shell-column]')
      return column ? `${String(column.getAttribute('data-shell-column'))}:${active.tagName.toLowerCase()}` : `page:${active.tagName.toLowerCase()}`
    })
    if (where !== 'sidebar' && where !== 'backdrop') reached.push(`tab ${step + 1} -> ${where}`)
  }
  expect(reached, 'no tab stop may live behind the scrim').toEqual([])

  await page.keyboard.press('Escape')
  await expect(sidebar(page)).toBeHidden()
  // Closing must hand focus back to the control that opened the drawer; leaving it on a
  // removed node drops the keyboard off the page entirely.
  await expect(expandButton(page)).toBeFocused()
  await expect(covered).toHaveJSProperty('inert', false)
  expect(await focusable('composer-input'), 'closing the drawer gives the conversation back').toBe(true)
})

test('dragging the window across the breakpoint leaves no overlay behind', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
  test.skip(!(await isCompact(page)), 'the test starts from a phone viewport')

  await expandButton(page).click()
  await expect(sidebar(page)).toBeVisible()

  await page.setViewportSize({ width: 1440, height: 1000 })
  // Wide, the sidebar is docked again by the persisted preference, and the ephemeral
  // drawer state must not be what is holding it open.
  await expect(sidebar(page)).toBeVisible()
  const position = await sidebar(page).evaluate((element) => getComputedStyle(element).position)
  expect(position, 'a docked sidebar is laid out by the grid, not pinned over it').toBe('static')

  await page.setViewportSize({ width: 390, height: 844 })
  await expect(sidebar(page)).toBeHidden()
  await expect(expandButton(page)).toBeVisible()
})

test('the header tabs keep their label on one line instead of stacking vertically', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

  // The tab strip is the one header region allowed to shrink and scroll. If its buttons
  // shrink with it, a two-character CJK label breaks one character per line and the
  // header grows to several times its height, pushing the conversation down. The
  // clipping walker in `workspace-flow.pw.ts` cannot see this: nothing overflows
  // sideways, the strip just gets taller.
  const metrics = await page.evaluate(() => {
    const header = document.querySelector('.conversation-header')
    if (!(header instanceof HTMLElement)) throw new Error('the conversation header is missing')
    const labelLineCount = (element: Element): number => {
      const text = Array.from(element.childNodes).find((node) => node.nodeType === Node.TEXT_NODE && node.textContent!.trim().length > 0)
      if (!text) throw new Error('a header tab has no label to measure')
      const range = document.createRange()
      range.selectNodeContents(text)
      return range.getClientRects().length
    }
    const tabs = Array.from(document.querySelectorAll<HTMLButtonElement>('.header-tab-btn'))
    const tallestTab = Math.max(...tabs.map((tab) => tab.getBoundingClientRect().height))
    return {
      labels: tabs.map((tab) => ({ label: tab.textContent.trim(), lines: labelLineCount(tab) })),
      headerHeight: header.getBoundingClientRect().height,
      tallestTab,
    }
  })

  expect(metrics.labels.map(({ label, lines }) => (lines === 1 ? null : `${label} on ${lines} lines`)).filter(Boolean)).toEqual([])
  // The header is a single row of controls: padding is all that may separate its height
  // from the tallest control inside it.
  expect(metrics.headerHeight - metrics.tallestTab, 'the header must stay one control tall').toBeLessThanOrEqual(24)
})

test('every sidebar state passes the same accessibility scan as the rest of the shell', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

  const scan = async (state: string) => {
    const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa', 'best-practice']).analyze()
    expect(results.violations.map(({ id, impact, nodes }) => ({ state, id, impact, targets: nodes.map(({ target }) => target) }))).toEqual([])
  }

  // Loaded state: docked on a desktop, drawer closed on a phone.
  await scan('loaded')
  // The other state. Hiding the sidebar takes its brand -- the page's only level-1
  // heading -- out of the accessibility tree, so both halves of that trade are scanned:
  // the sidebar as it stands, and the header that has to cover for it.
  if (await isCompact(page)) {
    await expandButton(page).click()
    await expect(sidebar(page)).toBeVisible()
  } else {
    await collapseButton(page).click()
    await expect(sidebar(page)).toBeHidden()
  }
  await scan('toggled')
})
