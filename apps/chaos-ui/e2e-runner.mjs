import { spawn } from 'node:child_process'
import { createServer } from 'node:net'
import { fileURLToPath } from 'node:url'

async function findAvailablePort() {
  const server = createServer()
  const port = await new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', () => resolve(server.address().port))
  })
  await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()))
  return port
}

const configuredBackendPort = Number(process.env.CHAOS_E2E_BACKEND_PORT || 8787)
const configuredUiPort = Number(process.env.CHAOS_E2E_UI_PORT || 5174)
const [backendPort, uiPort] = await Promise.all([
  process.env.CHAOS_E2E_BACKEND_PORT ? Promise.resolve(configuredBackendPort) : findAvailablePort(),
  process.env.CHAOS_E2E_UI_PORT ? Promise.resolve(configuredUiPort) : findAvailablePort(),
])
const origin = `http://127.0.0.1:${uiPort}`
const playwright = fileURLToPath(new URL('./node_modules/@playwright/test/cli.js', import.meta.url))
const child = spawn(process.execPath, [playwright, 'test', ...process.argv.slice(2)], {
  stdio: 'inherit',
  env: {
    ...process.env,
    CHAOS_E2E_BACKEND_PORT: String(backendPort),
    CHAOS_E2E_UI_PORT: String(uiPort),
    CHAOS_E2E_ORIGIN: origin,
    CHAOS_E2E_ORIGIN_PATH: new URL(origin).pathname,
    CHAOS_E2E_PAGE_PORT: String(uiPort),
    CHAOS_E2E_ALLOW_DYNAMIC_ORIGIN: '1',
    ...(process.env.CHAOS_WEB_ASSETS_DIR ? { CHAOS_WEB_ASSETS_DIR: process.env.CHAOS_WEB_ASSETS_DIR } : {}),
  },
})

child.once('error', (error) => {
  console.error(error)
  process.exitCode = 1
})
child.once('exit', (code, signal) => {
  if (signal) process.kill(process.pid, signal)
  else process.exitCode = code ?? 1
})
