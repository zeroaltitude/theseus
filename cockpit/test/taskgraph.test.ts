// The task graph's pure parts (`src/lib/taskgraph.ts`, M7 39b), run by `npm test`: the tree's order, a claim while it
// holds, a waiting change in the card's words, and the graph's layout. Invented ids.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { TaskRecord } from '@protocol'
import { cardOf, changeQuestion, changeWords, claimWords, layout, taskTree } from '../src/lib/taskgraph.ts'

function task(id: string, at: number, more: Partial<TaskRecord> = {}): TaskRecord {
  return {
    id, version: 1, title: `the ${id} part`, objective: '', acceptance: [], state: 'accepted', deps: [], owner: 'agent',
    origin: { session: 'ses_0000lagoon', principal: 'operator' }, evidence: [], created_at_ms: at, updated_at_ms: at,
    ...more,
  }
}

test('the tree puts each child under its parent, oldest first, and an orphan is a root', () => {
  const rows = taskTree([
    task('tsk_south', 4, { parent: 'tsk_reef' }),
    task('tsk_reef', 1),
    task('tsk_north', 3, { parent: 'tsk_reef' }),
    task('tsk_buoy', 2),
    task('tsk_orphan', 5, { parent: 'tsk_gone' }),
  ])
  assert.deepEqual(rows.map((r) => [r.t.id, r.depth]), [
    ['tsk_reef', 0], ['tsk_north', 1], ['tsk_south', 1], ['tsk_buoy', 0], ['tsk_orphan', 0],
  ])
})

test('a claim shows while it holds, and a waiting change asks as its card does', () => {
  const t = task('tsk_000000reef', 1, { claim: { by: 'exe_0000lagoon', session: 'ses_0000lagoon', until_ms: 1_800_000 } })
  assert.equal(claimWords(t, 60_000), 'claimed by session lagoon · 29m left')
  assert.equal(claimWords(t, 1_800_000), null)
  t.proposal = { acceptance: ['every marker has a depth'], abandon: false, by: 'ses_0000lagoon', card: 'act_0000card', base_version: 1, at_ms: 2 }
  assert.equal(changeWords(t), 'Change the acceptance of tsk_000000reef (the tsk_000000reef part)?')
  const card = { correlation_id: 'act_0000card' } as Parameters<typeof cardOf>[1][number]
  assert.equal(cardOf(t, [card]), card)
  assert.equal(cardOf(t, []), undefined)
})

test('the layout puts a task at its depth, with edges to its children and its deps', () => {
  const { nodes, links } = layout([
    task('tsk_reef', 1),
    task('tsk_north', 2, { parent: 'tsk_reef' }),
    task('tsk_buoy', 3, { deps: ['tsk_north'] }),
  ], 100, 10)
  assert.deepEqual(nodes.map((n) => [n.id, n.x, n.y]), [['tsk_reef', 0, 0], ['tsk_north', 100, 10], ['tsk_buoy', 0, 20]])
  assert.deepEqual(links.map((l) => [l.source, l.target, l.dep]), [['tsk_reef', 'tsk_north', false], ['tsk_buoy', 'tsk_north', true]])
})

test("a question's change reads as theseus-protocol's TaskChange::question says it", () => {
  const c = { task: 'tsk_0000reef', title: 'Chart the reef', field: 'acceptance', before: 'every marker has a depth', after: 'every marker has a depth; the chart is signed' }
  assert.equal(changeQuestion(c), 'Change the acceptance of tsk_0000reef (Chart the reef)? Before: every marker has a depth After: every marker has a depth; the chart is signed')
  assert.equal(changeQuestion({ ...c, field: 'abandon', before: 'accepted', after: 'abandoned' }), 'Abandon tsk_0000reef (Chart the reef)? Before: accepted After: abandoned')
  assert.ok(changeQuestion({ ...c, before: '' }).includes('Before: (none)'))
})
