// The simple Ops page and the Ship's Operations page read one store (GitHub issue 17, "Ops, first cut"): every number on
// the simple page is the number on the Ship's page, and an ask can be answered and a running turn steered from it.
// Both pages are React, which this runner does not render, so the proof is two halves: the pipeline each page runs
// over the SAME fixtures, compared number by number (the Ship's, written here exactly as `views/Ops.tsx` runs it), and
// the pages' sources, read as text, to hold that each page still runs what the test runs and the simple page draws the
// Ship's own pieces to answer and to steer.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { allCounters, boardRows, columnsOf, spendToday, unaskedCards } from '../src/ops/board.ts'
import { answerOf, queueOf } from '../src/ops/queue.ts'
import { composerStep } from '../src/ops/composer.ts'
import { workSetOf, type WorkSetInput } from '../src/simple/ops/workset.ts'
import { store } from './simple-ops-fixtures.ts'

const src = (f: string) => readFileSync(new URL(`../src/${f}`, import.meta.url), 'utf8')

/** The Ship's Operations page, step by step, as `views/Ops.tsx` runs it on the present: its queue, its rows, its
 *  columns, its counters, its spend. */
function ship(i: WorkSetInput) {
  const views = [...i.views]
  const titleOf = (sid: string) => { const s = i.sessions.find((x) => x.session_id === sid); return s?.title || s?.label || sid.slice(-6) }
  const queue = queueOf({ confirms: [...i.confirms], views, records: [...i.records], groups: [...i.groups], asks: [...i.asks], executions: undefined, past: false, titleOf })
  const rows = boardRows({ views, tasks: [...i.tasks], sessions: [...i.sessions], records: [...i.records], lines: {}, queue, now: i.now, past: false })
  const spend = spendToday(i.history, i.now)
  return { queue, rows, cols: columnsOf(rows, i.now), counts: allCounters(rows, i.now, spend.usd), waiting: unaskedCards(rows), spent: spend.usd }
}

test('the simple page’s counts are the Ship’s, number for number, on the same store', () => {
  const i = store()
  const s = ship(i)
  const w = workSetOf(i)
  assert.deepEqual(w.counts, s.counts, 'needs you, working, paused·queued, still today, done today, spent today')
  assert.equal(w.spendUsd, s.spent)
  assert.deepEqual(w.queue.map((q) => q.key), s.queue.map((q) => q.key), 'the same asks, in the same order')
  const look = (rs: typeof s.rows) => rs.map((r) => [r.id, r.bucket, r.column, r.counted, r.asks])
  assert.deepEqual(look(w.rows), look(s.rows), 'the same work set: every card, its bucket, its column, whether it counts')
  // The fixture is a real board: every number above is not zero by accident.
  assert.ok(s.counts.needs > 0 && s.counts.working > 0 && s.counts.paused > 0 && s.counts.still > 0 && s.counts.done > 0 && s.counts.spent > 0, JSON.stringify(s.counts))
})

test('each list on the simple page holds what its number says, and its number is the Ship’s counter', () => {
  const i = store()
  const s = ship(i)
  const w = workSetOf(i)
  assert.equal(w.flight.length, s.counts.working, 'in flight: the counted working cards')
  assert.equal(w.cols.scheduled.length, s.counts.still, 'on the clock: the Ship’s still-today counter')
  assert.equal(w.cols.paused.length, s.counts.paused, 'paused: the Ship’s paused·queued counter')
  assert.equal(w.cols.done.length, s.counts.done, 'done today')
  // Needs you: the cards that wait, once each: the asks' cards, then the cards no ask names.
  const cards = new Set([...w.cols.needs.filter((r) => r.counted).map((r) => r.id)])
  assert.equal(cards.size, s.counts.needs, 'needs you: pieces of work that wait on the owner')
  assert.equal(w.waiting.length, s.waiting.length)
  // Planned: the steps no one has begun, what is on the clock and what is held, each from the Ship's rows.
  assert.equal(w.planned, s.rows.filter((r) => r.bucket === 'todo').length + s.counts.still + s.counts.paused)
  assert.ok(w.next.length > 0 && w.next.every((r) => r.bucket === 'todo'), 'steps not begun')
})

test('a job is working on the Ship’s Board and is inside its card on the simple page, never a second count', () => {
  const i = store()
  const s = ship(i)
  const w = workSetOf(i)
  const jobs = s.cols.working.filter((r) => r.kind === 'job')
  assert.ok(jobs.length >= 2, 'the fixture has running jobs')
  assert.ok(jobs.every((r) => !r.counted), 'the Ship’s counter leaves a job out')
  assert.ok(w.flight.every((r) => r.kind !== 'job'), 'so does the simple list')
  assert.equal(s.cols.working.length - jobs.length, w.flight.length + s.cols.working.filter((r) => r.kind !== 'job' && !r.counted).length)
})

test('the order j and k walk is every entry of every list once, top to bottom', () => {
  const w = workSetOf(store())
  assert.equal(new Set(w.order).size, w.order.length, 'no entry twice')
  const shown = [...w.queue.map((q) => q.key), ...w.waiting.map((r) => r.id), ...w.flight.map((r) => r.id), ...w.next.map((r) => r.id), ...w.cols.scheduled.map((r) => r.id), ...w.cols.paused.map((r) => r.id), ...w.resting.map((x) => x.session_id), ...w.cols.done.map((r) => r.id)]
  assert.deepEqual(w.order, shown)
  assert.ok(w.resting.some((x) => x.session_id === 'ses_rest'), 'a conversation at rest is listed')
})

test('each page still runs the steps this test runs: the simple page one work set, the Ship’s its own pipeline', () => {
  const ops = src('views/Ops.tsx')
  assert.match(ops, /const queue = useMemo\(\(\) => queueOf\(\{\n\s+confirms, views, records: world \? \[\] : records, groups: world \? \[\] : groups, asks: world \? \[\] : asks, executions: world\?\.executions, past: !!world, titleOf,/)
  assert.match(ops, /const rows = useMemo\(\(\) => boardRows\(\{ views, tasks, sessions, records, lines, queue, now, past: !!world \}\)/)
  assert.match(ops, /const counts = useMemo\(\(\) => allCounters\(rows, now, spend\.usd\)/)
  assert.match(ops, /const waiting = useMemo\(\(\) => unaskedCards\(rows\)/)
  const mine = src('simple/Ops.tsx')
  assert.match(mine, /workSetOf\(\{ views, tasks, sessions, records, confirms, groups, asks, history: history\.rows, now \}\)/)
  const code = mine.replace(/\/\/.*$/gm, '')
  for (const own of ['boardRows(', 'queueOf(', 'columnsOf(', 'allCounters(', 'spendToday(', 'unaskedCards(']) assert.ok(!code.includes(own), `the simple page builds no work set of its own: ${own}`)
  const set = src('simple/ops/workset.ts')
  for (const step of ['queueOf(', 'boardRows(', 'columnsOf(', 'allCounters(', 'spendToday(', 'unaskedCards(']) assert.ok(set.includes(step), step)
})

test('an ask is answered from the simple page by the Ship’s own answer: the same call, from the same card', () => {
  const w = workSetOf(store())
  const approval = w.queue.find((q) => q.kind === 'approval')
  assert.ok(approval, 'the fixture holds an approval')
  const yes = answerOf(approval, { verb: 'approve' })
  assert.equal(yes?.method, 'work.answer', 'every question is work.answer, as every surface answers it')
  assert.deepEqual(yes?.params, { question_id: approval.confirm!.correlation_id, option: 'approve' })
  const no = answerOf(approval, { verb: 'decline', note: 'not now' })
  assert.deepEqual(no?.params, { question_id: approval.confirm!.correlation_id, option: 'decline', note: 'not now' })
  const mine = src('simple/Ops.tsx')
  assert.match(mine, /import \{ Answers \} from '@\/ops\/Queue'/)
  assert.match(mine, /<Answers i=\{item\} whole /)
  assert.match(mine, /<ConfirmCard key=\{c\.correlation_id\} c=\{c\} here \/>/, 'a gate question on a session shows as the Ship’s card')
  // The Ship's answers act through the one wrapper that refuses under the time machine.
  assert.match(src('ops/Queue.tsx'), /const \{ busy, act, on \} = useOpsAct\(\)/)
})

test('a running turn is steered from the simple page by the Ship’s Join pane: ⏎ while it runs is turn.submit with steer', () => {
  assert.deepEqual(composerStep({ kind: 'running' }, { kind: 'enter', text: 'go left' }).effect, { send: 'go left', steer: true })
  const mine = src('simple/Ops.tsx')
  assert.match(mine, /import \{ JoinPane \} from '@\/ops\/JoinPane'/)
  assert.match(mine, /<JoinPane key=\{sid\} sessionId=\{sid\} session=\{row\?\.session \?\? session\}/)
  assert.match(src('ops/JoinPane.tsx'), /post\('join', 'turn\.submit', params\)/)
  // The page can stop and pause a running turn by key, too, through the same acts as the Ship's page.
  const keys = src('simple/ops/useOpsKeys.ts')
  assert.match(keys, /'execution\.stop', \{ execution_id: f\.exe \}/)
})
