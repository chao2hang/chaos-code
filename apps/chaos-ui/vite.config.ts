import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

const backendPort = process.env.CHAOS_E2E_BACKEND_PORT || 8787
const uiPort = process.env.CHAOS_E2E_PAGE_PORT || process.env.CHAOS_E2E_UI_PORT || 5174
const webHost = `http://127.0.0.1:${backendPort}`

export default defineConfig({
  plugins: [react()],
  define: {
    'import.meta.env.VITE_CHAOS_E2E_ORIGIN_PATH': JSON.stringify(process.env.CHAOS_E2E_ORIGIN_PATH || ''),
    'import.meta.env.VITE_CHAOS_E2E_BACKEND_PORT': JSON.stringify(process.env.CHAOS_E2E_BACKEND_PORT || ''),
    'import.meta.env.VITE_CHAOS_WS_PORT': JSON.stringify(process.env.CHAOS_WEB_PORT || ''),
  },
  server: {
    // 5173 已保留给 qxy-pem（外网域名映射），本项目固定使用 5174
    host: '0.0.0.0',
    port: Number(uiPort),
    proxy: {
      '/api': {
        target: webHost,
        changeOrigin: true,
        headers: {
          origin: `http://127.0.0.1:${uiPort}`,
        },
      },
      '/health': {
        target: webHost,
        changeOrigin: true,
      },
      '/ws': {
        target: webHost.replace(/^http/, 'ws'),
        ws: true,
        changeOrigin: true,
        headers: {
          origin: `http://127.0.0.1:${uiPort}`,
        },
      },
    },
  },
})
