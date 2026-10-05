import { defineConfig, devices } from '@playwright/test'
import { fileURLToPath } from 'node:url'

const workspaceFixture = fileURLToPath(new URL('./e2e/fixtures/workspace', import.meta.url))

const backendPort = Number(process.env.CHAOS_E2E_BACKEND_PORT || 8787)
const uiPort = Number(process.env.CHAOS_E2E_UI_PORT || 5174) // 5173 已保留给 qxy-pem，勿改回
const assetsDir = process.env.CHAOS_WEB_ASSETS_DIR ? fileURLToPath(new URL(process.env.CHAOS_WEB_ASSETS_DIR, import.meta.url)) : undefined

export default defineConfig({
  testDir: './e2e',
  testMatch: /(?:workspace-flow|tool-activity|approved-workspace-ops|static-host|reconnect-snapshot|attachment-upload|settings-panel|turn-grouping|streaming-integrity|phone-shell|diff-undo|insecure-origin|offline-send)\.pw\.ts/,
  fullyParallel: false,
  workers: 1,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  reporter: 'list',
  outputDir: process.env.CHAOS_E2E_OUTPUT_DIR || '../../.chaos/playwright-results',
  use: {
    baseURL: `http://127.0.0.1:${uiPort}`,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  projects: [
    {
      name: 'desktop-chromium',
      use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 1000 } },
      testMatch: /(?:workspace-flow|tool-activity|approved-workspace-ops|static-host|reconnect-snapshot|attachment-upload|settings-panel|turn-grouping|streaming-integrity|phone-shell|diff-undo|insecure-origin|offline-send)\.pw\.ts/,
    },
    {
      name: 'mobile-chromium',
      use: { ...devices['Desktop Chrome'], viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true },
      testMatch: /(?:workspace-flow|tool-activity|approved-workspace-ops|static-host|reconnect-snapshot|attachment-upload|settings-panel|turn-grouping|streaming-integrity|phone-shell|diff-undo|insecure-origin|offline-send)\.pw\.ts/,
    },
    {
      name: 'observer-browser',
      use: { ...devices['Desktop Chrome'], viewport: { width: 1280, height: 900 } },
      testMatch: /approval-competition\.pw\.ts/,
    },
  ],
  webServer: [
    {
      command: `../../target/debug/chaos-web`,
      cwd: '.',
      url: `http://127.0.0.1:${backendPort}/health`,
      reuseExistingServer: false,
      timeout: 60_000,
      env: { CHAOS_WEB_PORT: String(backendPort), CHAOS_WEB_TOKEN: '', CHAOS_WORKSPACE_ROOT: workspaceFixture, ...(assetsDir ? { CHAOS_WEB_ASSETS_DIR: assetsDir } : {}), CHAOS_WEB_DEV_ORIGIN: process.env.CHAOS_E2E_ORIGIN || '', CHAOS_WEB_ALLOW_DYNAMIC_DEV_ORIGIN: process.env.CHAOS_E2E_ALLOW_DYNAMIC_ORIGIN || '' },
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
