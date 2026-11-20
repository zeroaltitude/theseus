// Planned work can be managed (GitHub issue 37, line 6): the self loop's container is kept out of the lists of plans, a
// project can be closed from its panel (one `project.close`, two answers, its counts said first), and every row under
// Planned and Paused says how old it is and when it last changed. React is not rendered here, so the rules are pure
// functions and the page sources are read as text, as `simple-ops-horizon.test.ts` does.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { ProjectCloseResult, ProjectInfo } from '../src/protocol.gen/index.ts'
import { ACTS, enabled } from '../src/ops/keys.ts'
import { boardRows } from '../src/ops/board.ts'
import { canCloseProject, lifeWords } from '../src/ops/facts.ts'
import { closeCounts, closedWords, closeWords } from '../src/ops/projectClose.ts'
import { plannedRecords } from '../src/lib/taskboard.ts'
import { HOUR, info, NOW, record } from './ops-fixtures.ts'

const src = (f: string) => readFileSync(new URL(`../src/${f}`, import.meta.url), 'utf8')
const DAY = 24 * HOUR

const rowOf = (rec: ReturnType<typeof record>) => boardRows({ views: [], tasks: [], sessions: [], records: [rec], now: NOW, past: false }).find((r) => r.id === rec.id)!

test('a row says how old it is and when it last changed, in the page’s relative style', () => {
  const old = rowOf(record('tsk_old', { created_at_ms: NOW - 32 * DAY, updated_at_ms: NOW - 19 * DAY }))
  assert.equal(lifeWords(old, NOW), 'made 32d ago · last changed 19d ago')
  const untouched = rowOf(record('tsk_still', { created_at_ms: NOW - 32 * DAY, updated_at_ms: NOW - 32 * DAY }))
  assert.equal(lifeWords(untouched, NOW), 'made 32d ago, never changed')
})

test('a paused task row says it too, from its task’s own times', () => {
  const rows = boardRows({ views: [], tasks: [info(1, 'cancelled', { created_at_ms: NOW - 33 * DAY, updated_at_ms: NOW - 33 * DAY + HOUR })], sessions: [], records: [], now: NOW, past: false })
  assert.equal(lifeWords(rows[0], NOW), 'made 33d ago · last changed 32d ago')
})

test('the Planned box and the panel draw it: every row of Next steps, On the clock and Paused, and a step’s panel', () => {
  const page = src('simple/Ops.tsx')
  assert.match(page, /return \[line, lifeWords\(r, now\)\]\.filter\(Boolean\)\.join\(' · '\)/)
  assert.equal((page.match(/onPick=\{onPick\} aged \/>/g) ?? []).length, 2, 'scheduled and paused rows are aged')
  assert.match(src('ops/Inspector.tsx'), /const life = lifeWords\(r, now\)/)
  assert.match(src('ops/Inspector.tsx'), /data-testid="inspector-life"/)
})

test('the daemon keeps the self loop’s container out of the plans: the lists read `apart`, the record stays for its items', () => {
  const box = record('tsk_loopbox', { title: 'Self-improvement backlog' })
  const item = record('tsk_loopitem', { parent: 'tsk_loopbox' })
  const plan = record('tsk_plan', {})
  const all = [box, item, plan]
  assert.deepEqual(plannedRecords({ records: all, apart: ['tsk_loopbox'] }).map((r) => r.id), ['tsk_loopitem', 'tsk_plan'])
  assert.equal(plannedRecords({ records: all }), all, 'nothing apart: the same list')
  assert.deepEqual(plannedRecords(undefined), [])
  // Both pages read the list through it, and nothing guesses from a title.
  assert.match(src('simple/Ops.tsx'), /plannedRecords\(tl\)/)
  assert.match(src('views/Ops.tsx'), /plannedRecords\(tl\)/)
  assert.ok(!/Self-improvement backlog/.test(src('lib/taskboard.ts').replace(/\/\/.*$/gm, '').replace(/\/\*[\s\S]*?\*\//g, '')))
  // With it left out, the planned count says 0 where it said 1.
  const rows = boardRows({ views: [], tasks: [], sessions: [], records: [...plannedRecords({ records: [box], apart: ['tsk_loopbox'] })], now: NOW, past: false })
  assert.equal(rows.filter((r) => r.kind === 'step').length, 0)
})

test('the button is on an open project’s panel and not on a closed one, a plan or a step', () => {
  assert.equal(canCloseProject(record('tsk_p', { kind: 'project', state: 'accepted' })), true)
  assert.equal(canCloseProject(record('tsk_p', { kind: 'project', state: 'in_progress' })), true)
  for (const state of ['done', 'failed', 'abandoned']) assert.equal(canCloseProject(record('tsk_p', { kind: 'project', state })), false, state)
  assert.equal(canCloseProject(record('tsk_i', { kind: 'item', state: 'accepted' })), false)
  assert.equal(canCloseProject(record('tsk_i', { state: 'accepted' })), false)
  assert.equal(canCloseProject(undefined), false)
  assert.match(src('ops/Inspector.tsx'), /\{proj && canCloseProject\(proj\) && /)
  assert.match(src('ops/Inspector.tsx'), /Close project…/)
})

test('the sheet counts what will end and what will stop, as the terminal does', () => {
  const recs = [
    record('tsk_p', { kind: 'project', state: 'accepted' }),
    record('tsk_m1', { kind: 'milestone', parent: 'tsk_p', state: 'accepted', due_at_ms: NOW + DAY }),
    record('tsk_m2', { kind: 'milestone', parent: 'tsk_p', state: 'accepted' }),
    record('tsk_m3', { kind: 'milestone', parent: 'tsk_p', state: 'done', due_at_ms: NOW - DAY }),
    record('tsk_i1', { parent: 'tsk_p', state: 'accepted' }),
    record('tsk_i2', { kind: 'item', parent: 'tsk_p', state: 'in_progress' }),
    record('tsk_i3', { kind: 'item', parent: 'tsk_p', state: 'abandoned' }),
  ]
  assert.deepEqual(closeCounts(recs), { items: 2, milestones: 2, dated: 1 })
  assert.equal(closeWords(closeCounts(recs), true), 'Ends 2 open items and 2 open milestones and stops its weekly review and the reminders of 1 dated milestone. It does not stop a task that carries part of the project.')
  assert.equal(closeWords({ items: 1, milestones: 0, dated: 0 }, false), 'Ends 1 open item and 0 open milestones. It does not stop a task that carries part of the project.')
})

test('the answer is said in counts and kinds, never the project’s words', () => {
  const r = { project: { id: 'tsk_p' } as ProjectInfo, how: 'dropped', items_ended: 2, milestones_ended: 1, review_stopped: 'ses_rev', reminders_cancelled: 2 } as ProjectCloseResult
  assert.equal(closedWords(r), 'Project dropped: 2 items and 1 milestone ended; its weekly review and 2 reminders stopped.')
  assert.equal(closedWords({ ...r, how: 'done', review_stopped: undefined, reminders_cancelled: 0 }), 'Project marked done: 2 items and 1 milestone ended.')
})

test('closing is an act: off under the time machine, one project.close through the page’s plumbing, two answers', () => {
  assert.ok(ACTS.has('project_close'))
  assert.equal(enabled('project_close' as never, true), false)
  assert.equal(enabled('project_close' as never, false), true)
  const s = src('ops/CloseProjectSheet.tsx')
  const code = s.replace(/\/\/.*$/gm, '')
  assert.match(s, /post<ProjectCloseResult>\('project_close', 'project\.close', \{ project, how/)
  assert.ok(!/\bcall\(/.test(code), 'no direct call around the gate')
  assert.match(s, /submit="Mark done"/)
  assert.match(s, />Drop<\/ActBtn>/)
  assert.match(s, /void go\('done'\)/)
  assert.match(s, /void go\('dropped'\)/)
  assert.match(s, /result=\{refused \? \{ ok: false, text: refused \} : null\}/)
})
