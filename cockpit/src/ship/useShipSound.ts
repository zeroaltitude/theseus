// The Ship's sound, on the page (theseus-hnof.2, the owner's C4): the toggle, kept in the browser and off by default,
// and, while it is on, the daemon's pushes heard through the cue table (`sound.ts`) and played (`audio.ts`).
import { useEffect, useRef, useState } from 'react'
import { onNewRows } from '@/lib/history'
import { client } from '@/lib/rpc'
import { ShipAudio } from './audio'
import { cueOf, cueOfRow, newEar, SOUND_KEY, soundOn, type Cue } from './sound'

export interface ShipSound {
  on: boolean
  /** On, and the browser lets it play (after a reload it waits for the first click on the page). */
  playing: boolean
  toggle: () => void
}

export function useShipSound(): ShipSound {
  const [on, setOn] = useState(() => soundOn(localStorage.getItem(SOUND_KEY)))
  const [playing, setPlaying] = useState(false)
  const audio = useRef<ShipAudio | null>(null)
  useEffect(() => {
    if (!on) return
    const a = (audio.current ??= new ShipAudio())
    const ear = newEar()
    const wake = () => {
      a.start()
      // The context resumes a moment after the gesture.
      setTimeout(() => setPlaying(a.running), 50)
    }
    wake()
    document.addEventListener('pointerdown', wake, true)
    document.addEventListener('keydown', wake, true)
    const sound = (cue: Cue | null) => {
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
  useEffect(() => () => audio.current?.dispose(), [])
  const toggle = () => {
    const next = !on
    localStorage.setItem(SOUND_KEY, next ? 'on' : 'off')
    setOn(next)
    if (next) {
      // This click is the gesture the browser asks for: start, and ring once, softly, so the operator hears it work.
      const a = (audio.current ??= new ShipAudio())
      a.start()
      setTimeout(() => { if (a.running) a.play('bell') }, 60)
    }
  }
  return { on, playing: on && playing, toggle }
}
