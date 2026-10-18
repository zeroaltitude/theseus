// The Tasks page's rules (theseus-ypnxt.6): the three sections, the search, the header's count, a place's name.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { TaskInfo, WorkView } from '../src/protocol.gen/index.ts'
import { noticeTarget, placeName, reportLineOf, rowsOf, split, workingCount } from '../src/lib/taskboard.ts'

const view = (n: number, state: string, extra: object = {}, level = 'working'): WorkView => ({
  id: `exe_${n}`, kind: 'task', position: n, at_ms: n * 1000, root: 'ses_r', session_id: `ses_t${n}`, title: `Task ${n}`, state,
  attention: { level, label: '', since_ms: 0 }, started_at_ms: 1, state_since_ms: 1, ...extra,
}) as unknown as WorkView
const info = (n: number, state: string, extra: object = {}): TaskInfo => ({
  task_id: `ses_t${n}`, short: `t${n}`.padStart(6, '0'), state, spent_usd: 0, limit_usd: 1, pending_confirms: 0, updated_at_ms: n * 1000, ...extra,
}) as unknown as TaskInfo

// Six tasks: one asks, one runs, one waits on its command, one is paused, one reported (done), one failed.
const views = [
  view(1, 'needs_you', {}, 'needs_you'),
  view(2, 'running'),
  view(3, 'waiting'),
  view(4, 'running', { paused: true }),
  view(5, 'done', { title: 'Harbour survey', report: { outcome: 'done', first_line: 'Mapped the quay walls' } }, 'idle'),
  view(6, 'failed', { report: { outcome: 'failed', reason: 'the provider refused the request' } }, 'idle'),
]
const tasks = [1, 2, 3, 4, 5, 6].map((n) => info(n, n === 5 ? 'complete' : n === 6 ? 'failed' : 'running'))

test('the sections: needs you, working (running, waiting, paused), done', () => {
  const s = split(rowsOf(views, tasks))
  assert.deepEqual(s.needs.map((r) => r.id), ['ses_t1'])
  assert.deepEqual(s.working.map((r) => r.id), ['ses_t4', 'ses_t3', 'ses_t2'], 'newest activity first')
  assert.deepEqual(s.done.map((r) => r.id), ['ses_t6', 'ses_t5'])
  assert.equal(s.working.find((r) => r.id === 'ses_t4')?.paused, true)
})

test('a task with only its record is placed by its state', () => {
  const s = split(rowsOf([], [info(7, 'running'), info(8, 'cancelled'), info(9, 'waiting', { pending_confirms: 1 })]))
  assert.deepEqual([s.working.length, s.done.length, s.needs.length], [1, 1, 1])
})

test('search runs over title and report line, every word, in any case', () => {
  const rows = rowsOf(views, tasks)
  assert.deepEqual(split(rows, 'harbour').done.map((r) => r.id), ['ses_t5'])
  assert.deepEqual(split(rows, 'QUAY walls').done.map((r) => r.id), ['ses_t5'])
  assert.deepEqual(split(rows, 'refused').done.map((r) => r.id), ['ses_t6'])
  const none = split(rows, 'harbour refused')
  assert.equal(none.done.length + none.working.length + none.needs.length, 0)
  assert.equal(split(rows, '  ').working.length, 3)
})

test('the header counts the tasks working, a waiting one included, a paused one too, an ended one not', () => {
  assert.equal(workingCount([view(1, 'waiting')]), 1)
  assert.equal(workingCount(views), 3)
  assert.equal(workingCount([view(1, 'running'), { ...view(2, 'running'), kind: 'conversation' } as WorkView]), 1)
})

test('a place has a name; the raw target is the tooltip', () => {
  assert.equal(placeName('discord:dm:112233'), 'discord dm')
  assert.equal(placeName('discord:channel:99'), 'discord channel')
  assert.equal(placeName('cli'), 'cli')
  assert.equal(placeName(undefined), undefined)
  const [r] = rowsOf([], [info(1, 'running', { target: 'discord:dm:112233' })])
  assert.equal(r.place, 'discord dm')
  assert.equal(r.target, 'discord:dm:112233')
  assert.equal(rowsOf([view(1, 'running', { place: 'web' })], [info(1, 'running', { target: 'discord:dm:1' })])[0].place, 'web')
})

test('a notice opens its task on the page, other work in its session', () => {
  assert.equal(noticeTarget({ id: 'exe_1', kind: 'task', session_id: 'ses_t1' }), '/tasks?sel=ses_t1')
  assert.equal(noticeTarget({ id: 'wak_1', kind: 'wake', session_id: 'ses_a' }), '/session/ses_a')
  assert.equal(noticeTarget({ id: 'act_1', kind: 'job' }), undefined)
})

// The wiring is React, which this runner does not render: the sources are read as text, as test/navgroups.test.ts does.
const src = (f: string) => readFileSync(new URL(`../src/${f}`, import.meta.url), 'utf8')

test('the page is routed and in the rail, the header counts tasks, Actions links to the page, and nothing polls', () => {
  assert.match(src('main.tsx'), /path: 'tasks'.*views\/Tasks/)
  assert.match(src('components/Shell.tsx'), /to: '\/tasks', label: 'Tasks'/)
  assert.match(src('components/Shell.tsx'), /t: '\/tasks'/)
  assert.match(src('components/Heartbeat.tsx'), /workingCount\(/)
  assert.match(src('components/Heartbeat.tsx'), /to="\/tasks"/)
  assert.match(src('views/Actions.tsx'), /to="\/tasks"/)
  assert.match(src('components/DesktopNotices.tsx'), /noticeTarget\(/)
  assert.doesNotMatch(src('views/Tasks.tsx'), /setInterval|refetchInterval/, 'the board and the push keep it fresh')
  assert.match(src('components/TaskHeader.tsx'), /progressLine\(/)
})

test('a report line the board no longer holds is read from the session’s last assistant message', () => {
  const nodes = [{ kind: 'user_message', text: '[Task a1b2c3: …]' }, { kind: 'assistant_message', text: '\n  Mapped the quay walls.\nThen more.' }]
  assert.equal(reportLineOf(nodes), 'Mapped the quay walls.')
  assert.equal(reportLineOf([nodes[0]]), undefined)
  assert.equal(reportLineOf(undefined), undefined)
  const [r] = rowsOf([], [info(1, 'complete', { ended_reason: 'reported' })], { ses_t1: 'Mapped the quay walls.' })
  assert.equal(r.reportLine, 'Mapped the quay walls.')
  assert.deepEqual(split([r], 'quay').done.length, 1)
  assert.equal(rowsOf([], [info(1, 'complete', { ended_reason: 'reported' })])[0].reportLine, 'reported')
})
