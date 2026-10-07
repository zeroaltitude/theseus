// The watch's sixth plate, since you last looked (`src/ship/since.ts`, theseus-hnof.3), run by `npm test`: the look
// this browser keeps, the stretch's sessions, tasks, failures, questions and spend, the moment's limits, and the
// replay's moments.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { LOOK_GAP_MS, lookAt, looked, replayMoments, sinceOf, type SinceInput } from '../src/ship/since.ts'

const HOUR = 3_600_000
const MIN = 60_000
const NOW = Date.UTC(2026, 9, 6, 21, 0, 0)
/** The end of your last look: three hours before the moment. */
const SINCE = NOW - 3 * HOUR

interface V { id: string; title?: string; kind?: string; executionId?: string }
interface L { id: string; session: string; kind: 'user' | 'model' | 'call' | 'result'; at: number; cid?: string; tool?: string; failed?: boolean; preview?: string }

/** A model with only what the plate reads: vessels, lights, and their lookups. */
function fleet(vs: V[], ls: L[] = []) {
  const vessels = vs.map((v) => ({ id: v.id, title: v.title ?? v.id, kind: v.kind ?? 'conversation', executionId: v.executionId, state: 'waiting', rig: 'anchor' }))
  const byId = new Map(vessels.map((v, i) => [v.id, i]))
  const lights = ls.map((l) => ({ id: l.id, sessionId: l.session, vessel: byId.get(l.session), kind: l.kind, at: l.at, correlationId: l.cid, tool: l.tool, failed: l.failed, preview: l.preview ?? '' }))
  return { vessels, lights, byId, lightById: new Map(lights.map((l, i) => [l.id, i])) } as unknown as SinceInput['model']
}

let pos = 0
const row = (at: number, kind: string, session: string | null, data: Record<string, unknown> = {}, turn: string | null = null) =>
  ({ position: ++pos, at_unix_ms: at, kind, session_id: session, turn_id: turn, data })

const asked = (at: number, cid: string, session: string, path: string) => row(at, 'tool.confirm_requested', session, {
  correlation_id: cid, session_id: session, tool: 'fs.write', requested_at_ms: at, expires_at_ms: at + 15 * MIN,
  reason: `create ${path} (38 bytes): fs.write — approve ([policy.tools] "fs.write" = approve)`,
})

const rows = [
  // Before the stretch: never read. A question asked then is answered in it: it went, it did not come.
  row(SINCE - 20 * MIN, 'tool.confirm_requested', 'ses_dm', { correlation_id: 'act_q0', tool: 'fs.write', reason: 'create notes.md: fs.write — approve' }),
  row(SINCE - 10 * MIN, 'action.failed', 'ses_dm', { correlation_id: 'act_before', producer: 'inproc:fs.read' }),
  row(SINCE + 5 * MIN, 'action.confirm_answered', 'ses_dm', { correlation_id: 'act_q0', approved: true, by: 'discord:owner', via: 'discord' }),
  // A session and a task start.
  row(SINCE + 10 * MIN, 'execution.opened', 'ses_new', { execution_id: 'exe_new', kind: 'conversation', limit_usd: 5 }),
  row(SINCE + 20 * MIN, 'execution.opened', 'ses_task', { execution_id: 'exe_task', kind: 'task', parent: 'exe_dm', limit_usd: 1 }),
  row(SINCE + 21 * MIN, 'turn.ended', 'ses_new', { elapsed_ms: 900 }, 'turn_n1'),
  row(SINCE + 22 * MIN, 'provider.call', 'ses_new', { cost_usd: 0.1 }),
  // A call fails.
  row(SINCE + 29 * MIN, 'action.planned', 'ses_dm', { correlation_id: 'act_read', tool: 'fs.read' }),
  row(SINCE + 30 * MIN, 'action.failed', 'ses_dm', { correlation_id: 'act_read', producer: 'inproc:fs.read' }),
  // A question is asked and answered; another asked and left to expire; a third still waits.
  asked(SINCE + 40 * MIN, 'act_q1', 'ses_dm', '/tmp/x/projects/harbour/log.md'),
  row(SINCE + 45 * MIN, 'action.confirm_answered', 'ses_dm', { correlation_id: 'act_q1', approved: true, by: 'discord:owner', via: 'discord' }),
  asked(SINCE + 60 * MIN, 'act_q2', 'ses_dm', '/tmp/x/projects/harbour/departures.md'),
  // An expiry declines the call in the same frame: the expiry says it.
  row(SINCE + 75 * MIN, 'action.declined', 'ses_dm', { correlation_id: 'act_q2', tool: 'fs.write', by: 'expiry', reason: 'expired' }),
  row(SINCE + 75 * MIN, 'action.expired', 'ses_dm', { correlation_id: 'act_q2', tool: 'fs.write', waited_ms: 15 * MIN }),
  row(SINCE + 80 * MIN, 'turn.ended', 'ses_new', { elapsed_ms: 1200 }, 'turn_n2'),
  row(SINCE + 81 * MIN, 'turn.ended', 'ses_dm', { elapsed_ms: 2000 }, 'turn_d1'),
  row(SINCE + 82 * MIN, 'provider.call', 'ses_dm', { cost_usd: 0.2 }),
  // The task reports back; another task, older, fails.
  row(SINCE + 2 * HOUR, 'task.ended', 'ses_dm', { execution_id: 'exe_dm', task: 'exe_task', state: 'complete', spent_usd: 0.05 }),
  row(SINCE + 150 * MIN, 'task.ended', 'ses_dm', { execution_id: 'exe_dm', task: 'exe_gull', state: 'failed', reason: 'its budget ran out' }),
  row(SINCE + 160 * MIN, 'provider.call', 'ses_dm', { cost_usd: 0.05 }),
  asked(SINCE + 170 * MIN, 'act_q3', 'ses_new', '/tmp/x/projects/harbour/tides.md'),
  // After the moment: never read.
  row(NOW + MIN, 'action.failed', 'ses_dm', { correlation_id: 'act_later', producer: 'inproc:fs.read' }),
]

const model = fleet([
  { id: 'ses_dm', title: 'Good morning' }, { id: 'ses_new', title: 'What is the swell today?' },
  { id: 'ses_task', title: 'Write up the harbour notes', kind: 'task', executionId: 'exe_task' },
  { id: 'ses_gull', title: 'Count every gull', kind: 'task', executionId: 'exe_gull' },
], [
  { id: 'tcl_read', session: 'ses_dm', kind: 'call', at: SINCE + 30 * MIN, cid: 'act_read', tool: 'fs.read' },
  { id: 'trs_read', session: 'ses_dm', kind: 'result', at: SINCE + 30 * MIN, cid: 'act_read', tool: 'fs.read', failed: true, preview: 'cannot read …/harbour/lighthouse-log.txt: No such file or directory (os error 2)' },
  { id: 'tcl_q1', session: 'ses_dm', kind: 'call', at: SINCE + 40 * MIN, cid: 'act_q1', tool: 'fs.write' },
])
const confirms = [{ correlation_id: 'act_q3', session_id: 'ses_new' }] as unknown as SinceInput['confirms']

/** Back at the Ship now, after three hours away. */
const at = (o: Partial<SinceInput>): SinceInput => ({ model, confirms, rows, rowsReady: true, since: SINCE, until: NOW, now: NOW, ...o })

test('the look this browser keeps: a gap of five minutes ends a look, and the next tells the stretch between', () => {
  // A browser's first look tells nothing yet.
  assert.deepEqual(lookAt(null, NOW), { since: NOW, start: NOW, seen: NOW })
  // Back within the gap (a reload, a glance away): the look goes on, and the stretch it tells stays.
  const look = { since: SINCE, start: NOW - 30 * MIN, seen: NOW - 2 * MIN }
  assert.deepEqual(lookAt(look, NOW), { since: SINCE, start: NOW - 30 * MIN, seen: NOW })
  // Back after it: a new look, and the stretch from where the Ship was last in sight to now.
  assert.deepEqual(lookAt({ ...look, seen: NOW - LOOK_GAP_MS - 1 }, NOW), { since: NOW - LOOK_GAP_MS - 1, start: NOW, seen: NOW })
  // A time kept in the future (a clock set back) starts over.
  assert.deepEqual(lookAt({ ...look, seen: NOW + HOUR }, NOW), { since: NOW, start: NOW, seen: NOW })
  assert.deepEqual(looked(JSON.stringify(look)), look)
  // Kept without its start (an older page's), the look began where its stretch did.
  assert.deepEqual(looked(JSON.stringify({ since: SINCE, seen: NOW })), { since: SINCE, start: SINCE, seen: NOW })
  for (const bad of [null, '', 'not json', '{"since": "x", "seen": 1}', '{"since": 0, "seen": 0}', '[1, 2]', JSON.stringify({ since: NOW, start: SINCE, seen: NOW })]) {
    assert.equal(looked(bad), null, String(bad))
  }
})

test('since you last looked: sessions and tasks started and finished, what went wrong, the questions, and the spend', () => {
  const s = sinceOf(at({}))
  assert.equal(s.value, '3h 00m')
  assert.equal(s.away, 3 * HOUR)
  assert.equal(s.quiet, false)
  assert.deepEqual(s.sessions, { started: 1, worked: 2, turns: 3 })
  assert.deepEqual(s.tasks, { started: 1, ended: 2, failed: 1 })
  assert.equal(s.failures, 1)
  assert.deepEqual(s.questions, { came: 3, went: 3, missed: 2, waiting: 1 })
  assert.ok(Math.abs(s.usd - 0.35) < 1e-9)
  assert.equal(s.calls, 3)
  assert.equal(s.tone, 'fault')
  assert.equal(s.caption, 'while you were away: 1 session started, 1 task started, 2 tasks finished, 1 failure, 2 questions came and went and $0.350 spent')
  // A line of each kind, the newest of each: what went wrong, a question you never saw, a task that ended; then the
  // next of each, and what started (the plate shows three).
  assert.deepEqual(s.lines.map((l) => [l.tag, l.text, l.detail, l.flag, l.figure]), [
    ['fs.read', 'cannot read …/harbour/lighthouse-log.txt: No such file or directory (os error 2)', undefined, undefined, '2h ago'],
    ['fs.write', 'create …/harbour/departures.md (38 bytes)', 'expired after 15m · Good morning', 'expired', '2h 00m'],
    ['task', 'Count every gull', 'failed: its budget ran out', undefined, '30m'],
  ])
  assert.deepEqual(s.lines[0].to, { session: 'ses_dm', node: 'trs_read' })
  assert.deepEqual(s.lines[2].to, { session: 'ses_gull' })
  // Four more: the older question, the task that reported back, and the session and the task that started.
  assert.equal(s.more, 4)
  assert.deepEqual(s.tally.map((t) => [t.part, t.value, t.word, t.sub, t.tone]), [
    ['sessions', '2', 'sessions', '1 new', 'live'],
    ['tasks', '2', 'finished', '1 started', 'fault'],
    ['wrong', '1', 'failed', 'went wrong', 'fault'],
    ['questions', '2', 'missed', '1 still waits', 'wait'],
    ['spent', '$0.350', 'spent', '3 calls', 'money'],
  ])
  // Each part lights its own; show lights them all.
  const part = Object.fromEntries(s.tally.map((t) => [t.part, t.focus]))
  assert.deepEqual(part.sessions, { vessels: ['ses_dm', 'ses_new'], lights: [] })
  assert.deepEqual(part.tasks, { vessels: ['ses_gull', 'ses_task'], lights: [] })
  assert.deepEqual(part.wrong, { vessels: ['ses_dm'], lights: ['tcl_read', 'trs_read'] })
  assert.deepEqual(part.questions, { vessels: ['ses_dm', 'ses_new'], lights: ['tcl_q1'] })
  assert.deepEqual(s.focus, { vessels: ['ses_dm', 'ses_gull', 'ses_new', 'ses_task'], lights: ['tcl_q1', 'tcl_read', 'trs_read'] })
  // The ledger from that moment.
  assert.equal(s.link.to, `/ledger?from=${SINCE}&to=${NOW}`)
})

test("the stretch is the time away: nothing from this look, nor (the time machine, a replay) after the moment", () => {
  // Back an hour ago: what happened in the last hour, you saw (q3's question is not counted); the lines say how long ago.
  const back = sinceOf(at({ until: NOW - HOUR }))
  assert.deepEqual([back.value, back.away], ['2h 00m', 2 * HOUR])
  assert.deepEqual(back.questions, { came: 2, went: 3, missed: 2, waiting: 0 })
  assert.deepEqual(back.tasks, { started: 1, ended: 1, failed: 0 })
  assert.equal(back.lines[0].figure, '2h ago')
  assert.equal(back.link.to, `/ledger?from=${SINCE}&to=${NOW - HOUR}`)
  const mid = sinceOf(at({ now: SINCE + 50 * MIN }))
  assert.equal(mid.value, '3h 00m')
  assert.deepEqual(mid.sessions, { started: 1, worked: 1, turns: 1 })
  assert.deepEqual(mid.tasks, { started: 1, ended: 0, failed: 0 })
  assert.equal(mid.failures, 1)
  assert.deepEqual(mid.questions, { came: 1, went: 2, missed: 1, waiting: 0 })
  assert.ok(Math.abs(mid.usd - 0.1) < 1e-9)
  // A moment before the stretch began says so, and so does a first look.
  const before = sinceOf(at({ now: SINCE - HOUR }))
  assert.deepEqual([before.quiet, before.caption], [true, 'this moment is before your last look ended'])
  const first = sinceOf(at({ since: NOW }))
  assert.deepEqual([first.value, first.quiet, first.tone, first.lines], ['—', true, 'idle', []])
  assert.equal(first.caption, 'you have looked all along: it tells what happens while you are away')
  // Back after a quiet stretch.
  const quiet = sinceOf(at({ since: NOW - 10 * MIN }))
  assert.deepEqual([quiet.value, quiet.quiet, quiet.caption], ['10m', true, 'nothing happened while you were away'])
  // Before the ledger is read, the number says so.
  assert.equal(sinceOf(at({ rowsReady: false })).value, '…')
})

test('a replay gives the busy minutes its time, from the stretch start to its end', () => {
  const from = NOW - 5 * HOUR
  const busy = from + HOUR
  const times = Array.from({ length: 10 }, (_, i) => busy + i * 12_000)
  const out = replayMoments(times, from, NOW, 50)
  assert.equal(out.length, 50)
  assert.equal(out[0], from)
  assert.equal(out[49], NOW)
  for (let i = 1; i < out.length; i++) assert.ok(out[i] >= out[i - 1], `moment ${i} goes back`)
  // Two busy minutes of five hours: a sixth of the replay (its quiet hours fold to five minutes each), not 0.7%.
  const inBusy = out.filter((t) => t >= busy && t <= busy + 2 * MIN).length
  assert.ok(inBusy >= 7 && inBusy <= 10, `${inBusy} moments in the busy minutes`)
  assert.deepEqual(replayMoments(times, NOW, NOW, 50), [NOW])
  assert.deepEqual(replayMoments([], from, NOW, 1), [NOW])
  // With nothing in it, the stretch sweeps evenly.
  const even = replayMoments([], from, from + 4 * MIN, 5)
  assert.deepEqual(even, [from, from + MIN, from + 2 * MIN, from + 3 * MIN, from + 4 * MIN])
})
