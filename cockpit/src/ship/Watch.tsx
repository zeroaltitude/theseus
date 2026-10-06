// The watch (theseus-hnof): the Ship's insight instruments. Five brass plates, each answering one of the owner's
// questions at a glance, from the daemon's own data: what is working now, what waits for you, what is slow, what today
// cost, and what went wrong. Each plate's "show" lights its vessels and calls on the chart and dims the rest.
//
// STUB: the interface the Ship renders. The plates themselves are built in `watch.ts` (pure) and here.
import type { ShipData } from './useShipData'
import type { ShipModel } from './model'

/** What a plate lights on the chart: the vessels (session ids) and the lights (node ids) it is about. */
export interface WatchFocus {
  key: WatchKey
  vessels: string[]
  lights: string[]
}

export type WatchKey = 'working' | 'waiting' | 'slow' | 'spent' | 'wrong'

/** Where an item of a plate points: a session, or a node (a call, a message) in it. */
export interface WatchTarget { session?: string; node?: string }

export interface WatchProps {
  model: ShipModel | null
  data: ShipData
  /** The overlay that is on, if any. */
  focus: WatchFocus | null
  onFocus: (f: WatchFocus | null) => void
  /** Fly the camera to an item. */
  onFly: (t: WatchTarget) => void
}

export function Watch(_props: WatchProps) {
  return null
}
