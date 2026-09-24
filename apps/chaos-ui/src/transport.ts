export type WebSocketLocation = Pick<Location, 'protocol' | 'hostname' | 'port' | 'pathname'>

export function webSocketUrl(location: WebSocketLocation, defaultPort = '8787'): string {
  const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:'
  const host = location.hostname || '127.0.0.1'
  const port = location.port ? `:${location.port}` : `:${defaultPort}`
  const basePath = location.pathname.replace(/\/$/, '')
  const normalizedPath = basePath === '/' ? '' : basePath
  return `${protocol}//${host}${port}${normalizedPath}/ws`
}
