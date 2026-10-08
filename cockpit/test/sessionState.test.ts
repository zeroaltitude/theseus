// Session states and the Ship's filter (`src/lib/sessionState.ts`, `src/ship/states.ts`, theseus-emqx), run by
// `npm test`: the derived state at the window's edges (the daemon's rule, line for line), the filter's counts, its
// address and its default, a hidden selection shown as a visitor, the palette listing every session with the retired
// ones last, the time machine's fold of the state rows, and the view that keeps every vessel's slot.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  busyOf, countsOf, DEFAULT_RULE, deriveState, filterOf, foldLife, paletteOrder, paramOf, ruleOf, searchText, shownIds,
  type Life,
} from '../src/lib/sessionState.ts'
import { buildModel } from '../src/ship/model.ts'
import { hiddenOf, viewOf } from '../src/ship/states.ts'
import { synthInput } from '../src/ship/synth.ts'

const H = 3_600_000
const NOW = 1_800_000_000_000

const of = (turns: number, created: number, active: number, o: Record<string, unknown> = {}) =>
  ({ turns, created_ms: created, last_active_ms: active, busy: false, ...o }) as any

test('the window has two edges, and an empty session its grace', () => {
  assert.equal(deriveState(of(3, 0, NOW - 24 * H + 1), DEFAULT_RULE, NOW).state, 'live')
  assert.equal(deriveState(of(3, 0, NOW - 24 * H), DEFAULT_RULE, NOW).state, 'quiet')
  const created = NOW - H
  assert.equal(deriveState(of(0, created + 1, created + 1), DEFAULT_RULE, NOW).state, 'live', 'within its grace')
  assert.deepEqual(deriveState(of(0, created, created), DEFAULT_RULE, NOW), { state: 'retired', retired: { reason: 'empty', at_ms: NOW } })
  assert.equal(deriveState(of(0, created, created, { reopened_ms: NOW - 1 }), DEFAULT_RULE, NOW).state, 'live', 'reopened')
  // A window the daemon names is the one used.
  assert.equal(deriveState(of(3, 0, NOW - 2 * H), ruleOf({ live_window_ms: H, empty_grace_ms: H }), NOW).state, 'quiet')
})

test('busy reads live whatever its age or its retirement, and still says why it was retired', () => {
  const byHand = { reason: 'by_hand' as const, at_ms: 5 }
  assert.deepEqual(deriveState(of(4, 0, 0, { retired: byHand, busy: true }), DEFAULT_RULE, NOW), { state: 'live', retired: byHand })
  assert.equal(deriveState(of(4, 0, NOW, { retired: byHand }), DEFAULT_RULE, NOW).state, 'retired')
  assert.equal(busyOf({ execution_state: 'waiting', pending_confirms: 1 } as any), true)
  assert.equal(busyOf({ execution_state: 'queued', pending_confirms: 0 } as any), true)
  assert.equal(busyOf({ execution_state: 'waiting', pending_confirms: 0, attention: { level: 'needs_you' } } as any), true)
  assert.equal(busyOf({ execution_state: 'waiting', pending_confirms: 0, attention: { level: 'ready' } } as any), false)
})

test('the filter is Live by default, and its address round-trips', () => {
  assert.equal(filterOf(null), 'live')
  assert.equal(filterOf(undefined), 'live')
  assert.equal(filterOf('nonsense'), 'live')
  for (const f of ['live', 'quiet', 'retired', 'all'] as const) assert.equal(filterOf(paramOf(f)), f)
  assert.equal(paramOf('live'), null, 'a link with no parameter shows Live')
})

const s = (id: string, state: string, o: Record<string, unknown> = {}) => ({ session_id: id, kind: 'conversation', state, ...o }) as any
const fleet = [
  s('a', 'live'), s('b', 'quiet'), s('c', 'retired'), s('d', 'retired'),
  s('t1', 'quiet', { kind: 'task', parent_session_id: 'a' }), s('t2', 'live', { kind: 'task', parent_session_id: 'c' }),
  s('old', undefined as any),
]

test('the counts are the conversations in each state, a daemon without states counting as live', () => {
  assert.deepEqual(countsOf(fleet), { live: 2, quiet: 1, retired: 2, all: 5 })
})

test('a filter shows its sessions, their tasks in tow, and the visitors with whatever started them', () => {
  assert.deepEqual([...shownIds(fleet, 'live')].sort(), ['a', 'old', 't1', 't2'])
  assert.deepEqual([...shownIds(fleet, 'retired')].sort(), ['c', 'd', 't2'])
  assert.deepEqual([...shownIds(fleet, 'all')].sort(), fleet.map((x) => x.session_id).sort())
  // A hidden selection (or a fly from the palette, a link, a plate) shows, with its parent.
  const v = shownIds(fleet, 'quiet', ['t2', null])
  assert.ok(v.has('t2') && v.has('c'), [...v].join())
  assert.ok(!v.has('d'))
})

test('the palette lists every session, the retired ones after the others, and matches old titles', () => {
  const order = paletteOrder(fleet).map((x: any) => x.session_id)
  assert.equal(order.length, fleet.length, 'every session, whatever the filter')
  assert.deepEqual(order.slice(-2), ['c', 'd'])
  assert.match(searchText({ session_id: 'ses_x', title: 'Chart the reef', label: null, title_was: ['hi there'] } as any), /hi there/)
})

test('the time machine folds the state rows: a move both ways, a retirement, a reopen', () => {
  const lives = new Map<string, Life>()
  const row = (kind: string, sid: string, at: number, data: Record<string, unknown> = {}) => ({ kind, session_id: sid, at_unix_ms: at, data })
  foldLife(lives, row('session.superseded', 'old', 10, { superseded_by: 'new', place: 'dm:42' }))
  assert.deepEqual(lives.get('old'), { supersededBy: { session_id: 'new', at_ms: 10, place: 'dm:42' }, retired: { reason: 'superseded', at_ms: 10 } })
  assert.deepEqual(lives.get('new'), { supersedes: { session_id: 'old', at_ms: 10, place: 'dm:42' } })
  foldLife(lives, row('session.retired', 'new', 20, { reason: 'by_hand' }))
  assert.deepEqual(lives.get('new')!.retired, { reason: 'by_hand', at_ms: 20 })
  foldLife(lives, row('session.reopened', 'old', 30, { was: 'superseded' }))
  const old = lives.get('old')!
  assert.equal(old.retired, undefined)
  assert.equal(old.reopened, 30)
  assert.equal(old.supersededBy!.session_id, 'new', 'the history stays')
  foldLife(lives, row('turn.ended', 'old', 40))
  assert.equal(lives.get('old'), old, 'another kind changes nothing')
})

test('the view keeps every vessel in its slot, so switching the filter never reshuffles the sea', () => {
  const input = synthInput(NOW)
  const whole = buildModel(input)
  const all = whole.vessels.map((v) => ({ session_id: v.id, kind: v.kind, state: v.life, parent_session_id: v.parentId }))
  assert.equal(viewOf(whole, new Set(whole.vessels.map((v) => v.id))), whole, 'all of it is the model itself')
  const counts = countsOf(all)
  assert.ok(counts.live > 0 && counts.quiet > 0 && counts.retired > 0, JSON.stringify(counts))
  const reasons = new Set(whole.vessels.map((v) => v.retired?.reason).filter(Boolean))
  assert.deepEqual([...reasons].sort(), ['by_hand', 'empty', 'superseded'], 'the synthetic fleet has every reason')
  assert.ok(whole.vessels.some((v) => v.supersededBy) && whole.vessels.some((v) => v.supersedes))
  for (const f of ['live', 'quiet', 'retired'] as const) {
    const view = viewOf(whole, shownIds(all, f))
    for (const v of view.vessels) {
      const w = whole.vessels[whole.byId.get(v.id)!]
      assert.deepEqual([v.x, v.z, v.heading], [w.x, w.z, w.heading], `${v.id} keeps its slot under ${f}`)
    }
    // Every index points into the view.
    for (const b of view.benches) assert.ok(b.vessel < view.vessels.length && b.lights.every((l) => view.lights[l].vessel === b.vessel))
    for (const l of view.lights) assert.equal(view.vessels[l.vessel].id, l.sessionId)
    for (const t of view.tethers) assert.ok(t.from < view.vessels.length && t.to < view.vessels.length)
    for (const [id, i] of view.byId) assert.equal(view.vessels[i].id, id)
    assert.deepEqual(view.bounds, whole.bounds, 'a fit frames the same sea')
    const hidden = hiddenOf(whole, view)
    assert.equal(hidden.sessions + hidden.tasks, whole.vessels.length - view.vessels.length)
  }
})
