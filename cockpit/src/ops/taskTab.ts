// The Ops page's Tasks tab (theseus-nuqut): five counts, one to one with the Board's columns (the owner,
// 2026-11-12: "5 seems good": running, needs me, paused or blocked, scheduled, done today), the rows behind a count,
// and a row's live narrative. Pure: `npm test` runs it (test/ops-task-tab.test.ts). It reads the Board's one work set
// (`boardRows`) and its column membership (`columnsOf`), and decides nothing of its own.
import type { NarrativeLine } from '@protocol'
import { columnsOf, type BoardColumns, type OpsRow } from './board.ts'

export type TabCount = 'running' | 'needs' | 'paused' | 'scheduled' | 'done'
export const TAB_COUNTS: readonly TabCount[] = ['running', 'needs', 'paused', 'scheduled', 'done']
export const TAB_WORDS: Readonly<Record<TabCount, string>> = {
  running: 'running', needs: 'needs me', paused: 'paused or blocked', scheduled: 'scheduled', done: 'done today',
}
/** The Board column each count is. Done is the column's "today" half; older rows are paged under it, not counted. */
const COLUMN: Readonly<Record<TabCount, Exclude<keyof BoardColumns, 'older'>>> = { running: 'working', needs: 'needs', paused: 'paused', scheduled: 'scheduled', done: 'done' }

/** The rows a count stands for: its column's counted rows, in the Board's order. */
export function tabRows(rows: readonly OpsRow[], now: number, c: TabCount): OpsRow[] {
  return columnsOf(rows, now)[COLUMN[c]].filter((r) => r.counted)
}

export function tabCounts(rows: readonly OpsRow[], now: number): Record<TabCount, number> {
  const cols = columnsOf(rows, now)
  const n = (c: TabCount) => cols[COLUMN[c]].filter((r) => r.counted).length
  return { running: n('running'), needs: n('needs'), paused: n('paused'), scheduled: n('scheduled'), done: n('done') }
}

/** The session a row's lines are told under: a task's or conversation's own, a job's or wake's owner. */
export const sessionOfRow = (r: OpsRow): string | undefined => r.sessionId ?? (r.id.startsWith('ses_') ? r.id : undefined)

/** A row's narrative: the lines of its session, oldest first. A router ask has the place's session, and so its lines. */
export function narrativeOf(lines: readonly NarrativeLine[], r: OpsRow, n = 200): NarrativeLine[] {
  const s = sessionOfRow(r)
  return s ? lines.filter((l) => l.session_id === s).slice(-n) : []
}
