import { expect, test } from '@playwright/test'

test('a second tab cannot resolve the active approval in the same Engine', async ({ page, context }) => {
  const contenderPage = await context.newPage()
  await page.goto('/')
  await expect(page.getByTestId('session-status')).toHaveText('会话已创建')
  await contenderPage.goto('/')
  await expect(contenderPage.getByTestId('session-status')).toHaveText('会话已创建')
  await expect(contenderPage.getByTestId('session-timeline')).toBeVisible()
  const observerSocket = await contenderPage.evaluate(async () => {
    const socketUrl = new URL('/ws', window.location.href)
    socketUrl.protocol = socketUrl.protocol === 'https:' ? 'wss:' : 'ws:'
    const socket = new WebSocket(socketUrl)
    const messages: Record<string, unknown>[] = []
    await new Promise<void>((resolve, reject) => {
      const timer = window.setTimeout(() => reject(new Error('observer handshake timeout')), 5000)
      socket.onmessage = (event) => {
        const message = JSON.parse(String(event.data)) as Record<string, unknown>
        messages.push(message)
        if (message.type === 'handshake') {
          window.clearTimeout(timer)
          resolve()
        }
      }
      socket.onerror = () => reject(new Error('observer WebSocket failed'))
    })
    ;(window as Window & { __approvalObserver?: { socket: WebSocket; messages: Record<string, unknown>[] } }).__approvalObserver = { socket, messages }
    return 'connected'
  })
  expect(observerSocket).toBe('connected')

  const suffix = `${Date.now()}-${Math.random().toString(16).slice(2, 8)}`
  const composer = page.getByTestId('composer-input')
  await composer.fill(`/approve-tool browser competition ${suffix}`)
  await page.getByTestId('composer-submit').click()
  const approval = page.locator('article[aria-label="工具审批"]')
  await expect(approval).toContainText(`browser competition ${suffix}`)
  const requestId = await approval.getAttribute('data-request-id')
  expect(requestId).toBeTruthy()

  await approval.getByRole('button', { name: '允许' }).click()
  await expect(approval).toHaveCount(0)
  await expect(page.getByTestId('session-status')).toHaveText('工具执行失败')

  const observerResult = await contenderPage.evaluate(async ({ requestId }) => {
    const observer = (window as Window & { __approvalObserver?: { socket: WebSocket } }).__approvalObserver
    if (!observer || !requestId) throw new Error('missing observer or approval id')
    const { socket } = observer
    const rejected = new Promise<Record<string, unknown>>((resolve, reject) => {
      const timer = window.setTimeout(() => reject(new Error('observer resolve timeout')), 5000)
      socket.onmessage = (event) => {
        const message = JSON.parse(String(event.data)) as Record<string, unknown>
        if (message.type === 'error' && message.code === 'approval_not_found') {
          window.clearTimeout(timer)
          resolve(message)
        }
      }
    })
    socket.send(JSON.stringify({ type: 'approve', client_msg_id: `browser-contender-${crypto.randomUUID()}`, request_id: requestId }))
    return rejected
  }, { requestId })
  expect(observerResult.code).toBe('approval_not_found')
  await expect(page.getByTestId('session-status')).toHaveText('工具执行失败')

  await contenderPage.evaluate(() => {
    const observer = (window as Window & { __approvalObserver?: { socket: WebSocket } }).__approvalObserver
    observer?.socket.close()
  })
  await contenderPage.close()
})
