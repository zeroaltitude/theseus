// The whole ledger, in the page (theseus-logs, round two). The time machine folds it, the money river prices it, the
// speed wall reads its traces and starts, and the boundaries board its holds and grants: one copy, read once and then
// followed.
//
// - The walk: `ledger.tail` with `after` (theseus-xo0m), a thousand rows a page, from the first row to the last.
// - Following: every few seconds, only the rows after the last one read.
// - A daemon from before `after` ignores it and answers with its newest rows and no `next`. Then the history keeps
//   those, follows by the tail, and says it is partial: it reaches back only as far as they do.
import { useEffect } from 'react'
import { create } from 'zustand'
import type { LedgerEntry, LedgerTailResult } from '@protocol'
import { call, client, useConn } from './rpc'

export interface LedgerHistory {
  /** Every row read, oldest first (by WAL position). */
  rows: LedgerEntry[]
  /** The last position read: the next read's `after`. */
  last: number
  /** The walk has reached the ledger's end at least once. */
  ready: boolean
  /** The daemon cannot page: `rows` begin at its newest thousand, not at the first row. */
  partial: boolean
  /** The ledger's row count, as the last answer said. */
  total: number
  error?: string
}

export const useLedgerHistory = create<LedgerHistory>(() => ({ rows: [], last: 0, ready: false, partial: false, total: 0 }))

const PAGE = 1000
const FOLLOW_MS = 2500

let users = 0
let busy = false
let timer: ReturnType<typeof setTimeout> | null = null

/** Rows after `last`, appended once each. */
function append(st: LedgerHistory, rows: LedgerEntry[]): Pick<LedgerHistory, 'rows' | 'last'> | null {
  const fresh = rows.filter((r) => r.position > st.last)
  if (!fresh.length) return null
  return { rows: st.rows.concat(fresh), last: fresh[fresh.length - 1].position }
}

/** Read until the end: pages with `after`, or (an older daemon) its newest rows. */
async function readOn(): Promise<void> {
  if (busy) return
  busy = true
  try {
    for (let pages = 0; pages < 10_000; pages++) {
      const st = useLedgerHistory.getState()
      if (st.partial) {
        const r = await call<LedgerTailResult>('ledger.tail', { n: PAGE })
        const next = append(st, r.rows)
        if (next || r.total !== st.total || st.error !== undefined) useLedgerHistory.setState({ ...(next ?? {}), total: r.total, ready: true, error: undefined })
        return
      }
      const r = await call<LedgerTailResult>('ledger.tail', { n: PAGE, after: st.last })
      const next = append(st, r.rows)
      // No cursor, a full page, and more rows in the ledger than it holds: a daemon that ignored `after`.
      const ignored = r.next === undefined && r.rows.length > 0 && r.rows[0].position <= st.last
      const short = st.last === 0 && r.next === undefined && r.total > r.rows.length
      if (ignored || short) {
        useLedgerHistory.setState({ ...(next ?? {}), partial: true, total: r.total, ready: true, error: undefined })
        return
      }
      // Nothing new and nothing changed: no new state, so no view draws again for a quiet poll.
      if (next || r.total !== st.total || st.error !== undefined || (r.next === undefined && !st.ready)) {
        useLedgerHistory.setState({ ...(next ?? {}), total: r.total, error: undefined, ...(r.next === undefined ? { ready: true } : {}) })
      }
      if (r.next === undefined) return
    }
  } catch (e) {
    useLedgerHistory.setState({ error: String((e as { message?: string })?.message ?? e) })
  } finally {
    busy = false
  }
}

/** One follower for the page: a read in flight schedules the next itself, so two views mounting at once never start
 *  two loops. */
function follow() {
  if (timer) { clearTimeout(timer); timer = null }
  if (users <= 0 || busy) return
  void readOn().finally(() => {
    if (users > 0 && !timer) timer = setTimeout(() => { timer = null; follow() }, FOLLOW_MS)
  })
}

// A reconnect may follow a restart, whose rows came while the link was down: read on from the last position. The
// first connection is not one: the views' own first reads come first, and `useHistoryRows` starts the walk.
client.onOpen(() => { if (users > 0 && useLedgerHistory.getState().rows.length) follow() })

/** How long the ship's log lets a page land before its first walk: the landing view's own reads and first frame (the
 *  Ship's) come first. */
const UNHURRIED_MS = 1500

/** The whole ledger, kept fresh while mounted. Every caller shares one copy and one follower. A view that shows it at
 *  once reads it at once; `unhurried` (the ship's log, on every page) starts the first walk only after the page has
 *  landed, unless a view needs it sooner. */
export function useHistoryRows(unhurried = false): LedgerHistory {
  const open = useConn((s) => s.status === 'open')
  useEffect(() => {
    users++
    let later: ReturnType<typeof setTimeout> | undefined
    if (open) {
      if (unhurried && !useLedgerHistory.getState().rows.length) later = setTimeout(follow, UNHURRIED_MS)
      else follow()
    }
    return () => {
      users--
      if (later) clearTimeout(later)
      if (users <= 0 && timer) { clearTimeout(timer); timer = null }
    }
  }, [open, unhurried])
  return useLedgerHistory()
}

/** Rows of some kinds, from the shared history, in order. */
export function ofKinds(rows: LedgerEntry[], kinds: ReadonlySet<string>): LedgerEntry[] {
  return rows.filter((r) => kinds.has(r.kind))
}
