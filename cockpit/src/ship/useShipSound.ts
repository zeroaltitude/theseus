// The Ship's sound, on the page (theseus-hnof.2, the owner's C4): the toggle, kept in the browser and off by default,
// and, while it is on, the daemon's pushes heard through the cue table (`sound.ts`) and played (`audio.ts`).
import { useEffect, useRef, useState } from 'react'
import { client } from '@/lib/rpc'
import { ShipAudio } from './audio'
import { cueOf, newEar, SOUND_KEY, soundOn } from './sound'

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
    const off = client.onNotify((method, params) => {
      const cue = cueOf(ear, method, params, Date.now())
      if (cue) a.play(cue)
    })
    return () => {
      off()
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
