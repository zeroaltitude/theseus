// The Ops page's Tasks tab (theseus-nuqut): the needs-me reasons, then the counts and the selection.
import test from 'node:test'
import assert from 'node:assert/strict'
import type { ThreadAsk } from '../src/protocol.gen/index.ts'
import { boardRows } from '../src/ops/board.ts'
import { needsEntries, reasonOf } from '../src/ops/needsReason.ts'
import { queueOf } from '../src/ops/queue.ts'
import { board, NOW, MIN } from './ops-fixtures.ts'

const routerAsk = (over: Partial<ThreadAsk> = {}): ThreadAsk => ({
  ask_id: 'ask_1', session_id: 'ses_tides', question: 'Which thread is this for?', options: [{ key: '1', label: 'tide tables' }, { key: 'home', label: 'here' }],
  message: 'why is the build red', expires_ms: NOW + 20 * MIN, ...over,
} as ThreadAsk)

function world() {
  const b = board()
  // A late-opened tab: the router's ask was read by `thread.ask_list`, not heard.
  const asks = [{ ask: routerAsk(), heard_ms: NOW - 5 * MIN }]
  const queue = queueOf({ confirms: b.confirms, views: b.views, groups: b.groups, asks, titleOf: (s) => s })
  const rows = boardRows({ views: b.views, tasks: b.tasks, sessions: b.sessions, queue, now: NOW })
  return { b, queue, rows }
}

test('every row in the needs-me column has a task and a non-empty reason', () => {
  const { rows, queue } = world()
  const entries = needsEntries(rows, queue)
  assert.ok(entries.length >= 3, `the column is not empty: ${JSON.stringify(entries)}`)
  for (const e of entries) {
    assert.ok(e.task.length > 0, 'it names its task')
    assert.ok(e.reason.trim().length > 0, `${e.task} has a reason`)
  }
  const by = new Map(entries.map((e) => [e.task, e.reason]))
  assert.match(by.get('ses_t5') ?? '', /chart server refused/, 'a failure says why it failed')
  assert.match(by.get('ses_t1') ?? '', /Which pier first|write under the projects root/, 'a task’s question or approval')
})

test('a router ask read after it was posted is "waiting on you" against the right task', () => {
  const { rows, queue } = world()
  const e = needsEntries(rows, queue).find((x) => x.task === 'ses_tides')
  assert.ok(e, 'the conversation the ask came from is in the column')
  assert.match(e.reason, /Which thread is this for\?/)
  assert.equal(reasonOf(queue.find((i) => i.kind === 'ask')!), 'Which thread is this for?')
})

// ------------------------------------------------------------------ the counts and the selection

import type { NarrativeLine } from '../src/protocol.gen/index.ts'
import { allCounters, columnsOf } from '../src/ops/board.ts'
import { narrativeOf, tabCounts, tabRows, TAB_COUNTS } from '../src/ops/taskTab.ts'

const line = (seq: number, session: string | undefined, text: string): NarrativeLine => ({ seq, at_unix_ms: NOW - seq, part: 'turn', session_id: session, text } as NarrativeLine)

test('five counts match the Board’s columns, and a count selects exactly that column’s rows', () => {
  const { rows } = world()
  const cols = columnsOf(rows, NOW)
  const counts = tabCounts(rows, NOW)
  assert.deepEqual(TAB_COUNTS, ['running', 'needs', 'paused', 'scheduled', 'done'])
  const mine = { running: cols.working, needs: cols.needs, paused: cols.paused, scheduled: cols.scheduled, done: cols.done }
  for (const c of TAB_COUNTS) {
    const want = mine[c].filter((r) => r.counted).map((r) => r.id)
    assert.deepEqual(tabRows(rows, NOW, c).map((r) => r.id), want, c)
    assert.equal(counts[c], want.length, c)
  }
  // The positive counts, so an empty board cannot pass: the fixture has work in every column.
  assert.ok(TAB_COUNTS.every((c) => counts[c] > 0), JSON.stringify(counts))
  // The same numbers as the Board’s counter bar.
  const bar = allCounters(rows, NOW, 0)
  assert.deepEqual(counts, { running: bar.working, needs: bar.needs, paused: bar.paused, scheduled: bar.still, done: bar.done })
  // Scheduled is its own count, not folded into paused or blocked.
  assert.ok(tabRows(rows, NOW, 'scheduled').every((r) => !tabRows(rows, NOW, 'paused').includes(r)))
})

test('selecting a row yields the narrative lines whose session is that row’s', () => {
  const { rows } = world()
  const lines = [line(1, 'ses_t2', 'a'), line(2, 'ses_t1', 'b'), line(3, undefined, 'daemon'), line(4, 'ses_t2', 'c'), line(5, 'ses_tides', 'd')]
  const t2 = rows.find((r) => r.id === 'ses_t2')!
  assert.deepEqual(narrativeOf(lines, t2).map((l) => l.text), ['a', 'c'])
  assert.deepEqual(narrativeOf(lines, rows.find((r) => r.id === 'ses_t1')!).map((l) => l.text), ['b'])
  assert.equal(narrativeOf(lines, t2, 1).length, 1, 'bounded to the last n')
  assert.deepEqual(narrativeOf([], t2), [])
})
