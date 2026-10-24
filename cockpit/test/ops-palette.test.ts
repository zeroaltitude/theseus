// The palette's verbs over targets (theseus-jid7t.6): a scope, entries with their plans, refused targets counted with
// the floor's reason, and a greyed entry when nothing takes it.
import test from 'node:test'
import assert from 'node:assert/strict'
import type { ProfileInfo } from '../src/protocol.gen/index.ts'
import { boardRows } from '../src/ops/board.ts'
import { entitiesOf, nextScope, scopeTargets, searchValue, targetOf, verbEntries, type OpsTarget } from '../src/ops/palette.ts'
import { board, NOW } from './ops-fixtures.ts'

const prof = (name: string, model: string, origin: string): ProfileInfo => ({ name, provider: 'p', model, max_output_tokens: 1, has_system: false, live: false, origin })
const cx = { profiles: [prof('quick', 'm-s', 'us'), prof('deep', 'm-b', 'us'), prof('kimi', 'm-cn', 'cn')], prices: new Map([['m-s', { input: 1, output: 5, cacheRead: 0, cacheWrite: 0, cacheWrite1h: 0 }], ['m-b', { input: 4, output: 20, cacheRead: 0, cacheWrite: 0, cacheWrite1h: 0 }]]) }
const t = (id: string, over: Partial<OpsTarget> = {}): OpsTarget => ({ id, kind: 'task', title: id, sessionId: id, profile: 'quick', paused: false, running: true, failed: false, ended: false, ...over })
const by = (es: ReturnType<typeof verbEntries>, id: string) => es.find((e) => e.id === id)!

test('three cards selected: switch to the China-origin model is refused for the two that hold private data, per profile', () => {
  const es = verbEntries([t('a'), t('b', { data: 'private' }), t('c', { data: 'sensitive' })], cx)
  const kimi = by(es, 'model:kimi')
  assert.equal(kimi.plans.length, 1, 'only the open one may go')
  assert.match(kimi.detail, /1 hold private data · 1 hold sensitive data/)
  const deep = by(es, 'model:deep')
  assert.equal(deep.plans.length, 3)
  assert.match(deep.detail, /~4× the cost/)
  assert.equal(by(es, 'model:quick').grey, 'already on it')
  assert.equal(by(es, 'model:quick').plans.length, 0)
  assert.ok(by(es, 'model:quick').grey)
})

test('a sensitive card alone: the China-origin entry is greyed with the reason; a plan is one session.profile', () => {
  const es = verbEntries([t('c', { data: 'sensitive' })], cx)
  assert.ok(by(es, 'model:kimi').grey)
  assert.deepEqual(by(es, 'model:deep').plans[0], { verb: 'model_menu', key: 'c:model', method: 'session.profile', params: { session_id: 'c', profile: 'deep' } })
})

test('pause, resume, cancel and stop take only what they apply to; the rest is counted', () => {
  const es = verbEntries([t('a'), t('b', { paused: true }), t('c', { ended: true }), t('d', { kind: 'job', sessionId: undefined }), t('e', { kind: 'conversation', executionId: 'exe_e' })], cx)
  assert.deepEqual(by(es, 'pause').plans.map((p) => p.key), ['a'])
  assert.deepEqual(by(es, 'resume').plans.map((p) => [p.key, p.method]), [['b', 'task.resume']])
  assert.equal(by(es, 'cancel').plans.length, 2)
  assert.match(by(es, 'cancel').plans[0].ask!, /Cancel 2 tasks/)
  assert.deepEqual(by(es, 'stop').plans.map((p) => p.params), [{ execution_id: 'exe_e' }])
  assert.equal(by(es, 'retry').grey, 'not failed')
})

test('modes go to conversations; Auto asks; with nothing in scope only Start a task is offered', () => {
  const es = verbEntries([t('e', { kind: 'conversation' })], cx)
  assert.match(by(es, 'mode:auto').plans[0].ask!, /Auto/)
  assert.equal(by(es, 'mode:plan').plans[0].ask, undefined)
  assert.equal(by(verbEntries([t('a')], cx), 'mode:plan').grey, 'not a conversation')
  assert.deepEqual(verbEntries([], cx).map((e) => e.id), ['new_task'])
})

test('the scope: one, all visible, the selection from the seam; ⇥ cycles; both word orders match', () => {
  const [a, b, c] = [t('a'), t('b'), t('c')]
  const from = { focused: a, visible: [a, b], selection: [c] }
  assert.deepEqual(scopeTargets('one', from).map((x) => x.id), ['a'])
  assert.deepEqual(scopeTargets('visible', from).map((x) => x.id), ['a', 'b'])
  assert.deepEqual(scopeTargets('selection', from).map((x) => x.id), ['c'])
  assert.deepEqual(scopeTargets('one', { ...from, focused: undefined }), [])
  assert.equal(nextScope('selection'), 'one')
  assert.ok(searchValue('pause', 'Tide tables').includes('tide tables pause') && searchValue('pause', 'Tide tables').startsWith('pause tide tables'))
})

test('a board row becomes a target with its profile, mark and execution', () => {
  const rows = new Map(boardRows({ ...board(), now: NOW }).map((r) => [r.id, r]))
  const x = targetOf(rows.get('ses_t1')!)
  assert.equal(x.kind, 'task')
  assert.equal(x.data, 'sensitive')
  assert.equal(x.profile, 'quick')
})

test('entities: sessions, tasks, threads, routines, projects and profiles, each with where choosing it goes', () => {
  const rows = boardRows({ ...board(), now: NOW })
  const es = entitiesOf(rows, [{ slug: 'tide-tables', title: 'Tide tables' }], [{ id: 'tsk_p', kind: 'project', title: 'Harbour', session: undefined }] as never, cx.profiles)
  const kinds = new Set(es.map((e) => e.kind))
  for (const k of ['session', 'task', 'thread', 'project', 'profile', 'job']) assert.ok(kinds.has(k as never), k)
  assert.deepEqual(es.find((e) => e.kind === 'thread')!.go, { sel: 'tide-tables' })
  assert.deepEqual(es.find((e) => e.id === 'kimi')!.go, { model: 'kimi' })
  assert.match(es.find((e) => e.id === 'ses_t1')!.hint, /sensitive/)
})
