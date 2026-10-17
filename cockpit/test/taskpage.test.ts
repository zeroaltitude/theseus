import { test } from 'node:test'
import assert from 'node:assert/strict'
import { composerPlan, joinable, taskContext, taskRedirectLine, viewOfTask } from '../src/lib/taskpage.ts'

const task = { session_id: 'ses_task0000000000a1b2c3', kind: 'task', parent_session_id: 'ses_main0000000000d4e5f6', title: 'Sound the north channel', label: null, execution_state: 'running' } as never
const talk = { session_id: 'ses_main0000000000d4e5f6', kind: 'conversation', title: 'Main', label: null, execution_state: 'idle' } as never

test('the task header is for a task session only', () => {
  assert.equal(taskContext(talk), null)
  assert.equal(taskContext({ ...(talk as object), parent_session_id: 'ses_other' } as never), null)
  assert.deepEqual(taskContext(task), { taskSession: 'ses_task0000000000a1b2c3', parent: 'ses_main0000000000d4e5f6', title: 'Sound the north channel', ended: false })
  // A task with no parent has nowhere to return to.
  assert.equal(taskContext({ ...(task as object), parent_session_id: undefined } as never), null)
})

test('back goes to the parent conversation', () => {
  assert.equal(taskContext(task)?.parent, 'ses_main0000000000d4e5f6')
})

test('joinable is the protocol\'s: running, queued, waiting, needs you', () => {
  for (const [state, yes] of [['running', true], ['queued', true], ['waiting', true], ['needs_you', true], ['done', false], ['failed', false], ['cancelled', false], ['expired', false]] as const) {
    assert.equal(joinable({ state }), yes, state)
  }
})

test('the work view decides whether the task has ended', () => {
  assert.equal(taskContext(task, { state: 'done' })?.ended, true)
  assert.equal(taskContext(task, { state: 'needs_you' })?.ended, false)
  assert.equal(taskContext({ ...(task as object), execution_state: 'done' } as never)?.ended, true)
})

test('a running task\'s composer posts to the task, as typed', () => {
  const p = composerPlan('ses_task0000000000a1b2c3', taskContext(task), 'go slower')
  assert.equal(p.sessionId, 'ses_task0000000000a1b2c3')
  assert.equal(p.input, 'go slower')
  assert.match(p.placeholder ?? '', /Message this task; it reads it at its next step/)
})

test('an ended task\'s composer posts to the parent with the shared line', () => {
  const p = composerPlan('ses_task0000000000a1b2c3', taskContext(task, { state: 'done' }), 'one more thing')
  assert.equal(p.sessionId, 'ses_main0000000000d4e5f6')
  assert.equal(p.input, taskRedirectLine('Sound the north channel', 'a1b2c3', 'the task has reported, so this goes to main') + 'one more thing')
  assert.match(p.input, /^\[Written to the task “Sound the north channel” \(…a1b2c3\), and /)
  assert.match(p.placeholder ?? '', /Send to main about this task/)
})

test('a conversation\'s composer is untouched', () => {
  assert.deepEqual(composerPlan('ses_main0000000000d4e5f6', null, 'hi'), { sessionId: 'ses_main0000000000d4e5f6', input: 'hi', placeholder: null })
})

test('a task session is found among the board\'s views', () => {
  const v = (id: string, kind: string, session_id: string) => ({ id, kind, session_id, state: 'running' }) as never
  const views = { a: v('a', 'conversation', 'ses_x'), b: v('b', 'task', 'ses_task0000000000a1b2c3') }
  assert.equal(viewOfTask(views, 'ses_task0000000000a1b2c3')?.id, 'b')
  assert.equal(viewOfTask(views, 'ses_x'), undefined)
})
