import { useEffect, useRef, useState } from 'react'

/** The time, refreshed every `ms`: makes uptime and "ago" readouts move between polls. It is state, so it is
 *  stable between renders and safe as an effect or memo dependency. */
export function useTick(ms = 1000) {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), ms)
    return () => clearInterval(t)
  }, [ms])
  return now
}

/** `value`, once it has stopped changing for `ms`: a costly view (a laid-out graph) follows a scrub of the time
 *  machine when the needle rests, not at every step. */
export function useSettled<T>(value: T, ms = 180): T {
  const [settled, setSettled] = useState(value)
  useEffect(() => {
    const t = setTimeout(() => setSettled(value), ms)
    return () => clearTimeout(t)
  }, [value, ms])
  return settled
}

/** Samples a polled number every `every` ms and keeps the last `n` samples, for a tile's own sparkline. */
export function useHistory(value: number | undefined, n = 40, every = 2000) {
  const [h, setH] = useState<number[]>([])
  const latest = useRef(value)
  useEffect(() => { latest.current = value }, [value])
  const tick = useTick(every)
  useEffect(() => {
    const v = latest.current
    if (v !== undefined) setH((prev) => [...prev.slice(-(n - 1)), v])
  }, [tick, n])
  return h
}
