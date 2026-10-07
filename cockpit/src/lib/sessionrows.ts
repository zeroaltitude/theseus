// A session's own ledger rows, for its deck (theseus-kuzw). The deck read its own `ledger.tail`, which the daemon caps at
// 1,000 rows: a long session lost its oldest turns, and the deck numbered the turns it kept from one. Now the rows come
// from the page's one copy of the ledger (`useHistoryRows`), the whole session however long, and a tail read of the
// session's newest rows stands in only while that copy is read, or when an older daemon cannot page (it then holds the
// whole ledger's newest thousand, fewer of this session's than the tail read).
//
// Pure, with only the protocol's types, so `node --test` runs it (test/sessionrows.test.ts).
import type { LedgerEntry } from '@protocol'

/** What the deck needs of the shared history. */
export interface HistoryView { rows: readonly LedgerEntry[]; ready: boolean; partial: boolean }

/** Whether the shared history holds every row of the ledger: read to its end, by a daemon that pages. */
export const wholeHistory = (h: HistoryView): boolean => h.ready && !h.partial

/** The deck's rows, oldest first: the session's rows from the whole history, else the tail read of its newest rows. */
export function deckRows(history: HistoryView, id: string, tail: readonly LedgerEntry[] | undefined): LedgerEntry[] | undefined {
  if (!wholeHistory(history)) return tail ? [...tail] : undefined
  return history.rows.filter((r) => r.session_id === id)
}
