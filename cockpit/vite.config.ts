import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { fileURLToPath } from 'node:url'

// The cockpit (theseus-45n5): the new experience, served by theseusd at /cockpit/ and embedded from
// crates/theseusd/cockpit/dist. It shares the protocol client with the Observatory (web/src/protocol.ts).
//
// `npm run dev` proxies the WebSocket to a SCRATCH daemon (default 127.0.0.1:7434, never Eddie's 7433) and
// rewrites Host and Origin to the daemon's own, which the web UI's H1 checks require. That rewrite makes the
// dev server a relay (theseus-zab), so point it only at a scratch daemon.
const devDaemon = process.env.THESEUS_DEV_DAEMON ?? '127.0.0.1:7434'

export default defineConfig({
  base: '/cockpit/',
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@protocol': fileURLToPath(new URL('../web/src/protocol.ts', import.meta.url)),
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  build: {
    outDir: '../crates/theseusd/cockpit/dist',
    emptyOutDir: true,
    sourcemap: false,
    chunkSizeWarningLimit: 1500,
  },
  server: {
    port: 5174,
    strictPort: true,
    proxy: {
      '/ws': {
        target: `http://${devDaemon}`,
        ws: true,
        changeOrigin: true,
        configure: (proxy) => {
          proxy.on('proxyReqWs', (req) => req.setHeader('origin', `http://${devDaemon}`))
        },
      },
    },
  },
})
