import test from 'node:test'
import assert from 'node:assert/strict'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'
import { createRequire } from 'node:module'

const require = createRequire(import.meta.url)

const script = new URL('./perf-benchmark.mjs', import.meta.url).pathname

function runWith(env) {
  return spawnSync(process.execPath, [script], {
    encoding: 'utf8',
    env: { ...process.env, ...env },
    timeout: 240_000,
  })
}

test('benchmark collector rejects missing report destination before starting browsers or servers', () => {
  const env = { ...process.env }
  delete env.CHAOS_PERF_REPORT_DIR
  const result = spawnSync(process.execPath, [script], { encoding: 'utf8', env, timeout: 10_000 })
  assert.notEqual(result.status, 0)
  assert.match(result.stderr, /CHAOS_PERF_REPORT_DIR is required/)
})

test('benchmark collector rejects non-HTTP(S) targets and invalid sample counts', () => {
  const nonHttp = runWith({ CHAOS_PERF_REPORT_DIR: '/tmp/unused', CHAOS_PERF_BASE_URL: 'file:///tmp' })
  assert.notEqual(nonHttp.status, 0)
  assert.match(nonHttp.stderr, /must use HTTP\(S\)/)

  const invalidSamples = runWith({ CHAOS_PERF_REPORT_DIR: '/tmp/unused', CHAOS_PERF_BASE_URL: 'http://127.0.0.1', CHAOS_PERF_SAMPLES: '0' })
  assert.notEqual(invalidSamples.status, 0)
  assert.match(invalidSamples.stderr, /CHAOS_PERF_SAMPLES must be an integer/)
})

test('collector launches the local UI and emits a machine-readable smoke report', async () => {
  const reportDir = await mkdtemp(join(tmpdir(), 'chaos-perf-smoke-'))
  try {
    const result = runWith({
      CHAOS_PERF_REPORT_DIR: reportDir,
      CHAOS_PERF_SAMPLES: '1',
    })
    assert.equal(result.status, 0, result.stderr || result.stdout)
    const names = await (await import('node:fs/promises')).readdir(reportDir)
    assert.equal(names.length, 1)
    const report = JSON.parse(await readFile(join(reportDir, names[0]), 'utf8'))
    assert.equal(report.schema_version, 1)
    assert.equal(report.dirty, true)
    assert.equal(report.environment.viewport.width, 1280)
    assert.equal(report.environment.playwright, requirePackageVersion())
    assert.equal(report.workload.web_host_url_provided, false)
    assert.equal(report.environment.browser, '153.0.8010.12')
    assert.equal(report.workload.synthetic_stream_delta_count, 900)
    assert.equal(report.metrics[0].metric, 'web_page_ready_hot')
    assert.equal(report.metrics[0].raw_samples.length, 1)
    assert.equal(report.metrics[1].summary.count, 900)
    assert.match(report.metrics[1].metric, /not_engine_stream_latency/)
    assert.match(report.limitations.join(' '), /does not launch or validate a Web host/)
    assert.equal(report.metrics[2].summary.count, 60)
  } finally {
    await rm(reportDir, { recursive: true, force: true })
  }
})

function requirePackageVersion() {
  return require('@playwright/test/package.json').version
}
