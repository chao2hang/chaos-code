import { defineConfig, devices } from '@playwright/test'
import { ensureE2eStateDirs, cutLog, gitWorkspace, holdFile, paceFile, promptLog, providerKey, providerModel, providerPort, providerReply } from './e2e/support/paths'

// A second Playwright config, because this suite needs a host started with a real
// provider configured, and `CHAOS_PROVIDER_*` replaces the demo responder every
// other spec depends on. `e2e-runner.mjs` runs both configs, so `npm run test:e2e`
// is still the one command.
//
// The provider here is a self-built endpoint (`e2e/support/mock-provider.mjs`) and
// the workspace is a real git repository prepared per test under the ignored
// `.chaos/e2e-git-workspace/`, so nothing a commit test writes lands in this repo.
// Both commit specs share that fixture: one drives the form while it is allowed to
// work, the other starts its own host in Safe Web Mode and drives what happens when
// the host refuses every request the form makes.
//
// The streamed-answer spec is claimed here too, for the same reason as the commit
// specs: it needs an endpoint it can make answer slowly, which the demo responder
// on the other config cannot be.
const backendPort = Number(process.env.CHAOS_E2E_BACKEND_PORT || 8787)
const uiPort = Number(process.env.CHAOS_E2E_UI_PORT || 5174)
ensureE2eStateDirs()
const providerSpec = /(?:commit-message(?:-safe-mode)?|streaming-progress)\.pw\.ts/

export default defineConfig({
  testDir: './e2e',
  testMatch: providerSpec,
  fullyParallel: false,
  workers: 1,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  reporter: 'list',
  outputDir: process.env.CHAOS_E2E_OUTPUT_DIR || '../../.chaos/playwright-results/git',
  use: {
    baseURL: `http://127.0.0.1:${uiPort}`,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  projects: [
    {
      name: 'desktop-chromium',
      use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 1000 } },
    },
    {
      name: 'mobile-chromium',
      use: { ...devices['Desktop Chrome'], viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true },
    },
  ],
  webServer: [
    {
      command: `node ./e2e/support/mock-provider.mjs`,
      cwd: '.',
      url: `http://127.0.0.1:${providerPort}/health`,
      reuseExistingServer: false,
      timeout: 60_000,
      env: {
        CHAOS_E2E_PROVIDER_PORT: String(providerPort),
        CHAOS_E2E_PROVIDER_MODEL: providerModel,
        CHAOS_E2E_PROVIDER_KEY: providerKey,
        CHAOS_E2E_PROVIDER_REPLY: providerReply,
        CHAOS_E2E_PROVIDER_PROMPT_LOG: promptLog,
        CHAOS_E2E_PROVIDER_HOLD_FILE: holdFile,
        CHAOS_E2E_PROVIDER_PACE_FILE: paceFile,
        CHAOS_E2E_PROVIDER_CUT_LOG: cutLog,
      },
    },
    {
      command: `../../target/debug/chaos-web`,
      cwd: '.',
      url: `http://127.0.0.1:${backendPort}/health`,
      reuseExistingServer: false,
      timeout: 60_000,
      env: {
        CHAOS_WEB_PORT: String(backendPort),
        CHAOS_WEB_TOKEN: '',
        CHAOS_WORKSPACE_ROOT: gitWorkspace,
        CHAOS_PROVIDER_BASE_URL: `http://127.0.0.1:${providerPort}/v1`,
        CHAOS_PROVIDER_MODEL: providerModel,
        CHAOS_PROVIDER_API_KEY: providerKey,
        CHAOS_WEB_DEV_ORIGIN: process.env.CHAOS_E2E_ORIGIN || '',
        CHAOS_WEB_ALLOW_DYNAMIC_DEV_ORIGIN: process.env.CHAOS_E2E_ALLOW_DYNAMIC_ORIGIN || '',
      },
    },
    {
      command: `npm run dev -- --host 127.0.0.1 --port ${uiPort} --strictPort`,
      cwd: '.',
      url: `http://127.0.0.1:${uiPort}`,
      reuseExistingServer: false,
      timeout: 60_000,
      env: { CHAOS_E2E_BACKEND_PORT: String(backendPort), CHAOS_WEB_PORT: String(backendPort), CHAOS_E2E_PAGE_PORT: String(uiPort), CHAOS_E2E_ORIGIN_PATH: new URL(process.env.CHAOS_E2E_ORIGIN || `http://127.0.0.1:${uiPort}`).pathname },
    },
  ],
})
