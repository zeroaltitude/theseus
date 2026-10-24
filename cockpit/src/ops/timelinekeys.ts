// The Timeline lens's keys (theseus-jid7t.5): what each of this row's verbs in `KEYS` does, for the page's one handler.
// `d`/`w` zoom, `⇧R` redacts, `,`/`.` move a week (a day in Day zoom), `j`/`k` step the marks and events by time, and
// on a focused mark the TUI's Horizon keys: `z` snooze, `p` pause or resume, `r` run now, `x` cancel, `⏎` open. Every act
// goes through `useOpsAct`, so the time machine turns it off.
import { useSearchParams } from 'react-router'
import type { Mark } from '@/lib/agenda'
import type { Address } from './address'
import type { TimelineReport } from './TimelineLens'
import { horizonVerb, stepOrder } from './timeline'
import { useOpsAct } from './act'
import type { Verb } from './keys'

export type TimelineVerb = Extract<Verb, 'zoom_day' | 'zoom_week' | 'redact' | 'week_prev' | 'week_next' | 'mark_next' | 'mark_prev' | 'mark_snooze' | 'mark_pause' | 'mark_run' | 'mark_cancel' | 'mark_open'>

/** `?day=`: the Day zoom's day of the week shown, 0 to 6; absent is the week. A key the address does not own, so
 *  every link keeps it. */
export function parseDay(raw: string | null): number | null {
  const n = raw == null ? NaN : Number(raw)
  return Number.isInteger(n) && n >= 0 && n <= 6 ? n : null
}

export function useTimelineKeys({ a, day, report, mark, set, pick, flash, open }: {
  a: Address; day: number | null; report: TimelineReport | null; mark: Mark | undefined
  set: (patch: Partial<Address>) => void; pick: (sel: string | null) => void; flash: (words: string) => void; open: (session: string) => void
}) {
  const [, setParams] = useSearchParams()
  const { act } = useOpsAct()
  const setDay = (d: number | null) => setParams((p) => { const n = new URLSearchParams(p); if (d == null) n.delete('day'); else n.set('day', String(d)); return n }, { replace: true })
  /** A week back or on; in Day zoom, a day, across the week's edge. */
  const shift = (delta: number) => {
    if (day == null) { set({ w: Math.max(-52, Math.min(52, a.w + delta)) }); return }
    const d = day + delta
    if (d >= 0 && d <= 6) { setDay(d); return }
    const w = Math.max(-52, Math.min(52, a.w + delta))
    if (w === a.w) return
    setParams((p) => { const n = new URLSearchParams(p); n.set('day', String(d < 0 ? 6 : 0)); if (w) n.set('w', String(w)); else n.delete('w'); return n }, { replace: true })
  }
  const zoom = (d: number | null) => setDay(d == null ? null : d >= 0 ? d : Math.max(0, report?.dayOf(a.sel) ?? -1))
  const horizon = (k: 'z' | 'p' | 'r' | 'x', verb: TimelineVerb) => {
    if (!mark) return
    const v = horizonVerb(mark, k)
    if (!v) { flash(`a ${mark.kind === 'check' ? 'check-in' : mark.kind} takes no ${k}`); return }
    void act(verb, `${mark.key}:${v.key}`, v.method, v.params, v.ask)
  }
  const run = (verb: TimelineVerb) => {
    switch (verb) {
      case 'zoom_day': zoom(day ?? -1); return
      case 'zoom_week': setDay(null); return
      case 'redact': set({ redact: !a.redact }); return
      case 'week_prev': shift(-1); return
      case 'week_next': shift(1); return
      case 'mark_next': pick(stepOrder(report?.order ?? [], a.sel, 1)); return
      case 'mark_prev': pick(stepOrder(report?.order ?? [], a.sel, -1)); return
      case 'mark_snooze': horizon('z', verb); return
      case 'mark_pause': horizon('p', verb); return
      case 'mark_run': horizon('r', verb); return
      case 'mark_cancel': horizon('x', verb); return
      case 'mark_open': { const s = mark?.run_session ?? mark?.session_id; if (s) open(s); else flash('this mark has no session to open'); return }
    }
  }
  // Today in one change of the address: the week, and in Day zoom today's day (the first of the week shown).
  const today = () => setParams((p) => { const n = new URLSearchParams(p); n.delete('w'); if (day != null) n.set('day', '0'); return n }, { replace: true })
  return { run, shift, zoom, today }
}
