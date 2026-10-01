import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Build output is embedded into theseusd (crates/theseusd/web/dist).
// `npm run dev` proxies the WebSocket to a running daemon on the default port.
// The daemon serves /ws only to its own page, so while you work set
// `[web] dev_origin = "http://localhost:5173"` in its config (the dev page's
// origin exactly as the browser shows it), and unset it when you are done
// (theseus-zab). Never add changeOrigin or rewriteWsOrigin to this proxy: it
// must pass each page's own Host and Origin through, so the daemon can still
// refuse every other page that reaches it through the dev server.
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
