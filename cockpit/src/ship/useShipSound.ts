// The Ship's sound, on the page (theseus-hnof.2, the owner's C4): the toggle, kept in the browser and off by default,
// and, while it is on, the daemon's pushes heard through the cue table (`sound.ts`) and played (`audio.ts`). The cues
// play on every page (the owner's F9, theseus-7zph): the Shell mounts `useSoundCues` once, and the Ship's Sound button
// reads and sets the one toggle through `useShipSound`. The oar splashes only while the Ship is shown (`cueHere`).
// While sound is on, the sea is heard too, on every page (theseus-pl0x, `surf.ts`): its height from the work now, read
// from what the page already reads (health, and its one copy of the ledger), silent where the sea is still.
import { useEffect, useRef, useState } from 'react'
import { useLocation } from 'react-router'
import { create } from 'zustand'
import type { Health } from '@protocol'
import { useCalm } from '@/lib/calm'
import { onNewRows, useLedgerHistory } from '@/lib/history'
import { client, useRpc } from '@/lib/rpc'
import { ShipAudio } from './audio'
import { tpmOf } from './sea.ts'
import { cueHere, cueOf, cueOfRow, newEar, SOUND_KEY, soundOn, type Cue } from './sound'
import { surfHeight } from './surf.ts'

export interface ShipSound {
  on: boolean
  /** On, and the browser lets it play (after a reload it waits for the first click on the page). */
  playing: boolean
  toggle: () => void
}

/** The page's one audio context, made on the first cue or the first toggle. */
let audio: ShipAudio | null = null
const theAudio = () => (audio ??= new ShipAudio())

/** The one toggle, kept in the browser, and whether the browser lets it play. */
const useSound = create<{ on: boolean; playing: boolean }>(() => ({
  on: typeof localStorage !== 'undefined' && soundOn(localStorage.getItem(SOUND_KEY)),
  playing: false,
}))

/** The Sound button's state and its toggle. */
export function useShipSound(): ShipSound {
  const on = useSound((s) => s.on)
  const playing = useSound((s) => s.playing)
  const toggle = () => {
    const next = !useSound.getState().on
    localStorage.setItem(SOUND_KEY, next ? 'on' : 'off')
    useSound.setState({ on: next })
    if (next) {
      // This click is the gesture the browser asks for: start, and ring once, softly, so the operator hears it work.
      // The sea comes in under the bell (`useSurf`, at once: its effect runs as this click's render lands).
      const a = theAudio()
      a.start()
      setTimeout(() => { if (a.running) a.play('bell') }, 60)
    }
  }
  return { on, playing: on && playing, toggle }
}

/** The cues, heard on every page while sound is on: mounted once, in the Shell. `onShip`: the Ship is the page shown. */
export function useSoundCues(onShip: boolean) {
  const on = useSound((s) => s.on)
  useSurf(on)
  const here = useRef(onShip)
  useEffect(() => { here.current = onShip }, [onShip])
  useEffect(() => {
    if (!on) return
    const a = theAudio()
    const ear = newEar()
    const wake = () => {
      a.start()
      // The context resumes a moment after the gesture.
      setTimeout(() => useSound.setState({ playing: a.running }), 50)
    }
    wake()
    document.addEventListener('pointerdown', wake, true)
    document.addEventListener('keydown', wake, true)
    const sound = (heard: Cue | null) => {
      const cue = cueHere(heard, here.current)
      if (!cue) return
      a.play(cue)
      // Dev and bench builds: the cues played, for a recording's count.
      if (import.meta.env.DEV || import.meta.env.MODE === 'bench') ((window as unknown as { __shipCues?: string[] }).__shipCues ??= []).push(cue)
    }
    const off = client.onNotify((method, params) => sound(cueOf(ear, method, params, Date.now())))
    // The page's one copy of the ledger (it reads nothing more for this): a failure or a question in a session whose
    // pushes the page does not hear. Only rows from now on.
    const since = Date.now()
    const offRows = onNewRows((rows) => { for (const r of rows) sound(cueOfRow(ear, r, Date.now(), since)) })
    return () => {
      off()
      offRows()
      document.removeEventListener('pointerdown', wake, true)
      document.removeEventListener('keydown', wake, true)
    }
  }, [on])
}

/** How often the sea's minute of tokens is read again with no new row: the minute slides on, and the swell settles. */
const SURF_TICK_MS = 10_000

declare global {
  interface Window { __shipSurf?: () => { height: number; voice: { gain: number; wash: number; rate: number } } | null }
}

/** The ambient sea while sound is on: its height (`surfHeight`) from the turns running (health, the query the heartbeat
 *  bar reads every 2 s) and tokens a minute (the page's one copy of the ledger: nothing more is read for it), the
 *  present's on every page; silent in Calm (which reduced motion turns on) and under `?swell=0`, as the sea is still.
 *  The page sets a height only when one of those changes; the waves themselves are the audio thread's. */
function useSurf(on: boolean) {
  const calm = useCalm((s) => s.calm)
  const swell = new URLSearchParams(useLocation().search).get('swell') !== '0'
  const hearing = on && !calm && swell
  const { data: h } = useRpc<Health>('health', undefined, 2000, { enabled: hearing })
  const [tpm, setTpm] = useState(0)
  useEffect(() => {
    if (!hearing) return
    const read = () => setTpm(tpmOf(useLedgerHistory.getState().rows, Date.now()))
    read()
    const off = onNewRows(read)
    const tick = setInterval(read, SURF_TICK_MS)
    return () => { off(); clearInterval(tick) }
  }, [hearing])
  const height = surfHeight({ on, calm, swell, tpm, turns: h?.kernel.executions_by_state.running ?? 0 })
  useEffect(() => { theAudio().surf(height) }, [height])
  // Dev and bench builds: what the sea plays now, for a live check.
  useEffect(() => {
    if (import.meta.env.DEV || import.meta.env.MODE === 'bench') window.__shipSurf = () => theAudio().surfing
  }, [])
}
