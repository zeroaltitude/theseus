// The work board on the page (`src/lib/workboard.ts`), run by `npm test`: the position rule on a snapshot and on
// changes, and a snapshot after a lost event as the truth for what it names.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { WorkChanged, WorkView, WorkWatchResult } from '../src/protocol.gen/index.ts'
import { applyChanged, applySnapshot, EMPTY } from '../src/lib/workboard.ts'

const NOW = 10_000_000

function view(id: string, kind: WorkView['kind'], state: WorkView['state'], parent?: string, position = 1): WorkView {
  const level = state === 'needs_you' || state === 'failed' ? 'needs_you' : state === 'running' || state === 'queued' || state === 'waiting' ? 'working' : state === 'ready' ? 'ready' : 'idle'
  return {
    id, kind, position, at_ms: NOW - 60_000, parent, root: parent ?? id, title: id, state,
    attention: { level, label: state, since_ms: 1 }, started_at_ms: 1, state_since_ms: 1,
  } as WorkView
}

function board(): WorkView[] {
  return [
    view('exe_pr', 'conversation', 'ready'),
    view('exe_pr_t', 'task', 'running', 'exe_pr'),
    view('act_pr_q', 'question', 'needs_you', 'exe_pr'),
    view('exe_dm', 'conversation', 'running'),
    view('exe_dm_t', 'task', 'running', 'exe_dm'),
  ]
}

test('a snapshot and changes apply by the position rule', () => {
  const snap = { position: 10, views: board().map((v, i) => ({ ...v, position: 5 + i })), total: 5 } as WorkWatchResult
  let s = applySnapshot(EMPTY, snap)
  assert.equal(Object.keys(s.views).length, 5)
  const old = { ...view('exe_pr_t', 'task', 'needs_you', 'exe_pr'), position: 2 }
  const upd = { position: 11, at_ms: NOW, views: [old], gone: ['exe_dm_t'] } as WorkChanged
  s = applyChanged(s, upd)
  assert.equal(s.views.exe_pr_t!.state, 'running', 'an older view is not applied')
  assert.ok(!('exe_dm_t' in s.views))
  assert.equal(s.position, 11)
  const newer = { ...view('exe_pr_t', 'task', 'done', 'exe_pr'), position: 12 }
  s = applyChanged(s, { position: 12, at_ms: NOW, views: [newer] } as WorkChanged)
  assert.equal(s.views.exe_pr_t!.state, 'done', 'a newer view is')
  // The snapshot after a lost event is the truth for what it names, and drops what it lacks.
  const again = applySnapshot(s, { position: 20, views: [], total: 0 } as WorkWatchResult)
  assert.equal(Object.keys(again.views).length, 0)
  assert.equal(again.position, 20)
})

test('a view newer than the snapshot survives it', () => {
  const s = applyChanged(EMPTY, { position: 30, at_ms: NOW, views: [{ ...view('exe_new', 'task', 'running'), position: 30 }] } as WorkChanged)
  const after = applySnapshot(s, { position: 20, views: [], total: 0 } as WorkWatchResult)
  assert.ok('exe_new' in after.views, 'its position is past the snapshot’s')
  assert.equal(after.position, 30)
})
