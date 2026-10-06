import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { fileURLToPath } from 'node:url'

// The cockpit (theseus-45n5): the web UI, served by theseusd at / (theseus-vm3n.6; its old /cockpit/ redirects)
// and embedded from crates/theseusd/cockpit/dist. The protocol client is src/protocol.ts, whose types
// (src/protocol.gen) a theseus-protocol test writes from the Rust ones.
//
// `npm run dev` serves the page on 127.0.0.1:5174, and the page connects STRAIGHT to the daemon's /ws
// (THESEUS_DEV_DAEMON; default a scratch daemon on 127.0.0.1:7434, never the owner's 7433). Set that daemon's
// `[web] dev_origin = "http://127.0.0.1:5174"` while developing (off by default; each use is ledgered). There is
// no /ws proxy: a proxy, whether or not it rewrites Origin, relays other pages and other users' processes to the
// daemon from the operator's own socket (theseus-zab, theseus-88im). Never add one.
const devDaemon = process.env.THESEUS_DEV_DAEMON ?? '127.0.0.1:7434'

export default defineConfig({
  base: '/',
  plugins: [react(), tailwindcss()],
  define: {
    'import.meta.env.VITE_THESEUS_DEV_DAEMON': JSON.stringify(devDaemon),
  },
  resolve: {
    alias: {
      '@protocol': fileURLToPath(new URL('./src/protocol.ts', import.meta.url)),
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
    host: '127.0.0.1',
    port: 5174,
    strictPort: true,
  },
})
