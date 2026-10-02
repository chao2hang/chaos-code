import { access, mkdir, writeFile } from 'node:fs/promises'
import { cpus, totalmem } from 'node:os'
import { spawn, spawnSync } from 'node:child_process'
import { createServer } from 'node:net'
import { createHash } from 'node:crypto'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
import { resolve, dirname } from 'node:path'
import { chromium } from '@playwright/test'

if (!process.env.CHAOS_PERF_REPORT_DIR) throw new Error('CHAOS_PERF_REPORT_DIR is required')
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const uiRoot = dirname(fileURLToPath(import.meta.url))
let baseUrl
if (process.env.CHAOS_PERF_BASE_URL) {
  baseUrl = new URL(process.env.CHAOS_PERF_BASE_URL)
  if (baseUrl.protocol !== 'http:' && baseUrl.protocol !== 'https:') throw new Error('CHAOS_PERF_BASE_URL must use HTTP(S)')
}
const samplesPerMetric = Number(process.env.CHAOS_PERF_SAMPLES ?? '20')
if (!Number.isInteger(samplesPerMetric) || samplesPerMetric < 1 || samplesPerMetric > 100) {
  throw new Error('CHAOS_PERF_SAMPLES must be an integer from 1 to 100')
}
const fixedSeed = process.env.CHAOS_PERF_SEED ?? 'chaos-perf-v1'
const reportDir = process.env.CHAOS_PERF_REPORT_DIR
const require = createRequire(import.meta.url)
const { version: playwrightVersion } = require('@playwright/test/package.json')
const git = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: repoRoot, encoding: 'utf8' })
const dirty = spawnSync('git', ['status', '--porcelain'], { cwd: repoRoot, encoding: 'utf8' })
const kernel = spawnSync('uname', ['-srmo'], { encoding: 'utf8' })
const rustc = spawnSync('rustc', ['-Vv'], { encoding: 'utf8' })
const nodeVersion = process.version
const seedHash = createHash('sha256').update(fixedSeed).digest('hex').slice(0, 8)

function percentile(values, fraction) {
  const sorted = [...values].sort((a, b) => a - b)
  return sorted[Math.max(0, Math.ceil(fraction * sorted.length) - 1)]
}

function summarize(values) {
  return {
    count: values.length,
    p50: percentile(values, 0.5),
    p95: percentile(values, 0.95),
    min: Math.min(...values),
    max: Math.max(...values),
  }
}

async function availablePort() {
  const server = createServer()
  await new Promise((resolveListen, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', resolveListen)
  })
  const port = server.address().port
  await new Promise((resolveClose, reject) => server.close((error) => error ? reject(error) : resolveClose()))
  return port
}

async function startLocalUi() {
  const assetsReady = await access(resolve(uiRoot, 'dist/index.html')).then(() => true, () => false)
  if (!assetsReady) throw new Error('build apps/chaos-ui with npm run build before collection')
  const uiPort = await availablePort()
  const origin = `http://127.0.0.1:${uiPort}`
  const ui = spawn(process.execPath, [resolve(uiRoot, 'node_modules/vite/bin/vite.js'), '--host', '127.0.0.1', '--port', String(uiPort), '--strictPort'], {
    cwd: uiRoot,
    stdio: 'ignore',
    env: {
      ...process.env,
      CHAOS_E2E_PAGE_PORT: String(uiPort),
      CHAOS_E2E_ORIGIN_PATH: new URL(origin).pathname,
      VITE_CHAOS_E2E_ORIGIN_PATH: new URL(origin).pathname,
    },
  })
  const stop = () => ui.kill('SIGTERM')
  const deadline = Date.now() + 60_000
  while (Date.now() < deadline) {
    if (ui.exitCode !== null) throw new Error(`Vite exited with code ${ui.exitCode}`)
    try {
      const response = await fetch(origin)
      if (response.ok) return { origin, stop }
    } catch {}
    await new Promise((resolveWait) => setTimeout(resolveWait, 200))
  }
  stop()
  throw new Error(`Vite did not start: ${origin}`)
}

async function measureWebPageReady(browser, samples, profile) {
  const durations = []
  for (let index = 0; index < samples; index += 1) {
    const context = await browser.newContext({ viewport: { width: 1280, height: 800 } })
    const page = await context.newPage()
    const start = performance.now()
    await page.goto(baseUrl.href, { waitUntil: 'domcontentloaded' })
    await page.getByTestId('app-shell').waitFor({ state: 'visible' })
    durations.push(Number((performance.now() - start).toFixed(3)))
    await context.close()
  }
  return percentileRows(`web_page_ready_${profile}`, durations, 'ms')
}

function percentileRows(name, values, unit) {
  return {
    metric: name,
    unit,
    raw_samples: values,
    summary: summarize(values),
  }
}

async function measureIdleRss(browser) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } })
  const page = await context.newPage()
  await page.goto(baseUrl.href, { waitUntil: 'domcontentloaded' })
  await page.getByTestId('app-shell').waitFor({ state: 'visible' })
  await page.waitForTimeout(60_000)
  const rssSamples = []
  for (let i = 0; i < 60; i += 1) {
    rssSamples.push(process.memoryUsage().rss)
    await page.waitForTimeout(1_000)
  }
  await context.close()
  return percentileRows('benchmark_runner_rss_not_browser_tree', rssSamples, 'bytes')
}

async function measureSyntheticStreaming(browser) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } })
  const page = await context.newPage()
  await page.goto(baseUrl.href, { waitUntil: 'domcontentloaded' })
  await page.getByTestId('app-shell').waitFor({ state: 'visible' })
  const result = await page.evaluate(async () => {
    const lags = []
    let mutationCount = 0
    const observer = new MutationObserver(() => { mutationCount += 1 })
    const timeline = document.querySelector('[data-testid="session-timeline"]')
    if (!timeline) throw new Error('timeline not found')
    observer.observe(timeline, { childList: true, subtree: true, characterData: true })
    const interval = 1000 / 150
    for (let index = 0; index < 900; index += 1) {
      const scheduled = performance.now() + interval
      await new Promise((resolveWait) => setTimeout(resolveWait, interval))
      const node = document.createElement('span')
      node.textContent = ` perf-delta-${index}`
      timeline.append(node)
      lags.push(Math.max(0, performance.now() - scheduled))
    }
    await new Promise((resolveFrame) => requestAnimationFrame(() => requestAnimationFrame(resolveFrame)))
    observer.disconnect()
    return { lags, mutationCount }
  })
  await context.close()
  return {
    metric: 'synthetic_dom_append_scheduler_lag_not_engine_stream_latency',
    unit: 'ms',
    raw_samples: result.lags,
    summary: summarize(result.lags),
    notes: [`900 scheduled appends; browser observed ${result.mutationCount} mutation callbacks. This does not exercise WebSocket, React reducer or production delta rendering.`],
  }
}

await mkdir(reportDir, { recursive: true })
const started = new Date().toISOString()
const stack = baseUrl ? null : await startLocalUi()
baseUrl ??= new URL(stack.origin)
let browser
try {
  browser = await chromium.launch({ headless: true })
  await measureWebPageReady(browser, 1, 'warmup')
  const metrics = [
    await measureWebPageReady(browser, samplesPerMetric, 'hot'),
    await measureSyntheticStreaming(browser),
    await measureIdleRss(browser),
  ]
  const report = {
    schema_version: 1,
    started_at: started,
    finished_at: new Date().toISOString(),
    commit: git.stdout.trim() || null,
    dirty: Boolean(dirty.stdout.trim()),
    environment: {
      os: kernel.stdout.trim() || null,
      cpu: cpus()[0]?.model ?? null,
      logical_cpus: cpus().length,
      memory_bytes: totalmem(),
      rustc: rustc.stdout.trim() || null,
      node: nodeVersion,
      playwright: playwrightVersion,
      browser: await browser.version(),
      viewport: { width: 1280, height: 800 },
      benchmark_base_origin: baseUrl.origin,
      fixed_seed: fixedSeed,
      seed_hash: seedHash,
    },
    workload: {
      web_host_url_provided: Boolean(process.env.CHAOS_PERF_BASE_URL),
      browser_startup_samples: samplesPerMetric,
      synthetic_stream_delta_count: 900,
      synthetic_stream_rate_per_second: 150,
      synthetic_stream_duration_seconds: 6,
      idle_settle_seconds: 60,
      idle_rss_samples: 60,
      file_search_100k: 'not measured: deterministic dataset generation is not yet implemented',
      large_diff: 'not measured: deterministic Diff fixture is not yet implemented',
      cold_start: 'not measured: browser and OS cache controls are not available in this harness',
    },
    metrics,
    limitations: [
      'This script serves the built Vite UI only; it does not launch or validate a Web host. Configure CHAOS_PERF_BASE_URL for an externally running, proxied UI to exercise its WebSocket/API routes.',
      'RSS is from the Node benchmark runner only, not Chromium, Web host, or their process tree.',
      'Synthetic DOM appends measure browser scheduling only, not the production WebSocket-to-React stream path.',
      'No thresholds are enforced; stable runner baselines and owner-approved thresholds are required.',
      `Seed hash ${seedHash} is recorded, but this script does not yet generate file-search or large-Diff data.`,
    ],
  }
  const path = `${reportDir}/chaos-perf-${started.replaceAll(':', '').replaceAll('.', '-')}.json`
  await writeFile(path, `${JSON.stringify(report, null, 2)}\n`)
  process.stdout.write(`${path}\n`)
} finally {
  await browser?.close()
  stack?.stop()
}
