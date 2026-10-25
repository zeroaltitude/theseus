// Old nodes retire softly (theseus-99l9z), run by `npm test`: the five classes and what the default hides, each
// checkbox bringing back its class and only it, the classes read off the work set's buckets, nothing deleted and no
// counter moved, the choice kept in the address and in the browser, and the Actions panel's graph opening in view.
// Invented ids.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { boardRows, countersOf, type OpsRow } from '../src/ops/board.ts'
import { bucketOfCard } from '../src/ops/state.ts'
import {
  ALL_SHOWN, CLASSES, DAY_MS, DEFAULT_CHOICE, NONE_SHOWN, classOf, hiddenIds, olderWords, readChoice, rowItem, split, storageKey, tally,
  writeParams, writeStored, type Choice, type RetireClass, type Retirable, type Shown, type Store,
} from '../src/ops/retire.ts'
import { writeTaskGraph } from '../src/lib/taskgraph.ts'
import { parseFull } from '../src/lib/fullgraph.ts'
import { HOUR, MIN, NOW, record, session, view } from './ops-fixtures.ts'

const day = (n: number): number => NOW - n * DAY_MS

/** The work set: running, needs-you and waiting cards, a conversation at rest, an ended task, a retired session, a step
 *  touched 2 days ago and one 30 days ago, and a backlog of terminal-state records. */
function fixture(): OpsRow[] {
  const views = [
    view('exe_run', 'conversation', 'running', { session_id: 'ses_run', at_ms: NOW - MIN }),
    view('exe_ask', 'conversation', 'needs_you', { session_id: 'ses_ask', at_ms: day(40) }),
    view('act_wait', 'job', 'waiting', { session_id: 'ses_run', at_ms: NOW - HOUR }),
    view('exe_rest', 'conversation', 'ready', { session_id: 'ses_rest', at_ms: day(20) }),
    view('exe_done', 'conversation', 'done', { session_id: 'ses_done', at_ms: day(1) }),
  ]
  const sessions = [session('ses_retired', { retired: { reason: 'manual', at_ms: day(3) } as never, last_active_ms: day(3) }), session('ses_rest', { last_active_ms: day(20) })]
  const records = [
    record('tsk_recent', { state: 'accepted', updated_at_ms: day(2) }),
    record('tsk_quiet', { state: 'accepted', updated_at_ms: day(10) }),
    record('tsk_old', { state: 'accepted', updated_at_ms: day(30) }),
    ...Array.from({ length: 6 }, (_, i) => record(`tsk_gone${i}`, { state: i % 2 ? 'done' : 'abandoned', updated_at_ms: day(10 + i) })),
  ]
  const rows = boardRows({ views, tasks: [], sessions, records, now: NOW, past: false })
  // The retired session is a card of its own in the work set (the Fleet's sessions are cards in the time machine).
  rows.push({ ...rows[0]!, id: 'ses_retired', bucket: 'rest', counted: false, column: null, session: sessions[0], activityMs: day(3), dueMs: undefined })
  return rows
}

const idsHidden = (rows: readonly OpsRow[], shown: Shown, days = 7): string[] =>
  split(rows, rowItem, shown, NOW, days).hidden.map((r) => r.id).sort()
const only = (...cs: RetireClass[]): Shown => Object.fromEntries(CLASSES.map((c) => [c, cs.includes(c)])) as unknown as Shown
const classes = (rows: readonly OpsRow[]): Record<string, RetireClass | null> => Object.fromEntries(rows.map((r) => [r.id, classOf(rowItem(r), NOW)]))

test('the default hides ended, retired and stale; live, waiting with work and recent stay', () => {
  const rows = fixture()
  const c = classes(rows)
  assert.equal(c['ses_run'], null, 'running is live')
  assert.equal(c['ses_ask'], null, 'needs you is live, however old')
  assert.equal(c['act_wait'], null, 'a job in flight is live')
  assert.equal(c['tsk_recent'], null, 'a step touched 2 days ago is recent')
  assert.equal(c['tsk_quiet'], 'idle', 'open and quiet for 10 days is idle')
  assert.equal(c['tsk_old'], 'stale', 'a step touched 30 days ago is stale')
  assert.equal(c['ses_done'], 'ended')
  assert.equal(c['ses_retired'], 'retired')
  assert.equal(c['ses_rest'], 'rest')
  assert.equal(c['tsk_gone0'], 'ended')
  const hidden = idsHidden(rows, DEFAULT_CHOICE.show)
  for (const keep of ['ses_run', 'ses_ask', 'act_wait', 'tsk_recent']) assert.ok(!hidden.includes(keep), `${keep} is drawn by default`)
  for (const gone of ['ses_done', 'ses_retired', 'tsk_old', 'tsk_gone0', 'tsk_gone1']) assert.ok(hidden.includes(gone), `${gone} is hidden by default (ended, retired and stale are out)`)
})

test('each checkbox brings back its class and only it, in every combination of the five', () => {
  const rows = fixture()
  const all = idsHidden(rows, NONE_SHOWN)
  const byClass = Object.fromEntries(CLASSES.map((c) => [c, rows.filter((r) => classOf(rowItem(r), NOW) === c).map((r) => r.id).sort()])) as Record<RetireClass, string[]>
  for (const c of CLASSES) assert.ok(byClass[c].length > 0, `the fixture holds a ${c} card`)
  for (let mask = 0; mask < 32; mask++) {
    const picked = CLASSES.filter((_, i) => mask & (1 << i))
    const back = new Set(picked.flatMap((c) => byClass[c]))
    assert.deepEqual(idsHidden(rows, only(...picked)), all.filter((id) => !back.has(id)), `checked: ${picked.join(', ') || 'nothing'}`)
  }
  assert.deepEqual(idsHidden(rows, ALL_SHOWN), [])
})

test('at rest is not ended: neither box brings back the other', () => {
  const rows = fixture()
  const hiddenWith = (s: Shown) => new Set(idsHidden(rows, s))
  const ended = hiddenWith(only('ended'))
  assert.ok(ended.has('ses_rest'), 'checking "ended" leaves the resting conversation hidden')
  assert.ok(!ended.has('ses_done'))
  const rest = hiddenWith(only('rest'))
  assert.ok(!rest.has('ses_rest'), 'checking "at rest" brings the resting conversation back')
  assert.ok(rest.has('ses_done') && rest.has('tsk_gone0'), 'and leaves ended ones hidden')
})

test('the classes read the work set\'s bucket: a new state that buckets as ended is hidden with no change here', () => {
  const x: Retirable = { bucket: 'ended', touchedMs: NOW }
  assert.equal(classOf(x, NOW), 'ended')
  // A record state the table has never seen reaches `bucketOfCard` as ended through the one model...
  const rec = record('tsk_future', { state: 'done', updated_at_ms: NOW })
  assert.equal(bucketOfCard({ kind: 'step', record: rec }, NOW), 'ended')
  // ...and a row carrying that bucket is hidden whatever its state string says.
  const row = { ...fixture()[0]!, state: 'a_state_nobody_wrote_yet', bucket: 'ended' as const, activityMs: NOW }
  assert.equal(classOf(rowItem(row), NOW), 'ended')
  assert.equal(classOf(rowItem({ ...row, state: 'done', bucket: 'working' }), NOW), null, 'a "done" state string with a working bucket is not ended')
  // The module names no state string.
  const src = readFileSync(new URL('../src/ops/retire.ts', import.meta.url), 'utf8').replace(/^\s*\/\/.*$/gm, '')
  assert.ok(!/['"](done|failed|cancelled|expired|abandoned|complete)['"]/.test(src), 'no list of state strings')
})

test('nothing is deleted and no counter moves: the hidden chip counts exactly what is left out', () => {
  const rows = fixture()
  const before = countersOf(rows, NOW, 1.5)
  const snapshot = JSON.stringify(rows)
  const parts = split(rows, rowItem, NONE_SHOWN, NOW)
  assert.ok(parts.hidden.length > 0)
  assert.equal(parts.shown.length + parts.hidden.length, rows.length, 'every row is in one half')
  assert.equal(JSON.stringify(rows), snapshot, 'the set is unchanged')
  assert.deepEqual(countersOf(rows, NOW, 1.5), before, 'the counters read the set, not the drawing')
  const out = hiddenIds(rows, (r) => r.id, (r) => r.parent, rowItem, NONE_SHOWN, NOW)
  assert.ok(out.size <= parts.hidden.length, 'ancestors of what is drawn are spared')
  assert.equal(olderWords(214), '+214 older')
  const t = tally(rows, rowItem, NOW)
  assert.equal(CLASSES.reduce((n, c) => n + t[c], 0), parts.hidden.length, 'the checkboxes\' numbers add up to the hidden count')
  // The views hand the counters the whole set: the lens filters its drawing, never `rows`.
  const lens = readFileSync(new URL('../src/ops/GraphLens.tsx', import.meta.url), 'utf8')
  assert.match(lens, /keep: kept/, 'the layout is told what to leave out')
  assert.ok(!/rows\.filter|rows=\{/.test(lens.split('const retire = useRetire')[1] ?? ''), 'rows are not filtered')
})

class Mem implements Store {
  m = new Map<string, string>()
  getItem(k: string) { return this.m.get(k) ?? null }
  setItem(k: string, v: string) { this.m.set(k, v) }
}

test('the choice is kept in the address and in the browser; a reload and a shared link both restore it', () => {
  const choice: Choice = { show: only('ended', 'rest'), days: 14 }
  const store = new Mem()
  const params = writeParams(new URLSearchParams('lens=graph&task=tsk_x'), choice)
  writeStored(store, 'graph', choice)
  assert.equal(params.get('show'), 'ended,rest')
  assert.equal(params.get('older'), '14')
  assert.equal(params.get('task'), 'tsk_x', 'other parameters stay')
  assert.deepEqual(readChoice(params, undefined, 'graph'), choice, 'a shared link: the address alone')
  assert.deepEqual(readChoice(new URLSearchParams('lens=graph'), store, 'graph'), choice, 'a reload with a bare address: this browser')
  assert.ok(store.getItem(storageKey('graph')), 'the browser holds it')
  assert.deepEqual(readChoice(new URLSearchParams('lens=graph'), store, 'fleet'), DEFAULT_CHOICE, 'each view keeps its own')
  assert.deepEqual(readChoice(new URLSearchParams('show=idle'), store, 'graph'), { show: only('idle'), days: 14 }, 'the address wins where it speaks')
  assert.deepEqual(readChoice(writeParams(new URLSearchParams(), { show: NONE_SHOWN, days: 7 }), store, 'graph').show, NONE_SHOWN, 'unchecking everything is said in the address too')
  assert.deepEqual(readChoice(new URLSearchParams('show=bogus&older=-3'), undefined, 'graph'), DEFAULT_CHOICE)
  const bar = readFileSync(new URL('../src/ops/RetireBar.tsx', import.meta.url), 'utf8')
  assert.match(bar, /writeStored\(storage\(\), view, c\)/, 'a change is written to storage')
  assert.match(bar, /setParams\(\(p\) => writeParams\(p, c\)/, 'and to the address')
})

test('the Actions panel\'s "open as a graph" puts the graph where the click was, and ?taskgraph=1 still deep-links', () => {
  const opened = writeTaskGraph(new URLSearchParams('x=1'), true)
  assert.equal(opened.get('taskgraph'), '1')
  assert.ok(parseFull(opened), 'nothing would render in that panel if the graph stayed in the panel further down: it fills the window')
  assert.equal(opened.get('x'), '1')
  const closed = writeTaskGraph(new URLSearchParams('taskgraph=1&graph=full&task=t1&x=1'), false)
  assert.equal(closed.toString(), 'x=1')
  const panel = readFileSync(new URL('../src/components/TaskGraph.tsx', import.meta.url), 'utf8')
  assert.match(panel, /writeTaskGraph\(p, !open\)/, 'the button opens the graph in view; the parameter alone renders nothing in that panel (the bug)')
  assert.match(panel, /params\.get\('taskgraph'\) === '1'/, 'the deep link still opens the graph')
  assert.match(panel, /scrollIntoView/, 'a deep link scrolls it into view')
  assert.match(panel, /help="actions\.taskgraph"/)
})
