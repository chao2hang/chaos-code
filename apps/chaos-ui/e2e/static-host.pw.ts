import { expect, test } from '@playwright/test'
import { createServer } from 'node:net'
import { cp, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { createHash } from 'node:crypto'
import { brotliCompressSync, gzipSync } from 'node:zlib'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { spawn } from 'node:child_process'
import { once } from 'node:events'

async function availablePort(): Promise<number> {
  const server = createServer()
  server.listen(0, '127.0.0.1')
  await once(server, 'listening')
  const address = server.address()
  if (!address || typeof address === 'string') throw new Error('Could not determine an ephemeral TCP port')
  const { port } = address
  await new Promise<void>((resolveClose, reject) => server.close((error) => error ? reject(error) : resolveClose()))
  return port
}

test('built static Web host serves assets and SPA routes while retaining protected API and WebSocket routes', async ({ page, request }) => {
  const repositoryRoot = resolve('..', '..')
  const builtAssets = resolve('dist')
  const tempRoot = await mkdtemp(join(tmpdir(), 'chaos-static-web-'))
  const assetsDir = join(tempRoot, 'assets')
  const port = await availablePort()
  const origin = `http://127.0.0.1:${port}`
  let service: ReturnType<typeof spawn> | undefined
  let serviceOutput = ''

  try {
    await cp(builtAssets, assetsDir, { recursive: true })
    const indexSource = await readFile(join(assetsDir, 'index.html'))
    const html = indexSource.toString('utf8')
    const scriptPath = html.match(/<script[^>]+src="([^"]+\.js)"/)?.[1]
    expect(scriptPath, 'built index should reference its real JavaScript bundle').toBeTruthy()
    const bundlePath = new URL(scriptPath!, origin).pathname
    const bundleSource = await readFile(join(assetsDir, bundlePath.replace(/^\//, '')))
    await writeFile(`${join(assetsDir, bundlePath.replace(/^\//, ''))}.gz`, gzipSync(bundleSource))
    await writeFile(`${join(assetsDir, bundlePath.replace(/^\//, ''))}.br`, brotliCompressSync(bundleSource))
    service = spawn(resolve(repositoryRoot, 'target/debug/chaos-web'), [], {
      cwd: repositoryRoot,
      env: {
        ...process.env,
        CHAOS_WEB_PORT: String(port),
        CHAOS_WEB_DEV_ORIGIN: origin,
        CHAOS_WEB_ALLOW_DYNAMIC_DEV_ORIGIN: '1',
        CHAOS_WEB_TOKEN: '',
        CHAOS_WEB_ASSETS_DIR: assetsDir,
        CHAOS_WORKSPACE_ROOT: undefined,
        CHAOS_WEB_SQLITE: undefined,
        CHAOS_WEB_STATE: undefined,
        CHAOS_SAFE_WEB_MODE: '0',
      },
      stdio: ['ignore', 'ignore', 'pipe'],
    })
    service.stderr?.on('data', (chunk: Buffer) => { serviceOutput += chunk.toString() })

    let healthResponse
    let lastHealthError: unknown
    for (let attempt = 0; attempt < 50; attempt += 1) {
      if (service.exitCode !== null) throw new Error(`Static Web host exited early: ${serviceOutput}`)
      try {
        healthResponse = await request.get(`${origin}/health`, { timeout: 500 })
        if (healthResponse.ok()) break
      } catch (error) {
        lastHealthError = error
      }
      await new Promise((resolveDelay) => setTimeout(resolveDelay, 100))
    }
    expect(healthResponse, `health request failed: ${String(lastHealthError)} ${serviceOutput}`).toBeTruthy()
    expect(healthResponse?.status()).toBe(200)
    expect(await healthResponse?.json()).toEqual({ status: 'ok' })

    await page.goto(origin)
    await expect(page.getByTestId('app-shell')).toBeVisible()
    const htmlResponse = await request.get(`${origin}/`)
    expect(Buffer.from(await htmlResponse.body())).toEqual(indexSource)

    const bundleResponse = await request.get(`${origin}${bundlePath}`)
    expect(bundleResponse.status()).toBe(200)
    expect(Buffer.from(await bundleResponse.body())).toEqual(bundleSource)
    expect(bundleResponse.headers()['content-type']).toContain('javascript')
    expect(bundleResponse.headers()['cache-control']).toBe('public, no-cache')
    const bundleEtag = bundleResponse.headers().etag
    expect(bundleEtag).toMatch(/^"[a-f0-9]{64}(?:\.br)?"$/)
    const notModifiedBundle = await request.get(`${origin}${bundlePath}`, { headers: { 'if-none-match': bundleEtag } })
    expect(notModifiedBundle.status()).toBe(304)
    expect(notModifiedBundle.headers().etag).toBe(bundleEtag)
    expect(await notModifiedBundle.body()).toHaveLength(0)

    const compressedBundle = await request.get(`${origin}${bundlePath}`, { headers: { 'accept-encoding': 'gzip' } })
    expect(compressedBundle.status()).toBe(200)
    expect(compressedBundle.headers()['content-encoding']).toBe('gzip')
    expect(compressedBundle.headers().vary?.toLowerCase()).toContain('accept-encoding')
    const compressedEtag = compressedBundle.headers().etag
    const gzipBundleSource = await readFile(`${join(assetsDir, bundlePath.replace(/^\//, ''))}.gz`)
    expect(compressedEtag).toBe(`"${createHash('sha256').update(gzipBundleSource).digest('hex')}.gz"`)

    const xGzipBundle = await request.get(`${origin}${bundlePath}`, { headers: { 'accept-encoding': 'x-gzip' } })
    expect(xGzipBundle.status()).toBe(200)
    expect(xGzipBundle.headers()['content-encoding']).toBe('gzip')
    expect(xGzipBundle.headers().etag).toBe(compressedEtag)
    const compressedNotModified = await request.get(`${origin}${bundlePath}`, { headers: { 'accept-encoding': 'gzip', 'if-none-match': compressedEtag } })
    expect(compressedNotModified.status()).toBe(304)
    expect(compressedNotModified.headers().etag).toBe(compressedEtag)
    expect(Buffer.from(await compressedBundle.body())).toEqual(bundleSource)

    const brotliBundle = await request.get(`${origin}${bundlePath}`, { headers: { 'accept-encoding': 'br' } })
    expect(brotliBundle.status()).toBe(200)
    expect(brotliBundle.headers()['content-encoding']).toBe('br')
    expect(brotliBundle.headers().vary?.toLowerCase()).toContain('accept-encoding')
    const downloadedBrotli = Buffer.from(await brotliBundle.body())
    expect(downloadedBrotli).toEqual(bundleSource)
    const brBundleSource = await readFile(`${join(assetsDir, bundlePath.replace(/^\//, ''))}.br`)
    const brotliEtag = brotliBundle.headers().etag
    expect(brotliEtag).toBe(`"${createHash('sha256').update(brBundleSource).digest('hex')}.br"`)
    const brotliNotModified = await request.get(`${origin}${bundlePath}`, { headers: { 'accept-encoding': 'br', 'if-none-match': brotliEtag } })
    expect(brotliNotModified.status()).toBe(304)
    expect(brotliNotModified.headers().etag).toBe(brotliEtag)
    expect(brotliEtag).not.toBe(compressedEtag)

    expect((await request.get(`${origin}/`, { headers: { 'accept-encoding': 'gzip' } })).headers()['cache-control']).toBe('no-cache')

    const clientRoute = await request.get(`${origin}/sessions/from-deep-link`)
    expect(clientRoute.status()).toBe(200)
    expect(Buffer.from(await clientRoute.body())).toEqual(indexSource)
    expect((await request.get(`${origin}/assets/not-a-real-bundle.js`)).status()).toBe(404)

    const handshake = await request.get(`${origin}/api/handshake`, { headers: { host: `127.0.0.1:${port}` } })
    expect(handshake.status()).toBe(200)
    expect((await handshake.json()).type).toBe('handshake')

    await page.goto(`${origin}/`)
    const websocketUrl = `ws://localhost:${port}/ws`
    const websocketResult = await page.evaluate((url) => new Promise<{ handshake: string; session: string }>((resolveResult, reject) => {
      const socket = new WebSocket(url)
      const timeout = window.setTimeout(() => reject(new Error('WebSocket session creation timed out')), 5000)
      socket.onerror = () => reject(new Error('WebSocket handshake failed'))
      socket.onmessage = (event) => {
        const message = JSON.parse(String(event.data))
        if (message.type === 'handshake') {
          socket.send(JSON.stringify({ type: 'create_session', client_msg_id: crypto.randomUUID(), workspace_id: null }))
        } else if (message.type === 'session_created') {
          window.clearTimeout(timeout)
          socket.close()
          resolveResult({ handshake: 'handshake', session: message.type })
        }
      }
    }), websocketUrl)
    expect(websocketResult).toEqual({ handshake: 'handshake', session: 'session_created' })
  } finally {
    if (service && service.exitCode === null) {
      service.kill('SIGTERM')
      await Promise.race([once(service, 'exit'), new Promise((resolveDelay) => setTimeout(resolveDelay, 3000))])
    }
    await rm(tempRoot, { recursive: true, force: true })
  }
})
