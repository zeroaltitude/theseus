// The calendar read (theseus-jid7t.5): `calendar.window` over the spans the page shows (`readSpans`: the week, or a
// day either side of a Day zoom, and today and tomorrow for Coming up), read again when a span changes, on the push
// `calendar.changed`, and every five minutes while the document is visible. That is the one timer this design allows:
// it stops while the tab is hidden (a return reads at once) and on unmount. A page left alone makes no other request.
//
// The answer stays in memory: nothing is stored, logged or put in the address. A refusal (another connection, a shared
// place) or an error is the lens's banner, in the daemon's words, never an empty grid.
import { useEffect, useMemo, useRef, useState } from 'react'
import type { CalendarWindow, CalendarWindowResult } from '@protocol'
import { client, useConn } from '@/lib/rpc'
import { weekWindow } from '@/lib/agenda'
import { joinWindows } from './calendar'
import { latches, wordsOf } from './refusal'
import { readSpans, zoomed } from './timeline'

/** The one timer's period. */
export const EVERY_MS = 5 * 60_000
/** The push's bursts are one read. */
const DEBOUNCE_MS = 300

export interface CalendarRead { calendars: CalendarWindow[]; error: string | null; read: boolean }

const EMPTY: CalendarRead = { calendars: [], error: null, read: false }

/** `calendar.window` over `spans`, kept current by the push and the one timer. */
export function useCalendar(spans: ReadonlyArray<{ from_ms: number; to_ms: number }>): CalendarRead {
  const [state, setState] = useState<CalendarRead>(EMPTY)
  const open = useConn((s) => s.status === 'open')
  // A refusal latches here, outside the effect, so a span change does not read again; the push or a reopen clears it.
  const latched = useRef(false)
  useEffect(() => { if (!open) latched.current = false }, [open])
  const sig = spans.map((s) => `${s.from_ms}-${s.to_ms}`).join(',')
  useEffect(() => {
    if (!open || !sig) return
    let dead = false
    let soon: ReturnType<typeof setTimeout> | null = null
    let every: ReturnType<typeof setInterval> | null = null
    const want = sig.split(',').map((s) => { const [from_ms, to_ms] = s.split('-').map(Number); return { from_ms, to_ms } })
    const read = async () => {
      try {
        const parts = await Promise.all(want.map((w) => client.call<CalendarWindowResult>('calendar.window', w)))
        if (!dead) setState({ calendars: joinWindows(parts), error: null, read: true })
      } catch (e) {
        // A stale week is never shown as current: a failed read drops what the last one held.
        if (!dead) setState({ calendars: [], error: wordsOf(e), read: true })
        // A refusal is the connection's, not the moment's: keep the banner and read no more until the push or a reopen.
        if (latches(e)) { latched.current = true; if (every) { clearInterval(every); every = null } }
      }
    }
    const later = () => { if (soon) return; if (latched.current) { latched.current = false; arm() }; soon = setTimeout(() => { soon = null; void read() }, DEBOUNCE_MS) }
    const arm = () => {
      if (every) { clearInterval(every); every = null }
      if (latched.current) return
      if (document.visibilityState === 'visible') every = setInterval(() => void read(), EVERY_MS)
    }
    const seen = () => { arm(); if (!latched.current && document.visibilityState === 'visible') later() }
    if (!latched.current) void read()
    arm()
    document.addEventListener('visibilitychange', seen)
    const off = client.onNotify((method) => { if (method === 'calendar.changed' || method === 'events.lost') later() })
    return () => {
      dead = true
      off()
      document.removeEventListener('visibilitychange', seen)
      if (soon) clearTimeout(soon)
      if (every) clearInterval(every)
    }
  }, [sig, open])
  return state
}

/** The Operations page's calendar: in the Timeline lens the window shown (`?w=`, `?day=`) and today and tomorrow;
 *  in any other lens only today and tomorrow, for Coming up, and only while `coming` says it is on screen. */
export function useOpsCalendar(timeline: boolean, weeks: number, day: number | null, now: number, coming = true): CalendarRead {
  const hour = Math.floor(now / 3_600_000)
  const spans = useMemo(() => {
    const w = zoomed(weekWindow(now, weeks), day)
    return readSpans(timeline ? w : null, now, timeline && day != null, coming)
  }, [timeline, weeks, day, hour, coming]) // eslint-disable-line react-hooks/exhaustive-deps
  return useCalendar(spans)
}
