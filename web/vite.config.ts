import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Build output is embedded into theseusd (crates/theseusd/web/dist).
// `npm run dev` proxies the WebSocket to a running daemon on the default port.
export default defineConfig({
  plugins: [react()],
  build: {
    outDir: '../crates/theseusd/web/dist',
    emptyOutDir: true,
    sourcemap: false,
  },
  server: {
    port: 5173,
    proxy: {
      '/ws': { target: 'ws://127.0.0.1:7433', ws: true },
    },
  },
})
