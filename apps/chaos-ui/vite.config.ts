import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

const webHost = 'http://127.0.0.1:8787'

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      '/api': {
        target: webHost,
        changeOrigin: true,
      },
      '/health': {
        target: webHost,
        changeOrigin: true,
      },
      '/ws': {
        target: webHost.replace(/^http/, 'ws'),
        ws: true,
      },
    },
  },
})
