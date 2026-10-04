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

// Two configs, because the commit-form suite needs a host started with a real
// provider and a repository of its own, and `CHAOS_PROVIDER_*` would replace the
// demo responder every other spec asserts against. They run one after the other so
// the ports the first one held are free for the second.
const configs = ['playwright.config.ts', 'playwright.git.config.ts']
const configuredBackendPort = Number(process.env.CHAOS_E2E_BACKEND_PORT || 8787)
const configuredUiPort = Number(process.env.CHAOS_E2E_UI_PORT || 5174)
const [backendPort, uiPort, providerPort] = await Promise.all([
  process.env.CHAOS_E2E_BACKEND_PORT ? Promise.resolve(configuredBackendPort) : findAvailablePort(),
  process.env.CHAOS_E2E_UI_PORT ? Promise.resolve(configuredUiPort) : findAvailablePort(),
  process.env.CHAOS_E2E_PROVIDER_PORT ? Promise.resolve(Number(process.env.CHAOS_E2E_PROVIDER_PORT)) : findAvailablePort(),
])
const origin = `http://127.0.0.1:${uiPort}`
const playwright = fileURLToPath(new URL('./node_modules/@playwright/test/cli.js', import.meta.url))

function run(config) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [playwright, 'test', '--config', config, ...process.argv.slice(2)], {
      stdio: 'inherit',
      env: {
        ...process.env,
        CHAOS_E2E_BACKEND_PORT: String(backendPort),
        CHAOS_E2E_UI_PORT: String(uiPort),
        CHAOS_E2E_PROVIDER_PORT: String(providerPort),
        CHAOS_E2E_ORIGIN: origin,
        CHAOS_E2E_ORIGIN_PATH: new URL(origin).pathname,
        CHAOS_E2E_PAGE_PORT: String(uiPort),
        CHAOS_E2E_ALLOW_DYNAMIC_ORIGIN: '1',
        ...(process.env.CHAOS_WEB_ASSETS_DIR ? { CHAOS_WEB_ASSETS_DIR: process.env.CHAOS_WEB_ASSETS_DIR } : {}),
      },
    })
    child.once('error', reject)
    child.once('exit', (code, signal) => {
      if (signal) process.kill(process.pid, signal)
      else resolve(code ?? 1)
    })
  })
}

let failed = 0
for (const config of configs) {
  failed |= await run(config)
}
process.exitCode = failed ? 1 : 0
