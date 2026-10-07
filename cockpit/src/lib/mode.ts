// Night or daylight (theseus-hnof.5): the cockpit's two modes. Night is the brass-and-neon bridge on navy glass, as it
// has always been; daylight is the same bridge by day, ivory chart paper and brass, its inks and tones deepened to read
// on paper. The operator's choice is kept in this browser; `?mode=light` or `?mode=dark` sets it for one page
// (screenshots, a wall display). The Ship's sea and the ship's log's track are screens and stay at night either way.
//
// The mode is a class on <html> (`light`), which re-points the theme's tokens (index.css); the tone hexes the views
// set inline swap to their daylight steps (`toneHex`), and the charts' colours are mapped as they are drawn
// (`daylight()` in Echart.tsx). A change of mode draws the views again from the start (main.tsx keys them on it).
import { create } from 'zustand'
import { toneHex } from './taxonomy'
import { MARKS, TONES, type Mode } from './daylight'
import { TONE_MARK } from './viz'

export type { Mode }

function initial(): Mode {
  if (typeof window === 'undefined') return 'dark'
  const asked = new URLSearchParams(window.location.search).get('mode')
  if (asked === 'light' || asked === 'dark') return asked
  try { return localStorage.getItem('cockpit.mode') === 'light' ? 'light' : 'dark' } catch { return 'dark' }
}

function apply(mode: Mode) {
  Object.assign(toneHex, TONES[mode])
  // The marks the charts set inline (the fleet's bar, the Ledger's tiles, the flame's spans) follow the mode too.
  Object.assign(TONE_MARK, MARKS[mode])
  if (typeof document === 'undefined') return
  const html = document.documentElement
  html.classList.toggle('light', mode === 'light')
  html.classList.toggle('dark', mode === 'dark')
  html.style.colorScheme = mode
}

const start = initial()
apply(start)

export const useMode = create<{ mode: Mode; setMode: (m: Mode) => void }>((set) => ({
  mode: start,
  setMode: (mode) => {
    try { localStorage.setItem('cockpit.mode', mode) } catch { /* the page's choice stands */ }
    apply(mode)
    set({ mode })
  },
}))

/** The mode now, outside React (a tooltip built as DOM, a chart's option). */
export const currentMode = (): Mode => useMode.getState().mode
