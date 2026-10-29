// The Agenda page's pure half and its promises (theseus-p17fd): the week strip's window and layout (an item of each kind
// at its hour, past runs by outcome, a DST week), the verbs each mark carries, the Routines table's rows and sparkline,
// the moved Wakes panel's fields, and that the page makes no request of its own (no timer, no refetch interval).
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { AgendaLine, AgendaResult, RunRow, WakeInfo, WorkView } from '../src/protocol.gen/index.ts'
import {
  allMarks, costSeries, page, dayOf, futureMarks, glyph, lastRuns, layout, outcomeOf, pastMarks, projectRows, routineRows, routineVerbs, runsSignature,
  sparkPoints, toneOf, touchesFuture, verbsFor, wakeFacts, watchBars, watchRows, weekWindow, RUNS_SHOWN, type Mark,
} from '../src/lib/agenda.ts'

const read = (p: string) => readFileSync(new URL(`../${p}`, import.meta.url), 'utf8')
const withZone = <T>(zone: string, f: () => T): T => {
  const was = process.env.TZ
  process.env.TZ = zone
  try { return f() } finally { if (was === undefined) delete process.env.TZ; else process.env.TZ = was }
}
const local = (y: number, mo: number, d: number, h = 0, mi = 0) => new Date(y, mo - 1, d, h, mi).getTime()

const line = (what: AgendaLine['what'], at_ms: number, over: Partial<AgendaLine> = {}): AgendaLine => ({
  at_ms, id: `wak_${what}_${at_ms}`, kind: what === 'run' ? 'routine' : what === 'expires' ? 'watch' : what === 'milestone' ? 'project' : what === 'starts' ? 'task' : 'wake',
  what, title: `a ${what}`, session_id: 'ses_aaaaaa', purpose: what === 'run' ? 'routine' : what === 'due' ? 'remind' : undefined, ...over,
})

const run = (n: number, over: Partial<RunRow> = {}): RunRow => ({
  task: `ses_run${String(n).padStart(3, '0')}`, short: `run${n}`, run: n, state: 'complete', started_ms: 1_000_000 + n * 3_600_000, started_local: '', duration_ms: 4_000 + n, cost_usd: 0.01 * (1 + (n % 5)), report: `report ${n}`, ...over,
})

test('the window is seven local days from midnight, today marked, and a week keeps its hours across a DST change', () => {
  withZone('America/New_York', () => {
    const w = weekWindow(local(2026, 3, 5, 14, 30))
    assert.equal(w.days.length, 7)
    assert.deepEqual(w.days.map((d) => d.today), [true, false, false, false, false, false, false])
    assert.equal(w.days[0].start_ms, local(2026, 3, 5))
    assert.equal(w.to_ms, local(2026, 3, 12))
    // 2026-03-08 is the spring-forward Sunday: a 23-hour day, and the week is 167 hours long.
    assert.deepEqual(w.days.map((d) => d.hours), [24, 24, 24, 23, 24, 24, 24])
    assert.equal((w.to_ms - w.from_ms) / 3_600_000, 167)
    assert.equal(w.days[3].label, 'Sun 3/8')
    // A 09:00 reminder lands at nine o'clock on the clock every day, the short one included.
    for (const d of w.days) {
      const at = new Date(new Date(d.start_ms).getFullYear(), new Date(d.start_ms).getMonth(), new Date(d.start_ms).getDate(), 9).getTime()
      const { placed } = layout(futureMarks([line('due', at)]), w)
      assert.equal(placed.length, 1)
      assert.equal(placed[0].day, d.index)
      assert.ok(Math.abs(placed[0].y - 9 / 24) < 1e-9, `${d.label}: ${placed[0].y}`)
    }
    // The fall-back week has a 25-hour day, and a mark there is still placed by the clock.
    const f = weekWindow(local(2026, 10, 30, 12))
    assert.deepEqual(f.days.map((d) => d.hours), [24, 24, 25, 24, 24, 24, 24])
    assert.equal(dayOf(local(2026, 11, 1, 23, 30), f), 2)
    assert.equal(dayOf(local(2026, 11, 2, 0, 5), f), 3)
  })
})

test('an item of each kind is placed on its day at its hour, with its own glyph and tone', () => {
  withZone('America/Phoenix', () => {
    const w = weekWindow(local(2026, 10, 14, 8))
    const at = (d: number, h: number, m = 0) => local(2026, 10, 14 + d, h, m)
    const lines = [
      line('due', at(0, 15), { purpose: 'remind' }), line('due', at(1, 6, 30), { purpose: 'check', id: 'wak_c' }), line('run', at(2, 8), { id: 'wak_r' }),
      line('starts', at(3, 22), { id: 'exe_t' }), line('milestone', at(4, 12), { id: 'prj_m' }),
    ]
    const bars = watchBars([{ id: 'wak_w', kind: 'watch', state: 'ready', title: 'PR 12', started_at_ms: at(1, 10), next_at_ms: at(3, 10) } as WorkView])
    const marks = allMarks(lines, [], [{ id: 'wak_w', kind: 'watch', state: 'ready', title: 'PR 12', started_at_ms: at(1, 10), next_at_ms: at(3, 10) } as WorkView])
    assert.deepEqual(marks.map((m) => m.kind), ['reminder', 'check', 'watch', 'run', 'start', 'milestone'], 'soonest first')
    const lay = layout(marks, w)
    const byKind = Object.fromEntries(lay.placed.map((p) => [p.mark.kind, p]))
    const want: Record<string, [number, number]> = { reminder: [0, 15], check: [1, 6.5], run: [2, 8], start: [3, 22], milestone: [4, 12] }
    for (const [kind, [day, hour]] of Object.entries(want)) {
      assert.equal(byKind[kind].day, day, kind)
      assert.ok(Math.abs(byKind[kind].y - hour / 24) < 1e-9, `${kind} at ${hour}`)
    }
    assert.deepEqual(['●', '○', '│', '▷', '⚑'], ['reminder', 'check', 'run', 'start', 'milestone'].map((kind) => glyph({ kind: kind as Mark['kind'] })))
    // A watch is a bar across its window: from the day it was set to the day it gives up, a piece on each day between.
    assert.deepEqual(lay.bars.map((b) => b.day), [1, 2, 3])
    assert.ok(Math.abs(lay.bars[0].from - 10 / 24) < 1e-9 && lay.bars[0].to === 1)
    assert.equal(lay.bars[1].from, 0)
    assert.ok(Math.abs(lay.bars[2].to - 10 / 24) < 1e-9)
    assert.equal(bars.length, 1)
    // The watch's own expiry line is the bar, drawn once.
    assert.equal(allMarks([line('expires', at(3, 10), { id: 'wak_w', kind: 'watch' })], [], [{ id: 'wak_w', kind: 'watch', state: 'ready', title: 'PR 12', started_at_ms: at(1, 10), next_at_ms: at(3, 10) } as WorkView]).filter((m) => m.kind === 'watch').length, 1)
    // Marks outside the window are counted, never drawn; items in one hour sit in lanes side by side.
    const crowd = layout(futureMarks([line('due', at(0, 9, 5), { id: 'a' }), line('due', at(0, 9, 20), { id: 'b' }), line('due', at(0, 9, 40), { id: 'c' }), line('due', at(9, 9), { id: 'late' })]), w)
    assert.equal(crowd.hidden, 1)
    assert.deepEqual(crowd.placed.map((p) => [p.lane, p.lanes]), [[0, 3], [1, 3], [2, 3]])
  })
})

test('past runs are coloured by how they ended', () => {
  assert.equal(outcomeOf({ state: 'complete' }), 'ok')
  assert.equal(outcomeOf({ state: 'failed' }), 'failed')
  assert.equal(outcomeOf({ state: 'cancelled' }), 'skipped')
  assert.equal(outcomeOf({ state: 'running' }), 'running')
  assert.equal(outcomeOf({ state: 'cancelled', blocked: { tool: 'gog.gmail.search', reason: 'nobody answered' } }), 'failed')
  const marks = pastMarks('Morning digest', 'ses_aaaaaa', [run(1), run(2, { state: 'failed' }), run(3, { state: 'cancelled' }), run(4, { state: 'running' })])
  assert.deepEqual(marks.map((m) => glyph(m)), ['✓', '✗', '–', '◐'])
  assert.deepEqual(marks.map((m) => toneOf(m)), ['ok', 'fault', 'idle', 'live'])
  assert.equal(new Set(marks.map((m) => m.key)).size, 4)
  assert.equal(marks[1].cost_usd, run(2).cost_usd)
})

test('each mark carries the verbs of its kind, and each verb is one protocol call', () => {
  const m = (over: Partial<Mark>): Mark => ({ key: 'k', kind: 'reminder', at_ms: 1, title: 'Call the vet', id: 'wak_abc123', session_id: 'ses_home', ...over })
  const reminder = verbsFor(m({}))
  assert.deepEqual(reminder.map((v) => v.method), ['wake.snooze', 'wake.snooze', 'wake.snooze', 'wake.snooze', 'wake.pause', 'wake.cancel'])
  assert.deepEqual(reminder.filter((v) => v.key.startsWith('snooze')).map((v) => v.params), [{ wake: 'wak_abc123', when: '10m' }, { wake: 'wak_abc123', when: '1h' }, { wake: 'wak_abc123', when: 'tomorrow 9:00' }])
  assert.ok(reminder.find((v) => v.key === 'move')?.prompts, 'Move asks for a time first')
  assert.deepEqual(reminder.at(-1)?.params, { wake: 'wak_abc123' })
  assert.ok(reminder.at(-1)?.ask, 'Cancel asks first')
  assert.deepEqual(verbsFor(m({ paused: true })).map((v) => v.method).filter((x) => x.startsWith('wake.p') || x === 'wake.resume'), ['wake.resume'])
  assert.deepEqual(verbsFor(m({ kind: 'check' })).at(-1)?.method, 'wake.cancel')
  const routine = verbsFor(m({ kind: 'run', title: 'Morning digest', session_id: 'ses_routine' }))
  assert.deepEqual(routine.map((v) => [v.label, v.method]), [['Run now', 'routine.run_now'], ['Pause', 'routine.pause'], ['Resume', 'routine.resume'], ['Skip next', 'routine.skip'], ['Cancel', 'routine.delete']])
  const skip = routine.find((v) => v.key === 'skip')
  assert.deepEqual(skip?.params, { id: 'ses_routine' })
  assert.ok(skip?.ask, 'Skip asks first: it drops one run and cannot be undone')
  assert.ok(routine.every((v) => (v.params as { id: string }).id === 'ses_routine'))
  assert.deepEqual(verbsFor(m({ kind: 'watch' })).map((v) => v.method), ['wake.cancel'])
  assert.deepEqual(verbsFor(m({ kind: 'start', id: 'exe_task' })).map((v) => [v.method, v.params]), [['task.cancel', { task: 'exe_task' }]])
  assert.deepEqual(verbsFor(m({ kind: 'milestone' })), [])
  assert.deepEqual(verbsFor(m({ kind: 'past' })), [])
  // A routine's row offers Pause when it runs and Resume when it is held, never both.
  const row = { title: 'Morning digest', session_id: 'ses_routine', id: 'wak_r' }
  assert.deepEqual(routineVerbs({ ...row, paused: false }).map((v) => v.key), ['run_now', 'pause', 'skip', 'cancel'])
  // A held series has no next run to skip: the daemon refuses it, so the row does not offer it.
  assert.deepEqual(routineVerbs({ ...row, paused: true }).map((v) => v.key), ['run_now', 'resume', 'cancel'])
})

const routineView = (over: Partial<WorkView> = {}): WorkView => ({
  id: 'wak_r1', kind: 'routine', position: 5, at_ms: 1, root: 'ses_r1', session_id: 'ses_r1', title: 'Morning digest', state: 'ready', attention: { level: 'ready', label: 'ready' },
  started_at_ms: 0, state_since_ms: 0, next_at_ms: 2_000_000, paused: false,
  routine: { schedule: 'weekdays 08:00', last: { outcome: 'ok', at_ms: 1_000_000, duration_ms: 4000, cost_usd: 0.03, line: 'three things' }, runs_30d: { ok: 20, failed: 1, skipped: 2 }, month: { spent_usd: 1.5, limit_usd: 10 }, deliver_to: 'dm' }, ...over,
} as WorkView)

test('the Routines table: the last 30 runs, a cost sparkline of them, and the row each cell reads', () => {
  const runs = Array.from({ length: 45 }, (_, i) => run(i + 1))
  const shown = lastRuns(runs)
  assert.equal(shown.length, RUNS_SHOWN)
  assert.equal(shown[0].run, 45, 'newest first')
  assert.equal(shown.at(-1)?.run, 16)
  const series = costSeries(shown)
  assert.equal(series.length, 30)
  const pts = sparkPoints(series, 84, 22).split(' ')
  assert.equal(pts.length, 30, 'a point a run')
  assert.ok(pts.every((p) => /^\d+(\.\d)?,\d+(\.\d)?$/.test(p)))
  const xs = pts.map((p) => Number(p.split(',')[0]))
  assert.deepEqual(xs, [...xs].sort((a, b) => a - b), 'oldest left, newest right')
  assert.equal(sparkPoints([], 84, 22), '')
  assert.equal(sparkPoints([0.02, 0.02, 0.02], 84, 22).split(' ')[1].split(',')[1], '11.0', 'a flat line sits mid-height')
  const rows = routineRows([routineView(), routineView({ id: 'wak_r2', title: 'Backups', next_at_ms: 1_500_000, paused: true }), { ...routineView({ id: 'wak_x' }), kind: 'wake' } as WorkView])
  assert.deepEqual(rows.map((r) => r.title), ['Backups', 'Morning digest'], 'soonest first, routines only')
  const r = rows[1]
  assert.deepEqual([r.schedule, r.record, r.spent_usd, r.limit_usd, r.share, r.last?.line, r.deliver_to], ['weekdays 08:00', { ok: 20, failed: 1, skipped: 2 }, 1.5, 10, 0.15, 'three things', 'dm'])
  assert.equal(rows[0].paused, true)
  assert.notEqual(runsSignature(routineView()), runsSignature(routineView({ routine: { ...routineView().routine!, last: { outcome: 'failed', at_ms: 3, duration_ms: 1, cost_usd: 0 } } })), 'a new run changes the signature, so its runs are read again')
  assert.equal(runsSignature(routineView()), runsSignature(routineView({ position: 99, at_ms: 5 })), 'a change that is not a run does not')
})

test('watches and projects show everything their views carry', () => {
  const w = { id: 'wak_w', kind: 'watch', title: 'PR 12 merged?', state: 'ready', watch: { check: 'gh pr view 12', until: 'equals MERGED', last: { value: 'OPEN', at_ms: 9 }, checks: 611, expires_at_ms: 99, failing: 'rate limited' } } as WorkView
  assert.deepEqual(watchRows([w, routineView()]), [{ id: 'wak_w', title: 'PR 12 merged?', check: 'gh pr view 12', until: 'equals MERGED', last: { value: 'OPEN', at_ms: 9 }, checks: 611, expires_at_ms: 99, failing: 'rate limited', state: 'ready' }])
  const p = { id: 'prj_a', kind: 'project', title: 'Move the office', project: { goal: 'Out by the 31st', steps: { done: 3, total: 12 }, milestones: { done: 1, total: 4 }, next: { title: 'Book the movers', due_at_ms: 77 }, overdue: 1, review: 'Fri' } } as WorkView
  const rows = projectRows([p])
  assert.equal(rows[0].share, 0.25)
  assert.deepEqual([rows[0].goal, rows[0].next, rows[0].overdue, rows[0].review], ['Out by the 31st', { title: 'Book the movers', due_at_ms: 77 }, 1, 'Fri'])
  assert.equal(projectRows([{ ...p, project: { ...p.project!, steps: { done: 0, total: 0 } } } as WorkView])[0].share, 0)
})

const WAKE: WakeInfo = {
  wake_id: 'wak_01234567890abc', short: '890abc', session_id: 'ses_home1234', execution_id: 'exe_1', session_title: 'Reminders', task: undefined, due_at_ms: 10_000_000, due_local: 'Fri 09:00',
  note: 'Call the vet', set_at_ms: 9_000_000, target: 'discord dm', state: 'waiting', every: '1d', days: ['mon', 'fri'], paused: true, purpose: 'remind',
}

test('the Wakes panel moved here loses no field it showed on Actions', () => {
  const f = wakeFacts(WAKE, 9_500_000, (n) => `${n / 1000}s`, (n) => `T${n}`, (id) => id.slice(-6))
  // The old panel read: note, short, session title (else the short id), task (when it has one), state, due_local, the
  // relative due, and in its tooltip the wake id, when it was set and where its reply goes.
  assert.equal(f.note, 'Call the vet')
  assert.equal(f.short, '890abc')
  assert.equal(f.where, 'Reminders')
  assert.equal(f.session_id, 'ses_home1234')
  assert.equal(f.state, 'waiting')
  assert.equal(f.due_local, 'Fri 09:00')
  assert.equal(f.relative, 'in 500s')
  assert.equal(f.tip, 'wak_01234567890abc\nset T9000000\nits reply goes to discord dm')
  assert.equal(wakeFacts({ ...WAKE, task: 'a1b2c3' }, 0, String, String, (i) => i).where, 'task a1b2c3')
  assert.equal(wakeFacts({ ...WAKE, session_title: undefined }, 0, String, String, (i) => i.slice(-4)).where, '1234')
  assert.equal(wakeFacts(WAKE, 12_000_000, (n) => `${n / 1000}s`, String, String).relative, 'due 2000s ago')
  assert.equal(wakeFacts({ ...WAKE, target: undefined }, 0, String, String, String).tip, 'wak_01234567890abc\nset 9000000')
  assert.equal(f.repeat, 'every 1d (mon, fri)')
  assert.equal(f.paused, true)
  // ...and the new panel draws every one of them, and keeps Cancel.
  const panel = read('src/components/Wakes.tsx')
  for (const part of ['f.note', 'f.short', 'f.where', 'f.state', 'f.due_local', 'f.relative', 'f.tip', 'f.session_id']) assert.ok(panel.includes(part), `the panel draws ${part}`)
  assert.match(panel, /'wake\.cancel', \{ wake: f\.wake_id \}, `Cancel the wake/)
  assert.match(panel, /<StatePill state=\{f\.state\}/)
  // Actions keeps a link to it, and no panel of its own.
  const actions = read('src/views/Actions.tsx')
  assert.ok(!/function Wakes\b/.test(actions) && !/<Wakes\b/.test(actions), 'the panel left Actions')
  assert.match(actions, /<Link to="\/agenda"/)
  assert.match(actions, /data-testid="wakes-link"/)
  // The Bridge's tile opens the page: the Operations page's Timeline lens (theseus-jid7t.5).
  assert.match(read('src/views/Bridge.tsx'), /label="Wakes due"[\s\S]{0,400}nav\('\/ops\?lens=timeline'\)[\s\S]{0,80}on the Timeline/)
})

test('the page is on the rail, in a group, behind a route, and says what it shows', () => {
  // The page is the Operations page's Timeline lens now (theseus-jid7t.3): /agenda redirects there, `g d` goes there.
  assert.match(read('src/components/Shell.tsx'), /d: '\/ops\?lens=timeline'/)
  assert.match(read('src/lib/navgroups.ts'), /'\/ops'/)
  assert.match(read('src/main.tsx'), /path: 'agenda', element: <ToOps \/>/)
  const page = [read('src/ops/TimelineLens.tsx'), read('src/components/Wakes.tsx')].join('\n')
  for (const id of ['agenda.week', 'agenda.routines', 'agenda.watches', 'agenda.projects', 'agenda.wakes']) assert.ok(page.includes(`help="${id}"`), id)
})

test('the page idles: no timer of its own that asks, no refetch interval, and only a change of the future reads it again', () => {
  const files = ['src/lib/agendawatch.ts', 'src/ops/TimelineLens.tsx', 'src/ops/WeekStripV2.tsx', 'src/components/WeekStrip.tsx', 'src/components/RoutinesTable.tsx', 'src/components/FutureLists.tsx', 'src/components/Wakes.tsx']
  for (const f of files) {
    const src = read(f)
    assert.ok(!/setInterval|refetchInterval|useRpc\(|useQuery\(/.test(src.replace(/\/\/.*$/gm, '')), `${f} polls`)
  }
  // The only timers: the page's minute hand (a state change, no call) and the push's debounce.
  assert.match(read('src/ops/TimelineLens.tsx'), /useTick\(60_000\)/)
  assert.match(read('src/lib/agendawatch.ts'), /setTimeout\(\(\) => \{ timer = null; void read\(\) \}, DEBOUNCE_MS\)/)
  const none = new Set<string>()
  assert.equal(touchesFuture({ views: [{ kind: 'conversation' }, { kind: 'job' }, { kind: 'question' }] }, none), false, 'a conversation, a job or a question is not the future')
  assert.equal(touchesFuture({ views: [{ kind: 'wake' }] }, none), true)
  assert.equal(touchesFuture({ views: [{ kind: 'routine' }] }, none), true)
  assert.equal(touchesFuture({ views: [{ kind: 'task', next_at_ms: 5 }] }, none), true, 'a deferred task')
  assert.equal(touchesFuture({ views: [], gone: ['wak_x'] }, new Set(['wak_x'])), true, 'a wake that left')
  assert.equal(touchesFuture({ views: [], gone: ['wak_y'] }, new Set(['wak_x'])), false)
})

test('the strip lays out 200 items in under 16 ms', () => {
  withZone('America/Phoenix', () => {
    const now = local(2026, 10, 14, 8)
    const w = weekWindow(now)
    const marks: Mark[] = []
    for (let i = 0; i < 200; i++) {
      const at = w.from_ms + ((i * 7919) % (7 * 24 * 60)) * 60_000
      marks.push({ key: `m${i}`, kind: (['reminder', 'check', 'run', 'past', 'start', 'milestone'] as const)[i % 6], at_ms: at, title: `item ${i}`, id: `wak_${i}`, outcome: i % 12 === 3 ? 'failed' : 'ok' })
    }
    for (let i = 0; i < 6; i++) marks.push({ key: `w${i}`, kind: 'watch', at_ms: w.from_ms + i * 3_600_000, end_ms: w.to_ms - i * 3_600_000, title: 'w', id: `wak_w${i}` })
    layout(marks, w)
    const times: number[] = []
    for (let i = 0; i < 31; i++) { const t0 = performance.now(); layout(marks, w); times.push(performance.now() - t0) }
    times.sort((a, b) => a - b)
    const median = times[15]
    console.log(`layout of ${marks.length} items: median ${median.toFixed(3)} ms, worst ${times.at(-1)!.toFixed(3)} ms`)
    assert.ok(median < 16, `median ${median} ms`)
    assert.equal(layout(marks, w).placed.length, 200)
  })
})

test('a busy series that fills the daemon’s cap is read on from where its lines stopped, and nothing is lost or doubled', async () => {
  // A minute-by-minute stream of 450 lines, the daemon answering 200 at a time from the asked time, `more` while it has left.
  const all = Array.from({ length: 450 }, (_, i) => line('run', 1_000 + i * 60_000, { id: 'wak_busy', occurrence: i + 1 }))
  const ask: number[] = []
  const answer = async (from: number): Promise<AgendaResult> => {
    ask.push(from)
    const rest = all.filter((l) => l.at_ms >= from)
    return { from_ms: from, to_ms: 9e12, lines: rest.slice(0, 200), more: rest.length > 200 }
  }
  const out = await page(await answer(0), answer)
  assert.equal(out.lines.length, 450)
  assert.equal(new Set(out.lines.map((l) => `${l.id}:${l.at_ms}`)).size, 450, 'no line twice')
  assert.equal(out.more, false)
  assert.equal(ask.length, 3, 'three reads for 450 lines, the first included')
  assert.deepEqual(out.lines.map((l) => l.at_ms), all.map((l) => l.at_ms), 'in order')
  // One read when the answer is whole; a stream that never ends stops at the page limit and says it is cut.
  ask.length = 0
  const small = await page({ from_ms: 0, to_ms: 9, lines: all.slice(0, 3), more: false }, answer)
  assert.equal(small.lines.length, 3)
  assert.equal(ask.length, 0)
  let calls = 0
  const endless = async (from: number): Promise<AgendaResult> => { calls++; return { from_ms: from, to_ms: 9e12, lines: [line('run', from + 60_000, { id: `wak_${calls}` })], more: true } }
  const cut = await page({ from_ms: 0, to_ms: 9e12, lines: [line('run', 5, { id: 'wak_0' })], more: true }, endless)
  assert.equal(cut.more, true)
  assert.ok(calls <= 12, `${calls} reads`)
})
