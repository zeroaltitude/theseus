// A project's panel lists the tasks involved in it (GitHub issue 37, line 7): running first, each opening the task, bounded
// by the daemon with a count of the rest. React is not rendered, so the rows are a pure function and the sources are
// read as text, as `simple-ops-horizon.test.ts` does.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { ProjectTask } from '../src/protocol.gen/index.ts'
import { taskHint, taskRows } from '../src/simple/ops/projectTasks.ts'
import { HOUR, NOW } from './ops-fixtures.ts'

const src = (f: string) => readFileSync(new URL(`../src/${f}`, import.meta.url), 'utf8')
const DAY = 24 * HOUR

const running: ProjectTask = { task_id: 'ses_run001', short: 'run001', title: 'Sound the channel', state: 'running', ended: false, how: 'started for it', started_at_ms: NOW - 2 * HOUR }
const ended: ProjectTask = { task_id: 'ses_end002', short: 'end002', title: 'Weekly review', state: 'complete', ended: true, how: 'weekly review', started_at_ms: NOW - 9 * DAY, ended_at_ms: NOW - 9 * DAY + HOUR }

test('each task is a row: its title, its state, when it started and ended, and how it was involved', () => {
  const rows = taskRows([running, ended], NOW)
  assert.deepEqual(rows.map((r) => r.id), ['ses_run001', 'ses_end002'])
  assert.equal(rows[0].sub, 'running · started 2h ago · started for it')
  assert.equal(rows[0].running, true)
  assert.equal(rows[1].sub, 'complete · started 9d ago · ended 8d ago · weekly review')
  assert.equal(rows[1].running, false)
  assert.equal(taskRows([{ ...running, title: '' }], NOW)[0].title, 'run001', 'an untitled task reads by its short id')
})

test('the hint counts the tasks and the running ones', () => {
  assert.equal(taskHint(0, 0), 'none yet')
  assert.equal(taskHint(1, 0), '1 task')
  assert.equal(taskHint(7, 1), '7 tasks, 1 running')
})

test('the project’s panel draws the section after the Horizon, each row selecting the task, and nothing acts or polls', () => {
  const inspector = src('ops/Inspector.tsx')
  assert.match(inspector, /\{proj && <Horizon project=\{proj\.id\} now=\{now\} onPick=\{select\} \/>\}\n\s*\{proj && <ProjectTasks project=\{proj\.id\} now=\{now\} onPick=\{select\} \/>\}/)
  const s = src('simple/ops/ProjectTasks.tsx')
  const code = s.replace(/\/\/.*$/gm, '')
  assert.match(s, /useRpc<ProjectShowResult>\('project\.show', \{ id: project \}, 0\)/)
  assert.match(s, /onClick=\{\(\) => onPick\(t\.id\)\}/)
  assert.match(s, /data\.tasks_more/)
  assert.ok(!/setInterval|refetchInterval|\bcall\(|useOpsAct/.test(code), 'it only reads')
})
