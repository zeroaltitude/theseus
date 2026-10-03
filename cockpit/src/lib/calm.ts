// Calm is a mode (theseus-logs): no post-processing, no particles, no decorative motion anywhere in the cockpit.
// It starts on when the system asks for reduced motion, and the operator's choice is kept. `?calm=1` turns it on for
// one page (screenshots, a quiet wall display).
import { create } from 'zustand'

function initial(): boolean {
  if (typeof window === 'undefined') return false
  if (new URLSearchParams(window.location.search).get('calm') === '1') return true
  const kept = localStorage.getItem('cockpit.calm')
  if (kept) return kept === 'on'
  return window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false
}

const start = initial()
if (typeof document !== 'undefined') document.documentElement.classList.toggle('calm', start)

export const useCalm = create<{ calm: boolean; setCalm: (on: boolean) => void }>((set) => ({
  calm: start,
  setCalm: (on) => {
    localStorage.setItem('cockpit.calm', on ? 'on' : 'off')
    document.documentElement.classList.toggle('calm', on)
    set({ calm: on })
  },
}))
