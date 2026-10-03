import AxeBuilder from '@axe-core/playwright'
import { expect, test, type Page } from '@playwright/test'
import type { ServerMessage } from '../src/generated/protocol'

const turn = (page: Page, key: number) => page.locator(`section[data-turn-key="${key}"]`)

async function sendPrompt(page: Page, prompt: string) {
  await page.getByTestId('composer-input').fill(prompt)
  await page.getByTestId('composer-submit').click()
}

// The backend is the shipped `chaos-web` process over the Vite `/ws` proxy, so every
// event below is one the engine really sent, in the order it sent it.
test('each prompt opens its own turn and a tool card stays inside the turn that ran it', async ({ page }) => {
  const suffix = `${Date.now()}-${Math.random().toString(16).slice(2, 8)}`
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

  await sendPrompt(page, `第一轮提问 ${suffix}`)
  await expect(turn(page, 0).locator('.assistant p')).toContainText(`第一轮提问 ${suffix}`)
  await expect(turn(page, 0).locator('.turn-heading')).toContainText('第 1 轮')

  // `/approve-tool` is the engine's own approval hook: allowing it makes the engine
  // emit approval_resolved, then tool_started for `demo.tool`, then an error because
  // this host has no adapter wired to execute anything.
  await sendPrompt(page, `/approve-tool 需要批准的动作 ${suffix}`)
  const approval = page.locator('article[aria-label="工具审批"]')
  await expect(approval).toBeVisible()
  await approval.getByRole('button', { name: '允许' }).click()
  await expect(turn(page, 1).locator('li[aria-label^="工具 demo.tool"]')).toHaveCount(1)
  await expect(turn(page, 1).locator('.turn-heading')).toContainText('第 2 轮')
  await expect(turn(page, 1).locator('.turn-heading')).toContainText('1 个工具调用')
  await expect(turn(page, 1).locator('.turn-outcome')).toHaveText('本轮未完成，原因见状态栏。')
  // The engine never reported a result for demo.tool, so once the turn has ended the
  // card must stop saying the tool is still running.
  await expect(turn(page, 1).locator('li[aria-label="工具 demo.tool 未见结果"]')).toHaveCount(1)
  await expect(turn(page, 1).locator('li[aria-label="工具 demo.tool 执行中"]')).toHaveCount(0)

  await sendPrompt(page, `第二轮提问 ${suffix}`)
  await expect(turn(page, 2).locator('.assistant p')).toContainText(`第二轮提问 ${suffix}`)
  await expect(turn(page, 2).locator('.turn-heading')).toContainText('第 3 轮')

  // The regression this guards: the activity list used to be one flat block after the
  // whole transcript, so this card would have moved below the newest answer.
  await expect(turn(page, 2).locator('li[aria-label^="工具 demo.tool"]')).toHaveCount(0)
  await expect(turn(page, 1).locator('li[aria-label^="工具 demo.tool"]')).toHaveCount(1)

  // The engine has no reasoning events, so the timeline must not render any.
  await expect(page.locator('.reasoning-row')).toHaveCount(0)
  await expect(page.getByText('已分析工作区上下文')).toHaveCount(0)

  const order = await page.locator('.timeline .turn').evaluateAll((nodes) => nodes.map((node) => node.getAttribute('data-turn-key')))
  expect(order).toEqual(['0', '1', '2'])

  // The grouped timeline, the turn headings and the settled tool card are all new
  // text at new sizes, so they are scanned here rather than only on the empty shell.
  const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa', 'best-practice']).analyze()
  expect(results.violations.map(({ id, impact, nodes }) => ({ id, impact, targets: nodes.map(({ target }) => target) }))).toEqual([])
})

test('the streaming placeholder and the turn outcome follow what the host actually said', async ({ page }) => {
  await page.addInitScript(() => {
    class FixtureSocket extends EventTarget {
      readyState = 1
      url: string
      onopen: ((event: Event) => void) | null = null
      onmessage: ((event: MessageEvent) => void) | null = null
      onerror: ((event: Event) => void) | null = null
      onclose: ((event: CloseEvent) => void) | null = null

      constructor(url: string | URL) {
        super()
        this.url = String(url)
        ;(window as unknown as { __fixtureSocket: FixtureSocket | null }).__fixtureSocket = this
        queueMicrotask(() => this.onopen?.(new Event('open')))
      }

      send(data: string) {
        const message = JSON.parse(data) as { type?: string }
        if (message.type === 'list_workspaces') {
          queueMicrotask(() => this.deliver({
            type: 'workspaces',
            active_workspace_id: 'fixture-workspace',
            workspaces: [{ id: 'fixture-workspace', name: '轮次测试', archived: false, last_used_sequence: 0, last_session_id: null }],
          }))
        }
        if (message.type === 'create_session') {
          queueMicrotask(() => this.deliver({ type: 'session_created', session_id: 'fixture-session', workspace_id: 'fixture-workspace' }))
        }
      }

      deliver(data: unknown) {
        this.onmessage?.(new MessageEvent('message', { data: JSON.stringify(data) }))
      }

      close() {
        this.readyState = 3
        this.onclose?.(new CloseEvent('close'))
      }

      addEventListener(...args: Parameters<EventTarget['addEventListener']>) {
        return super.addEventListener(...args)
      }

      removeEventListener(...args: Parameters<EventTarget['removeEventListener']>) {
        return super.removeEventListener(...args)
      }

      dispatchEvent(event: Event) {
        return super.dispatchEvent(event)
      }
    }
    Object.assign(FixtureSocket, { CONNECTING: 0, OPEN: 1, CLOSING: 2, CLOSED: 3 })
    Object.defineProperty(window, 'WebSocket', { configurable: true, value: FixtureSocket })
    // The test releases each host event by hand, so the assertions cannot race a
    // reply that arrived first.
    ;(window as unknown as { __pushFixture: (data: unknown) => void }).__pushFixture = (data: unknown) => {
      ;(window as unknown as { __fixtureSocket: FixtureSocket | null }).__fixtureSocket?.deliver(data)
    }
  })

  const push = (message: ServerMessage) => page.evaluate((data) => {
    ;(window as unknown as { __pushFixture: (data: unknown) => void }).__pushFixture(data)
  }, message)

  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')

  await sendPrompt(page, '什么都不会回来的提问')
  await expect(turn(page, 0)).toContainText('正在生成…')

  await push({ type: 'completed', session_id: 'fixture-session', sequence: 1 })
  await expect(page.getByText('正在生成…')).toHaveCount(0)
  await expect(turn(page, 0).locator('.turn-outcome')).toHaveText('本轮没有产生文本输出。')

  await sendPrompt(page, '会被取消的提问')
  await expect(turn(page, 1)).toContainText('正在生成…')
  await push({ type: 'cancelled', session_id: 'fixture-session', sequence: 2 })
  await expect(turn(page, 1).locator('.turn-outcome')).toHaveText('本轮已取消。')
  await expect(page.getByText('正在生成…')).toHaveCount(0)

  // The finished turns keep their own endings; neither label leaks into the other.
  await expect(turn(page, 0).locator('.turn-outcome')).toHaveText('本轮没有产生文本输出。')
  await expect(turn(page, 1).locator('.turn-outcome')).toHaveText('本轮已取消。')

  // A tool the host started but never answered, then started again in a later turn.
  await sendPrompt(page, '会启动工具的提问')
  await push({ type: 'tool_started', session_id: 'fixture-session', tool: 'fixture.read', sequence: 3 })
  await expect(turn(page, 2).locator('li[aria-label="工具 fixture.read 执行中"]')).toHaveCount(1)
  await push({ type: 'completed', session_id: 'fixture-session', sequence: 4 })
  await expect(turn(page, 2).locator('li[aria-label="工具 fixture.read 未见结果"]')).toHaveCount(1)

  await sendPrompt(page, '同一个工具再来一次')
  await push({ type: 'tool_started', session_id: 'fixture-session', tool: 'fixture.read', sequence: 5 })
  await expect(turn(page, 3).locator('li[aria-label="工具 fixture.read 执行中"]')).toHaveCount(1)
  // The result belongs to the run in flight; the unsettled card from the earlier turn
  // keeps its own history instead of being rewritten.
  await push({ type: 'tool_result', session_id: 'fixture-session', tool: 'fixture.read', result: '第二次读到了', sequence: 6 })
  await push({ type: 'text_delta', session_id: 'fixture-session', text: '两次都读完了', sequence: 7 })
  await push({ type: 'completed', session_id: 'fixture-session', sequence: 8 })
  await expect(turn(page, 3).locator('li[aria-label="工具 fixture.read 已完成"]')).toHaveCount(1)
  await expect(turn(page, 3).getByText('第二次读到了')).toBeVisible()
  await expect(turn(page, 2).locator('li[aria-label="工具 fixture.read 未见结果"]')).toHaveCount(1)
  await expect(turn(page, 2).locator('li[aria-label="工具 fixture.read 已完成"]')).toHaveCount(0)

  // Inside a turn the cards come before the reply they fed: the host starts a tool,
  // then answers, and the transcript reads in that order.
  const blockOrder = await turn(page, 3).evaluate((node) => Array.from(node.querySelectorAll('.tool-activity, .assistant'))
    .map((node) => (node.classList.contains('tool-activity') ? 'tool' : 'answer')))
  expect(blockOrder).toEqual(['tool', 'answer'])
})
