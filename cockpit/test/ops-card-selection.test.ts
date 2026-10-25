// Every card opens the inspector (theseus-gnhnk): a click on any node the Graph draws resolves, through the page's
// selection path, to a row whose inspector body is the right one; a step or plan has its own body (`StepWhole`'s
// facts); the idle tab opens the panel even when Needs you is empty; and a selection and the counter filter leave each
// other alone. All pure data: React is not rendered here.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { boardRows, shown, type OpsRow } from '../src/ops/board.ts'
import { layoutRows } from '../src/ops/graph.ts'
import { queueOf } from '../src/ops/queue.ts'
import { parseAddress, writeAddress } from '../src/ops/address.ts'
import { idleSel, OPEN_SEL, regionsFor, WIDE_PX, NARROW_PX, type Fold } from '../src/ops/regions.ts'
import { bodyOf, carriedWords, NONE_WORDS, noneCounts, rowFor, stepView } from '../src/ops/sections.ts'
import { allCounters } from '../src/ops/board.ts'
import { board, confirm, NOW, record, view } from './ops-fixtures.ts'

const src = (f: string) => readFileSync(new URL(`../src/${f}`, import.meta.url), 'utf8')

function world() {
  const b = board()
  const views = [...b.views, view('exe_hull', 'step', 'running', { title: 'Scrape the hull', parent: 'exe_tides', thread: 'tide-tables' })]
  const records = [
    record('tsk_plan', { title: 'Chart the estuary', state: 'in_progress', origin: { session: 'ses_tides' } }),
    record('tsk_sound', { title: 'Sound the channel', parent: 'tsk_plan', state: 'done', origin: { session: 'ses_tides' }, updated_at_ms: NOW - 30 * 60_000 }),
    record('tsk_shoals', { title: 'Mark the shoals', parent: 'tsk_plan', state: 'suspended', origin: { session: 'ses_tides' }, deps: ['tsk_sound'] }),
    record('tsk_sub', { title: 'Buoy the shoals', parent: 'tsk_shoals', state: 'waiting_human', origin: { session: 'ses_tides' } }),
    record('tsk_carry', { title: 'Tabulate by tide', parent: 'tsk_plan', state: 'in_progress', session: 'ses_t2', origin: { session: 'ses_tides' } }),
    record('tsk_loose', { title: 'Mend the nets', state: 'accepted', origin: { session: 'ses_home' } }),
  ]
  const confirms = [...b.confirms, confirm('act_lost', { session_id: 'ses_nobody', execution_id: 'exe_nobody', task: undefined })]
  const queue = queueOf({ confirms, views })
  const rows = boardRows({ ...b, views, records, queue, now: NOW })
  return { b, views, records, queue, rows }
}

/** The page's selection path for an address `sel`: a queue item first, else the row, else nothing. */
const resolve = (w: ReturnType<typeof world>, sel: string) => {
  const item = w.queue.find((i) => i.key === sel)
  if (item) return { kind: 'item' as const }
  const row = rowFor(w.rows, sel)
  return row ? { kind: 'row' as const, row } : { kind: 'none' as const }
}

test('a click on every node the layout emits fills the inspector, with the body of its kind', () => {
  const w = world()
  const g = layoutRows(w.rows, [], null, { now: NOW, future: true })
  const kinds = new Set(g.nodes.map((n) => n.kind))
  for (const k of ['conversation', 'task', 'step', 'job', 'ask', 'wake']) assert.ok(kinds.has(k as never), `the fixture draws a ${k} node`)
  const failed: string[] = []
  for (const n of g.nodes) {
    if (n.kind === 'done' || n.kind === 'folded') continue // a fold is a button of its own (it opens), not a card
    const s = resolve(w, n.id)
    if (s.kind === 'none') { failed.push(`${n.kind} ${n.id}`); continue }
    if (s.kind === 'row') {
      const want = n.kind === 'job' ? 'job' : n.kind === 'step' || n.kind === 'project' ? 'step' : 'row'
      if (bodyOf(s.row) !== want) failed.push(`${n.kind} ${n.id}: body ${bodyOf(s.row)}, not ${want}`)
    }
  }
  assert.deepEqual(failed, [], `these node kinds opened nothing: ${failed.join('; ')}`)
})

test('the step rows the owner clicked are in the set, by the id their node carries', () => {
  const w = world()
  for (const id of ['tsk_shoals', 'tsk_sub', 'tsk_loose', 'tsk_plan', 'exe_hull']) assert.equal(resolve(w, id).kind, 'row', id)
  assert.equal(rowFor(w.rows, 'tsk_shoals')?.kind, 'step')
  assert.equal(rowFor(w.rows, 'tsk_carry')?.id, 'ses_t2', 'a record that rides on its task is found by its task row')
  assert.equal(rowFor(w.rows, 'ses_t2')?.recordId, 'tsk_carry')
})

test('the page asks rowFor for its row, and the Graph hands every click to pick', () => {
  const page = src('views/Ops.tsx')
  assert.match(page, /item \? undefined : rowFor\(rows, a\.sel\)/)
  assert.match(page, /onFocus=\{pick\}/)
  assert.match(src('ops/GraphLens.tsx'), /else onFocus\(n\.id\)/)
  assert.match(src('ops/GraphLens.tsx'), /if \(byId\.has\(n\.id\)\) onPick\(n\.id\)/)
  assert.match(src('ops/GraphLens.tsx'), /onContext\(n\.id, e\.clientX, e\.clientY\)/)
})

test('shift-click and the right-click menu reach every card with a row: their lookup is the selection path', () => {
  const w = world()
  const byId = new Map(w.rows.map((r) => [r.id, r]))
  const g = layoutRows(w.rows, [], null, { now: NOW, future: true })
  const missing = g.nodes.filter((n) => n.kind !== 'done' && n.kind !== 'folded' && !byId.has(n.id)).map((n) => `${n.kind} ${n.id}`)
  assert.deepEqual(missing, [], 'GraphLens picks and opens the menu only for a node with a row: every card has one')
})

test('StepWhole: a plan gives n of m and its children as a checklist', () => {
  const w = world()
  const plan = rowFor(w.rows, 'tsk_plan')!
  const v = stepView(plan, w.rows, NOW)
  assert.equal(v.noun, 'plan')
  assert.deepEqual(v.progress, { n: 1, m: 3 })
  assert.deepEqual(v.children.map((c) => c.id).sort(), ['ses_t2', 'tsk_shoals', 'tsk_sound'].sort())
  assert.equal(v.children.find((c) => c.id === 'tsk_sound')!.closed, true)
  assert.equal(v.children.find((c) => c.id === 'tsk_shoals')!.closed, false)
  assert.ok(v.history.some((l) => l.label === 'became a plan'))
})

test('StepWhole: a nested step has its breadcrumb, its waits and its ledger link', () => {
  const w = world()
  const sub = stepView(rowFor(w.rows, 'tsk_sub')!, w.rows, NOW)
  assert.equal(sub.noun, 'step')
  assert.deepEqual(sub.crumbs.map((c) => c.id), ['ses_tides', 'tsk_plan', 'tsk_shoals'], 'the conversation it came from, the plan, its parent step')
  assert.equal(sub.ledger, '/ledger?q=tsk_sub')
  assert.equal(sub.stateWord, 'waiting human')
  const shoals = stepView(rowFor(w.rows, 'tsk_shoals')!, w.rows, NOW)
  assert.equal(shoals.stateWord, 'suspended')
  assert.equal(shoals.noun, 'plan', 'a step with a step under it is a plan')
})

test('StepWhole: a taskless step is carried by its conversation; a step with a task shows the task', () => {
  const w = world()
  const loose = stepView(rowFor(w.rows, 'tsk_loose')!, w.rows, NOW)
  assert.equal(loose.task, undefined)
  assert.equal(loose.carrier?.id, 'ses_home')
  assert.equal(carriedWords(loose), `carried by ${rowFor(w.rows, 'ses_home')!.title}`)
  // A step view that names a task's session: the task row's sections and verbs are drawn.
  const hull = { ...rowFor(w.rows, 'exe_hull')!, sessionId: 'ses_t2' } as OpsRow
  const withTask = stepView(hull, w.rows, NOW)
  assert.equal(withTask.task?.id, 'ses_t2')
  const inspector = src('ops/Inspector.tsx')
  assert.match(inspector, /v\.task \? <RowWhole r=\{v\.task\}/, 'the task row\'s whole body (facts, Live, pause, stop, cancel) is the step\'s when one is attached')
  assert.match(inspector, /carriedWords\(v\)/)
})

test('the idle tab opens the panel: with no ask it selects nothing and says so; with an ask it takes the first', () => {
  assert.equal(idleSel(undefined), OPEN_SEL)
  assert.equal(idleSel('q:act_c1'), 'q:act_c1')
  const page = src('views/Ops.tsx')
  assert.match(page, /label="Inspector · select a card" onClick=\{\(\) => pick\(idleSel\(queue\[0\]\?\.key\)\)\}/)
  assert.ok(!/select a card" onClick=\{\(\) => pick\(queue\[0\]\?\.key \?\? null\)\}/.test(page), 'the base\'s no-op is gone')
  // The sentinel is a selection of nothing: `none`, with the empty state's words and counts.
  assert.match(page, /\{ kind: 'none', counts: noneCounts\(counts\)/)
  assert.match(page, /a\.sel === OPEN_SEL/)
  assert.equal(NONE_WORDS, 'Nothing is selected. Click a card, or press j to take the first.')
  const w = world()
  assert.equal(resolve(w, OPEN_SEL).kind, 'none', 'no row is named ~')
  assert.equal(parseAddress(writeAddress({ ...parseAddress(''), sel: OPEN_SEL })).sel, OPEN_SEL, 'a link reopens the empty panel')
  const c = noneCounts(allCounters(w.rows, NOW, 0))
  assert.deepEqual(c.map((x) => x.b), ['needs', 'working', 'paused', 'still', 'done'])
})

test('regionsFor: the existing cases are unchanged, and a panel opened by hand with nothing selected is side', () => {
  const widths = [1440, WIDE_PX, 1100, 900, NARROW_PX - 1, 600]
  const folds: Fold[] = ['open', 'folded']
  // The base's rule, written out again as the table's oracle for byHand = false.
  const base = (width: number, q: Fold, i: Fold, selected: boolean) => {
    if (width < NARROW_PX) return { queue: 'page', inspector: selected ? 'over' : 'none', canvas: false }
    const qq = q === 'folded' ? 'folded' : 'side'
    if (i === 'folded') return { queue: qq, inspector: 'folded', canvas: true }
    if (width < WIDE_PX) return { queue: qq, inspector: selected ? 'over' : 'none', canvas: true }
    return { queue: qq, inspector: selected ? 'side' : 'idle', canvas: true }
  }
  for (const w of widths) for (const q of folds) for (const i of folds) for (const sel of [true, false]) {
    assert.deepEqual(regionsFor(w, q, i, sel), base(w, q, i, sel), `${w} ${q} ${i} ${sel}`)
    assert.deepEqual(regionsFor(w, q, i, sel, false), base(w, q, i, sel), `${w} ${q} ${i} ${sel} by hand off`)
    if (sel) assert.deepEqual(regionsFor(w, q, i, sel, true), base(w, q, i, sel), `a selection is a selection: ${w} ${q} ${i}`)
  }
  assert.deepEqual(regionsFor(1440, 'open', 'open', false, true), { queue: 'side', inspector: 'side', canvas: true })
  assert.deepEqual(regionsFor(1100, 'open', 'open', false, true), { queue: 'side', inspector: 'over', canvas: true })
  assert.deepEqual(regionsFor(800, 'open', 'open', false, true), { queue: 'page', inspector: 'over', canvas: false })
  assert.deepEqual(regionsFor(1440, 'open', 'folded', false, true), { queue: 'side', inspector: 'folded', canvas: true }, 'a fold still folds it')
})

test('a selection and the counter filter leave each other alone', () => {
  const w = world()
  const cx = { now: NOW, spentToday: new Set<string>(), records: w.records }
  const pausedOnly = shown(w.rows, { bucket: 'paused' }, '', cx).rows
  const hidden = w.rows.find((r) => !pausedOnly.includes(r) && r.kind === 'step')!
  assert.ok(hidden, 'the fixture has a step the paused filter hides')
  // Selecting it resolves from the whole set; the filter's shown set is not widened by the selection.
  assert.equal(resolve(w, hidden.id).kind, 'row')
  assert.ok(!shown(w.rows, { bucket: 'paused' }, '', cx).rows.some((r) => r.id === hidden.id), 'the card stays hidden by the filter while selected')
  // Clearing the filter in the address keeps the selection.
  const a = { ...parseAddress(''), sel: hidden.id, f: { bucket: 'paused' } }
  const cleared = parseAddress(writeAddress({ ...a, f: {} }))
  assert.equal(cleared.sel, hidden.id)
  assert.deepEqual(cleared.f, {})
  assert.equal(parseAddress(writeAddress(a)).f.bucket, 'paused', 'and selecting keeps the filter')
})
