// A long-horizon task on both Operations pages (GitHub issue 17, "horizon task management"): its plan, its next steps and
// what it waits on are read from the project's records, and the two changes are the daemon's own calls
// (`project.item.add`, `project.item.close`, issue 13). React is not rendered here, so the reading and the calls are
// tested as pure functions, and the two pages' sources are read as text to hold that both draw the one section, that
// each change goes through the gate that is off under the time machine, and that nothing polls.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { ProjectInfo, TaskRecord } from '../src/protocol.gen/index.ts'
import { ACTS, enabled } from '../src/ops/keys.ts'
import { addItemCall, closeItemCall, horizonOf, itemRefusal, ITEM_MAX, projectIdOf } from '../src/simple/ops/horizon.ts'
import { HOUR, NOW, record } from './ops-fixtures.ts'

const src = (f: string) => readFileSync(new URL(`../src/${f}`, import.meta.url), 'utf8')
const DAY = 24 * HOUR

const project: ProjectInfo = {
  id: 'tsk_proj', title: 'Chart the harbour', objective: 'a chart every pilot can trust', acceptance: ['every pier is on it'], state: 'accepted',
  progress: { steps_done: 1, steps_total: 4, milestones_done: 1, milestones_total: 3, overdue: 1 },
}
const records: TaskRecord[] = [
  record('tsk_proj', { title: 'Chart the harbour', kind: 'project', state: 'accepted' }),
  record('tsk_m1', { title: 'North quays charted', kind: 'milestone', parent: 'tsk_proj', state: 'done', due_at_ms: NOW - 9 * DAY }),
  record('tsk_m2', { title: 'South quays charted', kind: 'milestone', parent: 'tsk_proj', state: 'accepted', due_at_ms: NOW - 2 * DAY }),
  record('tsk_m3', { title: 'Chart printed', kind: 'milestone', parent: 'tsk_proj', state: 'accepted', due_at_ms: NOW + 20 * DAY }),
  record('tsk_i1', { title: 'Measure pier four', parent: 'tsk_proj', state: 'in_progress' }),
  record('tsk_i2', { title: 'Sound the channel', kind: 'item', parent: 'tsk_proj', state: 'accepted' }),
  record('tsk_i3', { title: 'Get the harbour master’s permit', kind: 'item', parent: 'tsk_proj', state: 'waiting_human' }),
  record('tsk_i4', { title: 'Print the chart', kind: 'item', parent: 'tsk_proj', state: 'accepted', deps: ['tsk_i3'] }),
  record('tsk_i5', { title: 'Old survey', kind: 'item', parent: 'tsk_proj', state: 'done' }),
]

test('the plan is the milestones, soonest first and the closed last, an overdue one marked', () => {
  const h = horizonOf(project, records, NOW)
  assert.deepEqual(h.plan.map((m) => [m.id, m.overdue]), [['tsk_m2', true], ['tsk_m3', false], ['tsk_m1', false]])
  assert.equal(h.plan[0].word, 'not started')
  assert.deepEqual([h.title, h.objective, h.acceptance], ['Chart the harbour', 'a chart every pilot can trust', ['every pier is on it']])
  assert.deepEqual([h.done, h.total], [1, 3], 'milestones done of total, the daemon’s own count, as its bar reads it')
})

test('next steps are the open steps no one has begun; doing is what is being worked; a closed one is neither', () => {
  const h = horizonOf(project, records, NOW)
  assert.deepEqual(h.next.map((s) => s.id), ['tsk_i2', 'tsk_i4'])
  assert.deepEqual(h.doing.map((s) => s.id), ['tsk_i1'])
  for (const id of ['tsk_i5', 'tsk_m1', 'tsk_proj']) assert.ok(![...h.next, ...h.doing].some((s) => s.id === id), `${id} is not a next step`)
  assert.ok(h.next.every((s) => s.item), 'items can be closed')
  assert.equal(horizonOf({ ...project, progress: { ...project.progress, milestones_total: 0, milestones_done: 0 } }, records, NOW).total, 4, 'with no milestones, the steps are counted')
})

test('what it waits on: a person, another step, a change not answered, a date that has passed, each with its reason', () => {
  const withChange = records.map((r) => (r.id === 'tsk_i2' ? { ...r, proposal: { objective: 'sound it twice', by: 'ses_x', card: 'act_p', base_version: 1, at_ms: NOW } as never } : r))
  const h = horizonOf(project, withChange, NOW)
  const why = Object.fromEntries(h.waiting.map((w) => [w.id, w.why]))
  assert.equal(why.tsk_i3, 'it waits for a person')
  assert.equal(why.tsk_i4, 'it waits on Get the harbour master’s permit')
  assert.equal(why.tsk_i2, 'a change to it waits for a yes')
  assert.equal(why.tsk_m2, 'its date has passed')
  assert.ok(!('tsk_m3' in why) && !('tsk_i1' in why) && !('tsk_i5' in why), 'what is on track is not waiting')
  assert.deepEqual(horizonOf(project, records.filter((r) => !['tsk_i3', 'tsk_i4', 'tsk_m2'].includes(r.id)), NOW).waiting, [], 'nothing waits when nothing is stuck')
})

test('an item is one line: refused empty and past the daemon’s limit, with no call; the calls are the daemon’s', () => {
  assert.equal(itemRefusal('   '), 'Write the step first.')
  assert.equal(itemRefusal('x'.repeat(ITEM_MAX)), null)
  assert.match(itemRefusal('x'.repeat(ITEM_MAX + 1))!, /at most 200 characters/)
  assert.deepEqual(addItemCall('tsk_proj', '  Sound the channel '), { method: 'project.item.add', params: { project: 'tsk_proj', title: 'Sound the channel' } })
  assert.deepEqual(closeItemCall('tsk_i2'), { method: 'project.item.close', params: { item: 'tsk_i2' } })
  assert.equal(projectIdOf({ record: { id: 'tsk_proj', kind: 'project' } }), 'tsk_proj')
  assert.equal(projectIdOf({ record: { id: 'tsk_i1', kind: undefined } }), null)
})

test('both changes are acts: off under the time machine, like every act on the Operations page', () => {
  for (const v of ['project_item_add', 'project_item_close']) {
    assert.ok(ACTS.has(v), v)
    assert.equal(enabled(v as never, true), false, `${v} in the past`)
    assert.equal(enabled(v as never, false), true, `${v} live`)
  }
})

test('both pages draw the one section: it is the Inspector’s, which the Ship’s page and the simple page both draw', () => {
  const inspector = src('ops/Inspector.tsx')
  assert.match(inspector, /import \{ Horizon \} from '\.\.\/simple\/ops\/Horizon'/)
  assert.match(inspector, /\{proj && <Horizon project=\{proj\.id\} now=\{now\} onPick=\{select\} \/>\}/)
  assert.match(inspector, /r\.record\?\.kind === 'project'/)
  // The Ship's page draws the Inspector for a selected row, and so does the simple page's Details.
  assert.match(src('views/Ops.tsx'), /<Inspector sel=\{selection\}/)
  assert.match(src('simple/Ops.tsx'), /<Inspector sel=\{\{ kind: 'row', row \}\}/)
  // `StepWhole` is the body of a plan or a step, which is what a project's row is.
  assert.match(inspector, /bodyOf\(sel\.row\) === 'step'\) return <StepWhole/)
})

test('the section reads on pushes and acts through the gate: no timer, post through useOpsAct, the list read again', () => {
  const s = src('simple/ops/Horizon.tsx')
  const code = s.replace(/\/\/.*$/gm, '')
  assert.ok(!/setInterval|refetchInterval/.test(code), 'polls')
  assert.match(s, /useRpc<ProjectShowResult>\('project\.show', \{ id: project \}, 0\)/)
  assert.match(s, /\['task\.changed', 'work\.changed'\]\.includes\(last\.method\)/)
  assert.match(s, /const \{ post, on \} = useOpsAct\(\)/)
  assert.match(s, /await post\(verb, call\.method, call\.params\)/)
  assert.ok(!/\bcall\(/.test(code), 'no direct call around the gate')
  assert.match(s, /help="simple\.ops\.horizon"|<HelpButton id="simple\.ops\.horizon"/)
})
