import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Build output is embedded into theseusd (crates/theseusd/web/dist).
// `npm run dev` serves the page on 127.0.0.1:5173, and the page connects STRAIGHT to the daemon's /ws
// (THESEUS_DEV_DAEMON, default the daemon on 127.0.0.1:7433). The daemon serves /ws only to its own page, so while
// you work set `[web] dev_origin = "http://127.0.0.1:5173"` in its config, and unset it when you are done
// (theseus-zab). There is no /ws proxy. A proxy, whether or not it rewrites Origin, relays other pages and other
// users' processes to the daemon from your own socket, where the daemon's owner check can't see them
// (theseus-88im). Never add one.
const devDaemon = process.env.THESEUS_DEV_DAEMON ?? '127.0.0.1:7433'

export default defineConfig({
  plugins: [react()],
  define: {
    'import.meta.env.VITE_THESEUS_DEV_DAEMON': JSON.stringify(devDaemon),
  },
  build: {
    outDir: '../crates/theseusd/web/dist',
    emptyOutDir: true,
    sourcemap: false,
  },
  server: {
    host: '127.0.0.1',
    port: 5173,
    strictPort: true,
  },
})
