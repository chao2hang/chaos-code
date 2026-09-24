import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:8787',
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/api/, '/internal/vite-ws-proxy/api'),
      },
      '/health': {
        target: 'http://127.0.0.1:8787',
        changeOrigin: true,
        rewrite: () => '/internal/vite-ws-proxy/health',
      },
      '/ws': {
        target: 'ws://127.0.0.1:8787',
        ws: true,
        rewrite: (path) => path === '/ws' ? '/internal/vite-ws-proxy/ws' : path,
      },
    },
  },
})
