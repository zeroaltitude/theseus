// The inspector's sections as facts (theseus-jid7t.6), over the invented board.
import test from 'node:test'
import assert from 'node:assert/strict'
import type { TaskRecord } from '../src/protocol.gen/index.ts'
import { boardRows } from '../src/ops/board.ts'
import { cacheWords, forecastWords, jobFacts, lastTurnOf, recordFacts, runningJob, standingLines, taskFacts, transcriptTail } from '../src/ops/sections.ts'
import { board, NOW } from './ops-fixtures.ts'

const rows = new Map(boardRows({ ...board(), now: NOW }).map((r) => [r.id, r]))

test('a task’s facts: spend of limit, turns, who it reports to, and the data mark', () => {
  const f = taskFacts(rows.get('ses_t1')!, NOW)
  const by = Object.fromEntries(f.map((l) => [l.label, l.value]))
  assert.match(by.spend, /of \$2/)
  assert.equal(by.turns.startsWith('3'), true)
  assert.match(by['reports to'], /home/)
  assert.equal(by.data, 'sensitive')
  assert.ok(by.cache.startsWith('warm'))
})

test('the forecast and the cache are worded, past p90 and a cold cache said', () => {
  const w = forecastWords({ usd_p50: 0.4, usd_p90: 1.2, min_p50: 6, min_p90: 20, n: 9, past: true, read: 'thrashing' })
  assert.match(w, /\$0\.40.*\$1\.20.*6–20 min.*from 9 finished.*past its p90.*thrashing/)
  assert.match(cacheWords(NOW - 6 * 60_000, NOW), /^cold/)
  assert.match(cacheWords(NOW - 60_000, NOW), /^warm/)
})

const rec = (over: Partial<TaskRecord> = {}): TaskRecord => ({ id: 'tsk_a', version: 3, title: 'Map', objective: 'map the quay', acceptance: ['every wall named'], state: 'working', deps: ['tsk_b'], owner: 'agent', origin: {} as never, evidence: [{ identity: 'n1', by: 'agent', at_ms: NOW - 60_000 }], created_at_ms: 0, updated_at_ms: 0, ...over } as TaskRecord)

test('the Record: version, owner, claim (lapsed said), deps, evidence, objective, acceptance, a waiting change', () => {
  const r = recordFacts(rec({ claim: { by: 'exe_x', session: 'ses_t1', until_ms: NOW - 1 }, proposal: { by: 'agent', card: 'drop the third wall', base_version: 3, at_ms: NOW } as never }), NOW)!
  const by = Object.fromEntries(r.lines.map((l) => [l.label, l.value]))
  assert.equal(by.record, 'tsk_a · v3 · working')
  assert.match(by.claim, /lapsed/)
  assert.equal(by['waits on'], 'tsk_b')
  assert.match(by.evidence, /^1 · the last 1m ago by agent/)
  assert.deepEqual([r.objective, r.acceptance, r.waiting?.card], ['map the quay', ['every wall named'], 'drop the third wall'])
  assert.equal(recordFacts(undefined, NOW), null)
})

test('standing records, the last turns, and a job', () => {
  const s = standingLines([{ id: 's1', kind: 'order', text: 'no pushes', scope: { kind: 'everywhere' }, state: 'live' }, { id: 's2', kind: 'fact', text: 'old', scope: { kind: 'session', of: 'ses_a' }, state: 'ended' }] as never)
  assert.deepEqual(s, [{ id: 's1', kind: 'order', text: 'no pushes', scope: 'everywhere' }])
  const nodes = ['user_message', 'tool_call', 'assistant_message'].map((kind, i) => ({ node_id: `n${i}`, kind, text: `t${i}`, at_unix_ms: i })) as never
  assert.deepEqual(transcriptTail(nodes, 5).map((x) => [x.who, x.text]), [['you', 't0'], ['theseus', 't2']])
  const job = rows.get('act_j1')!
  const jf = jobFacts(job, NOW)
  assert.equal(jf[0].label, 'command')
  assert.ok(jf.some((l) => l.label === 'running'))
})

test('the job to follow is the session’s running one; the last turn is the newest that ended', () => {
  const all = [...rows.values()]
  const j = runningJob(all, 'ses_tides')
  assert.equal(j?.id, 'act_j1')
  assert.equal(runningJob(all, 'ses_t4'), undefined)
  assert.equal(runningJob(all, undefined), undefined)
  const led = [{ kind: 'turn.ended', session_id: 'ses_a', turn_id: 't1' }, { kind: 'turn.failed', session_id: 'ses_a', turn_id: 't2' }, { kind: 'turn.ended', session_id: 'ses_b', turn_id: 't9' }, { kind: 'provider.call', session_id: 'ses_a', turn_id: 't3' }]
  assert.equal(lastTurnOf(led, 'ses_a'), 't2')
  assert.equal(lastTurnOf(led, 'ses_zz'), undefined)
})
