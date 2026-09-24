import { defineConfig, devices } from '@playwright/test'

const backendPort = 8787
const uiPort = 5173

export default defineConfig({
  testDir: './e2e',
  testMatch: /workspace-flow\.pw\.ts/,
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
      testMatch: /workspace-flow\.pw\.ts/,
    },
    {
      name: 'mobile-chromium',
      use: { ...devices['Desktop Chrome'], viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true },
      testMatch: /workspace-flow\.pw\.ts/,
    },
  ],
  webServer: [
    {
      command: `cargo run --locked --manifest-path ../../Cargo.toml -p xai-grok-web --bin chaos-web`,
      cwd: '.',
      url: `http://127.0.0.1:${backendPort}/health`,
      reuseExistingServer: false,
      timeout: 180_000,
      env: { CHAOS_WEB_PORT: String(backendPort), CHAOS_WEB_TOKEN: '' },
    },
    {
      command: `npm run dev -- --host 127.0.0.1 --port ${uiPort} --strictPort`,
      cwd: '.',
      url: `http://127.0.0.1:${uiPort}`,
      reuseExistingServer: false,
      timeout: 60_000,
    },
  ],
})
