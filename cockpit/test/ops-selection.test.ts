// The selection and its bulk acts (theseus-jid7t.4): toggle, select all, clear; each bulk verb with its count and the
// items that admit it, or the reason none does; decision 4 (bulk Approve only for tool approvals not of the always-ask
// kind, that this page may answer); the fan-out (at most four at a time, one refusal never stops the others, results in
// order); failed items stay selected. And drag: every row of the drag table and every refusal.
import test from 'node:test'
import assert from 'node:assert/strict'
import { afterRun, bulkApprovable, bulkVerbs, clear, fanOut, selectAll, toggle, type BulkVerb } from '../src/ops/selection.ts'
import { budgetWords, dropOf, dropKey, type DragItem, type DropTarget } from '../src/ops/drag.ts'
import { boardRows, type OpsRow } from '../src/ops/board.ts'
import { queueOf } from '../src/ops/queue.ts'
import { PAST_WORDS } from '../src/ops/keys.ts'
import { board, confirm, NOW } from './ops-fixtures.ts'

const b = board()
const rows = boardRows({ ...b, now: NOW })
const row = (id: string) => rows.find((r) => r.id === id)!
const offer = (sel: OpsRow[], verb: BulkVerb, confirms = b.confirms) => bulkVerbs(sel, queueOf({ confirms, views: b.views }))!.find((o) => o.verb === verb)!

test('toggle, select all visible, clear', () => {
  let s = toggle(new Set(), 'ses_t2')
  assert.deepEqual([...s], ['ses_t2'])
  s = toggle(s, 'ses_tides')
  s = toggle(s, 'ses_t2')
  assert.deepEqual([...s], ['ses_tides'])
  assert.deepEqual([...selectAll(['a', 'b', 'a'])].sort(), ['a', 'b'])
  assert.equal(clear().size, 0)
})

test('each bulk verb counts the items that admit it, and says why when none does', () => {
  const sel = [row('ses_t2'), row('ses_t3'), row('ses_tides'), row('ses_t4'), row('wak_r1')].filter(Boolean)
  const pause = offer(sel, 'pause')
  assert.equal(pause.label, 'Pause 1 · resume 1', 'one working, one held')
  assert.deepEqual(pause.items.map((i) => i.method).sort(), ['task.pause', 'task.resume'])
  const cancel = offer(sel, 'cancel')
  assert.equal(cancel.label, 'Cancel 3', 'two living tasks and a wake; the ended task admits nothing')
  assert.deepEqual(cancel.items.map((i) => i.method).sort(), ['task.cancel', 'task.cancel', 'wake.cancel'])
  assert.ok(cancel.asks, 'a destructive verb asks once')
  const stop = offer(sel, 'stop')
  assert.equal(stop.label, 'Stop 1')
  assert.deepEqual(stop.items[0].params, { execution_id: 'exe_tides' })
  assert.equal(offer(sel, 'rehome').label, 'Re-home 2')
  assert.equal(offer(sel, 'attach').label, 'Attach 3 to project…', 'living tasks and conversations, as a drop takes them')
  const approve = offer(sel, 'approve')
  assert.equal(approve.label, 'Approve')
  assert.match(approve.reason!, /nothing selected waits on a tool approval/)
  assert.equal(offer([row('ses_t4')], 'pause').reason, 'nothing selected is a working task')
  for (const v of ['stop', 'cancel', 'rehome', 'decline'] as const) assert.ok(offer(sel, v).asks, v)
  for (const v of ['pause', 'attach'] as const) assert.ok(!offer(sel, v).asks, v)
})

test('decision 4: bulk Approve offers only tool approvals that are not of the always-ask kind, that this page may answer', () => {
  const confirms = [
    confirm('act_ok', { session_id: 'ses_t2' }),
    confirm('act_floor', { session_id: 'ses_t2', floor: true, tool: 'fs.write', reason: 'touches the daemon’s own state' }),
    confirm('act_budget', { session_id: 'ses_t2', tool: 'budget.reset', budget: { spent_usd: 2, limit_usd: 2 } as never }),
    confirm('act_change', { session_id: 'ses_t2', change: { task: 'tsk_a', title: 'a', field: 'objective', before: '', after: 'x' } as never }),
    confirm('act_elsewhere', { session_id: 'ses_t2', may_answer: false }),
    confirm('act_other', { session_id: 'ses_t3' }),
  ]
  const approve = offer([row('ses_t2')], 'approve', confirms)
  assert.deepEqual(approve.items.map((i) => i.item!.confirm!.correlation_id), ['act_ok'], 'not the floor, not a budget, not a change, not one for another connection, not another row’s')
  assert.deepEqual(approve.items[0].params, { question_id: 'act_ok', option: 'approve' })
  assert.equal(approve.items[0].method, 'work.answer')
  assert.deepEqual(approve.oneByOne!.map((i) => i.confirm!.correlation_id), ['act_floor'], 'the floor approval is listed as asking one by one')
  assert.equal(approve.label, 'Approve 1')
  const q = queueOf({ confirms, views: b.views })
  assert.deepEqual(q.filter(bulkApprovable).map((i) => i.confirm!.correlation_id).sort(), ['act_ok', 'act_other'])
  const decline = offer([row('ses_t2')], 'decline', confirms)
  assert.deepEqual(decline.items.map((i) => i.item!.confirm!.correlation_id).sort(), ['act_budget', 'act_change', 'act_floor', 'act_ok'], 'a decline is safe at the floor; never for another connection')
  assert.equal(decline.items[0].params!.option, 'decline')
})

test('the fan-out runs at most four at a time, in order, and one refusal never stops the others', async () => {
  const items = Array.from({ length: 11 }, (_, i) => ({ id: `ses_${i}`, key: `k${i}`, label: `item ${i}` }))
  let running = 0
  let most = 0
  const results = await fanOut(items, async (it) => {
    running++
    most = Math.max(most, running)
    await new Promise((r) => setTimeout(r, 2))
    running--
    if (it.key === 'k3') throw new Error('no such task: it ended')
    if (it.key === 'k10') throw { code: -32602, message: 'the home is a shared place' }
    return Number(it.key.slice(1)) % 5 === 0 ? { ok: false, words: 'the daemon refused: an ended task' } : { ok: true, words: 'done' }
  }, 4)
  assert.equal(most, 4)
  assert.deepEqual(results.map((r) => r.key), items.map((i) => i.key))
  assert.deepEqual(results.filter((r) => !r.ok).map((r) => r.key), ['k0', 'k3', 'k5', 'k10'])
  assert.equal(results[3].words, 'no such task: it ended', 'a thrown refusal in the daemon’s words')
  assert.equal(results[5].words, 'the daemon refused: an ended task')
  assert.equal(results[10].words, 'the home is a shared place', 'an RPC error object, as the client rejects with')
  const sel = new Set([...items.map((i) => i.id), 'ses_other'])
  assert.deepEqual([...afterRun(sel, results)].sort(), ['ses_0', 'ses_10', 'ses_3', 'ses_5'], 'the failed items stay selected, and only they')
  assert.equal(afterRun(sel, results.filter((r) => r.ok)).size, 0)
  assert.deepEqual(await fanOut([], async () => ({ ok: true, words: '' })), [])
})

const task: DragItem = { id: 'ses_t2', kind: 'task', title: 'Tabulate the tides', sessionId: 'ses_t2', ended: false, data: 'private', home: 'ses_home', homeTitle: 'Harbour chat' }
const conv: DropTarget = { kind: 'conversation', sessionId: 'ses_tides', title: 'Tide chat', data: 'private' }
const lane: DropTarget = { kind: 'lane', slug: 'tide-tables', title: 'Tide tables' }
const project: DropTarget = { kind: 'project', id: 'tsk_proj', title: 'Harbour wall' }

test('drag: a task onto a conversation or a lane re-homes it (asks, with the budget sentence); onto a project attaches', () => {
  assert.deepEqual(dropOf(task, conv, false), {
    method: 'task.rehome', params: { task: 'ses_t2', home: 'ses_tides' }, posture: 'ask',
    words: `Its report goes to the conversation “Tide chat” from now on. ${budgetWords('Harbour chat')}`,
  })
  assert.match(budgetWords('Harbour chat'), /^The budget stays charged to Harbour chat\.$/)
  const l = dropOf(task, lane, false)
  assert.ok('method' in l && l.params.home === 'thread:tide-tables' && l.posture === 'ask')
  assert.deepEqual(dropOf(task, project, false), { method: 'project.attach', params: { session: 'ses_t2', project: 'tsk_proj' }, posture: 'do', words: 'Attach “Tabulate the tides” to the project “Harbour wall”.' })
  const c: DragItem = { id: 'ses_tides', kind: 'conversation', title: 'Tide chat', sessionId: 'ses_tides', ended: false }
  assert.deepEqual(dropOf(c, project, false), { method: 'project.attach', params: { session: 'ses_tides', project: 'tsk_proj' }, posture: 'do', words: 'Attach “Tide chat” to the project “Harbour wall”.' })
  assert.equal(dropKey(conv), 'conversation:ses_tides')
  assert.equal(dropKey(lane), 'lane:tide-tables')
  assert.equal(dropKey(project), 'project:tsk_proj')
})

test('drag: every refusal, with its reason', () => {
  const refused = (s: Partial<DragItem>, t: DropTarget = conv, past = false) => { const d = dropOf({ ...task, ...s }, t, past); return 'refused' in d ? d.refused : null }
  assert.equal(refused({}, conv, true), PAST_WORDS, 'anything under the time machine')
  assert.equal(refused({}, project, true), PAST_WORDS)
  assert.match(refused({ ended: true })!, /ended/)
  assert.match(refused({ ended: true }, project)!, /ended/)
  assert.match(refused({ kind: 'job' })!, /job/)
  assert.match(refused({ kind: 'question' })!, /question/)
  assert.match(refused({ kind: 'system' })!, /system row/)
  assert.match(refused({ kind: 'routine' })!, /routine run/)
  assert.match(refused({ kind: 'wake' })!, /wake is not a task/)
  assert.match(refused({ kind: 'step' })!, /step/)
  assert.match(refused({ kind: 'done' })!, /fold/)
  assert.match(refused({ kind: 'conversation', data: undefined })!, /a conversation has no parent/, 'a running conversation onto a home')
  assert.match(refused({ kind: 'conversation' }, lane)!, /no parent/)
  assert.equal(refused({ data: 'private' }, { ...conv, data: 'open' }), 'a private task cannot report into an open conversation', 'a home weaker in data mark')
  assert.equal(refused({ data: 'sensitive' }, { ...lane, data: 'private' }), 'a sensitive task cannot report into a private thread')
  assert.equal(refused({ data: 'private' }, { ...conv, data: 'sensitive' }), null, 'a stronger home is fine')
  assert.equal(refused({ data: 'private' }, { ...conv, data: undefined }), null, 'a mark the page cannot tell: the daemon decides')
  assert.match(refused({ home: 'ses_tides' })!, /already reports there/)
  assert.match(refused({ sessionId: 'ses_tides' })!, /itself/)
  assert.match(refused({ sessionId: undefined })!, /no session/)
})
