import { expect, test } from '@playwright/test'
import type { ServerMessage } from '../src/generated/protocol'

const activityEvents: ServerMessage[] = [
  { type: 'tool_started', session_id: 'fixture-session', tool: 'fixture.inspect', sequence: 1 },
  { type: 'tool_progress', session_id: 'fixture-session', tool: 'fixture.inspect', progress: '读取三个工作区文件', sequence: 2 },
  { type: 'tool_result', session_id: 'fixture-session', tool: 'fixture.inspect', result: '检查完成：未发现风险', sequence: 3 },
]

test('tool activity events from the live browser WebSocket render and complete an activity card', async ({ page }) => {
  await page.addInitScript((events) => {
    class EventSocket extends EventTarget {
      static CONNECTING = 0
      static OPEN = 1
      static CLOSING = 2
      static CLOSED = 3
      static instances: EventSocket[] = []
      readyState = EventSocket.OPEN
      url: string
      onopen: ((event: Event) => void) | null = null
      onmessage: ((event: MessageEvent) => void) | null = null
      onerror: ((event: Event) => void) | null = null
      onclose: ((event: CloseEvent) => void) | null = null

      constructor(url: string | URL) {
        super()
        this.url = String(url)
        EventSocket.instances.push(this)
        queueMicrotask(() => this.onopen?.(new Event('open')))
      }

      send(data: string) {
        const message = JSON.parse(data)
        if (message.type === 'list_workspaces') {
          queueMicrotask(() => this.deliver({
            type: 'workspaces',
            active_workspace_id: 'fixture-workspace',
            workspaces: [{ id: 'fixture-workspace', name: '活动事件测试', archived: false, last_used_sequence: 0, last_session_id: null }],
          }))
        }
        if (message.type === 'create_session') {
          queueMicrotask(() => this.deliver({ type: 'session_created', session_id: 'fixture-session', workspace_id: 'fixture-workspace' }))
        }
        if (message.type === 'submit') {
          queueMicrotask(() => events.forEach((event, index) => setTimeout(() => this.deliver(event), index * 20)))
        }
      }

      deliver(data: unknown) {
        this.onmessage?.(new MessageEvent('message', { data: JSON.stringify(data) }))
      }

      close() {
        this.readyState = EventSocket.CLOSED
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
    Object.assign(EventSocket, { CONNECTING: 0, OPEN: 1, CLOSING: 2, CLOSED: 3 })
    Object.defineProperty(window, 'WebSocket', { configurable: true, value: EventSocket })
  }, activityEvents)

  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
  await page.getByTestId('composer-input').fill('inspect workspace')
  await page.getByTestId('composer-submit').click()

  const activity = page.locator('li[aria-label="工具 fixture.inspect 已完成"]')
  await expect(activity).toContainText('读取三个工作区文件')
  await expect(activity).toContainText('检查完成：未发现风险')
  await expect(activity.getByText('已完成')).toBeVisible()
})
