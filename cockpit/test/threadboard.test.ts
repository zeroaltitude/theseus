// Threads on the work board (`src/lib/threadboard.ts`, theseus-5i8i), run by `npm test`: the grouping the Rust
// `theseus_protocol::group` makes, line for line (items under their thread, loose work left alone, a question counted
// once, a pending seal counted, a paused thread out of the board's count, a done thread only while open, the lock on a
// sealed title), the position rule on a snapshot and on changes, and the time machine's fold of the `thread.*` rows.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { ThreadGroup, WorkChanged, WorkView, WorkWatchResult } from '../src/protocol.gen/index.ts'
import {
  applyChanged, applySnapshot, boardNeeds, details, EMPTY, foldThreads, groupThreads, historyLinks, isWarm, titleOf, totalNeeds,
  WARM_MS, workingSetLines,
} from '../src/lib/threadboard.ts'

const NOW = 10_000_000

function view(id: string, kind: WorkView['kind'], state: WorkView['state'], thread?: string, parent?: string, position = 1): WorkView {
  const level = state === 'needs_you' || state === 'failed' ? 'needs_you' : state === 'running' || state === 'queued' || state === 'waiting' ? 'working' : state === 'ready' ? 'ready' : 'idle'
  return {
    id, kind, position, at_ms: NOW - 60_000, parent, root: parent ?? id, title: id, state,
    attention: { level, label: state, since_ms: 1 }, started_at_ms: 1, state_since_ms: 1, thread,
  } as WorkView
}

function group(slug: string, title: string, over: Partial<ThreadGroup> = {}): ThreadGroup {
  return { slug, title, state: 'active', session_id: `ses_${slug}`, last_active_ms: NOW - 120_000, context_tokens: 41_000, today_usd: 1.2, ...over }
}

function board(): WorkView[] {
  return [
    view('exe_pr', 'conversation', 'ready', 'pr-round'),
    view('exe_pr_t', 'task', 'running', 'pr-round', 'exe_pr'),
    view('act_pr_q', 'question', 'needs_you', 'pr-round', 'exe_pr'),
    view('exe_tides', 'conversation', 'ready', 'tides'),
    view('exe_tides_t', 'task', 'running', 'tides', 'exe_tides'),
    view('exe_dm', 'conversation', 'running'),
    view('exe_dm_t', 'task', 'running', undefined, 'exe_dm'),
  ]
}

test('items are grouped by thread and unthreaded work is left alone', () => {
  const g = groupThreads(board(), [group('pr-round', 'PR round'), group('tides', 'Tides')])
  const pr = g.threads.find((t) => t.group.slug === 'pr-round')!
  assert.deepEqual(pr.items.map((v) => v.id), ['act_pr_q', 'exe_pr_t'], 'needs you first')
  assert.equal(pr.head?.id, 'exe_pr')
  assert.deepEqual(g.threads.find((t) => t.group.slug === 'tides')!.items.map((v) => v.id), ['exe_tides_t'])
  assert.deepEqual(g.loose.map((v) => v.id), ['exe_dm', 'exe_dm_t'])
})

test('a view of a thread the board has no group for is loose, not lost', () => {
  const g = groupThreads(board(), [])
  assert.equal(g.threads.length, 0)
  assert.equal(g.loose.length, board().length)
})

test('needs-you counts a question once and a pending seal, and puts those threads first', () => {
  const g = groupThreads(board(), [group('tides', 'Tides', { seal_pending: 'the notes are private' }), group('pr-round', 'PR round')])
  const n = (slug: string) => g.threads.find((t) => t.group.slug === slug)!.needsYou
  assert.equal(n('pr-round'), 1)
  assert.equal(n('tides'), 1)
  assert.equal(totalNeeds(g), 2)
  const asking = view('exe_pr', 'conversation', 'needs_you', 'pr-round')
  asking.question = { id: 'a' } as WorkView['question']
  const q = view('act_q', 'question', 'needs_you', 'pr-round', 'exe_pr')
  assert.equal(groupThreads([asking, q], [group('pr-round', 'PR round')]).threads[0]!.needsYou, 1)
})

test('a paused thread shows its count and leaves the board’s', () => {
  const g = groupThreads(board(), [group('pr-round', 'PR round', { state: 'paused' }), group('tides', 'Tides')])
  const pr = g.threads.find((t) => t.group.slug === 'pr-round')!
  assert.equal(pr.needsYou, 1)
  assert.equal(boardNeeds(pr), 0)
  assert.equal(totalNeeds(g), 0)
  assert.ok(details(pr, NOW).includes('paused'))
})

test('a done thread shows only while something under it is open', () => {
  const done = [group('tides', 'Tides', { state: 'done' })]
  assert.equal(groupThreads([view('exe_tides', 'conversation', 'done', 'tides')], done).threads.length, 0)
  assert.equal(groupThreads(board(), done).threads.length, 1)
})

test('a sealed thread’s title has its lock', () => {
  assert.equal(titleOf(group('tides', 'Tides', { sealed: true })), '🔒 Tides')
  assert.equal(titleOf(group('tides', 'Tides')), 'Tides')
})

test('the details give state, activity, warmth, context, dollars and persona', () => {
  const g = groupThreads(board(), [group('pr-round', 'PR round', { persona: 'maren' })])
  assert.deepEqual(details(g.threads[0]!, NOW), ['active', '1m ago', 'warm', '41k ctx', '$1.20 today', 'maren'])
  assert.ok(isWarm(NOW - WARM_MS, NOW) && !isWarm(NOW - WARM_MS - 1, NOW) && !isWarm(0, NOW))
  const fresh = groupThreads([], [group('new', 'New', { last_active_ms: 0, session_id: undefined })])
  assert.deepEqual(details(fresh.threads[0]!, NOW).slice(0, 2), ['active', 'no messages yet'])
})

test('a snapshot and changes apply by the position rule; an update keeps the stats it did not read', () => {
  const snap = { position: 10, views: board().map((v, i) => ({ ...v, position: 5 + i })), total: 3, threads: [group('pr-round', 'PR round')] } as WorkWatchResult
  let s = applySnapshot(EMPTY, snap)
  assert.equal(Object.keys(s.views).length, 7)
  const old = { ...view('exe_pr_t', 'task', 'needs_you', 'pr-round', 'exe_pr'), position: 2 }
  const upd: WorkChanged = { position: 11, at_ms: NOW, views: [old], gone: ['exe_dm_t'], threads: [{ ...group('pr-round', 'PR round', { state: 'paused' }), context_tokens: undefined, today_usd: undefined, last_active_ms: 1 }] }
  s = applyChanged(s, upd)
  assert.equal(s.views.exe_pr_t!.state, 'running', 'an older view is not applied')
  assert.ok(!('exe_dm_t' in s.views))
  const g = s.groups['pr-round']!
  assert.equal(g.state, 'paused')
  assert.equal(g.context_tokens, 41_000)
  assert.equal(g.last_active_ms, NOW - 120_000)
  // The snapshot after a lost event is the truth for what it names, and drops what it lacks.
  const again = applySnapshot(s, { position: 20, views: [], total: 0, threads: [] } as WorkWatchResult)
  assert.equal(Object.keys(again.views).length, 0)
  assert.ok('pr-round' in again.groups)
})

test('the history links name where a thread came from and went', () => {
  assert.deepEqual(historyLinks({ parent: 'a', split_from: 'b', merged_into: 'c' }), [
    { label: 'Under', slug: 'a' }, { label: 'Split from', slug: 'b' }, { label: 'Merged into', slug: 'c' },
  ])
  assert.deepEqual(historyLinks({}), [])
  assert.deepEqual(historyLinks({ parent: 'a', links: ['b', 'c'] }), [
    { label: 'Under', slug: 'a' }, { label: 'Linked with', slug: 'b' }, { label: 'Linked with', slug: 'c' },
  ])
  assert.deepEqual(workingSetLines({ repos: ['harbour'], files: [], hosts: ['quay.example'] }), ['Repos: harbour', 'Hosts: quay.example'])
})

test('the time machine folds a thread’s state from its rows', () => {
  const rows = [
    { at_unix_ms: 100, kind: 'thread.created', data: { slug: 'tides', title: 'Tides', state: 'active' } },
    { at_unix_ms: 200, kind: 'thread.paused', data: { slug: 'tides', title: 'Tides', state: 'paused' } },
    { at_unix_ms: 300, kind: 'thread.edited', data: { slug: 'tides', title: 'Tides and moons', state: 'paused' } },
    { at_unix_ms: 400, kind: 'thread.done', data: { slug: 'tides', title: 'Tides and moons', state: 'done' } },
    { at_unix_ms: 150, kind: 'provider.call', data: { slug: 'x' } },
  ]
  assert.deepEqual(foldThreads(rows, 50), [])
  assert.equal(foldThreads(rows, 150)[0]!.state, 'active')
  assert.equal(foldThreads(rows, 250)[0]!.state, 'paused')
  assert.equal(foldThreads(rows, 350)[0]!.title, 'Tides and moons')
  assert.equal(foldThreads(rows, 500)[0]!.state, 'done')
})
