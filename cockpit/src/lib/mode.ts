// Night or daylight (theseus-hnof.5): the cockpit's two modes. Night is the brass-and-neon bridge on navy glass, as it
// has always been; daylight is the same bridge by day, ivory chart paper and brass, its inks and tones deepened to read
// on paper. The operator chooses night, daylight, or the system's (theseus-001m: daylight while the system is light,
// following it as it changes); night is the default. The choice is kept in this browser; `?mode=light`, `?mode=dark`
// or `?mode=system` sets it for one page (screenshots, a wall display). The Ship's sea and the ship's log's track are
// screens and stay at night either way.
//
// The mode is a class on <html> (`light`), which re-points the theme's tokens (index.css); the tone hexes the views
// set inline swap to their daylight steps (`toneHex`), and the charts' colours are mapped as they are drawn
// (`daylight()` in Echart.tsx). A change of mode draws the views again from the start (main.tsx keys them on it).
import { create } from 'zustand'
import { toneHex } from './taxonomy'
import { choiceOf, MARKS, modeOf, TONES, type Mode, type ModeChoice } from './daylight'
import { TONE_MARK } from './viz'

export type { Mode, ModeChoice }

const KEY = 'cockpit.mode'
const SYSTEM_LIGHT = '(prefers-color-scheme: light)'
const systemLight = (): boolean => typeof window !== 'undefined' && !!window.matchMedia?.(SYSTEM_LIGHT).matches

function initial(): ModeChoice {
  if (typeof window === 'undefined') return 'dark'
  const asked = new URLSearchParams(window.location.search).get('mode')
  let kept: string | null = null
  try { kept = localStorage.getItem(KEY) } catch { /* night */ }
  return choiceOf(asked, kept)
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

const choice = initial()
const start = modeOf(choice, systemLight())
apply(start)

interface ModeState {
  /** The mode drawn now. */
  mode: Mode
  /** The operator's choice: night, daylight, or the system's (theseus-001m). */
  choice: ModeChoice
  setChoice: (c: ModeChoice) => void
  /** Night or daylight, chosen outright. */
  setMode: (m: Mode) => void
}

export const useMode = create<ModeState>((set, get) => ({
  mode: start,
  choice,
  setChoice: (c) => {
    try { localStorage.setItem(KEY, c) } catch { /* the page's choice stands */ }
    const mode = modeOf(c, systemLight())
    apply(mode)
    set({ choice: c, mode })
  },
  setMode: (m) => get().setChoice(m),
}))

// Following the system: when it turns light or dark, so does the cockpit (a draw again from the start, as any change).
if (typeof window !== 'undefined' && window.matchMedia) {
  window.matchMedia(SYSTEM_LIGHT).addEventListener('change', (e) => {
    const st = useMode.getState()
    const mode = modeOf(st.choice, e.matches)
    if (st.choice !== 'system' || mode === st.mode) return
    apply(mode)
    useMode.setState({ mode })
  })
}

/** The mode now, outside React (a tooltip built as DOM, a chart's option). */
export const currentMode = (): Mode => useMode.getState().mode
