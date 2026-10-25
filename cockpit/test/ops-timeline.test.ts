// The Timeline lens's layout (theseus-jid7t.5): events and marks share a day; overlapping events in side-by-side lanes;
// the all-day band; DST days keep their wall-clock rows; the night fold and its count; the ran bars; the clashes; the
// Day zoom and the spans read; the keys' order and the Horizon verbs; the table's rows. All on invented events.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { RunRow, WorkView } from '../src/protocol.gen/index.ts'
import { weekWindow, type Mark } from '../src/lib/agenda.ts'
import {
  calEvents, centerHour, clashes, eventLayout, heightOf, horizonVerb, hourAt, meetingCount, nightFold, openHours, ranBars, readSpans, scrollFor, stepOrder, tableRows, timelineOrder, yOf, zoomDay, zoomed,
} from '../src/ops/timeline.ts'
import { cal, ev, local, SECRET, twoCalendars } from './ops-calendar-fixtures.ts'

const withZone = <T>(zone: string, f: () => T): T => {
  const was = process.env.TZ
  process.env.TZ = zone
  try { return f() } finally { if (was === undefined) delete process.env.TZ; else process.env.TZ = was }
}
const mark = (key: string, kind: Mark['kind'], at_ms: number, over: Partial<Mark> = {}): Mark => ({ key, kind, at_ms, title: `a ${kind}`, id: `wak_${key}`, session_id: 'ses_harbour', ...over })

test('events lay out by wall-clock hour, overlapping ones side by side, each cluster sharing its lane count', () => {
  withZone('America/Phoenix', () => {
    const w = weekWindow(local(2026, 10, 19, 8))
    const work = cal('work.cal.example.test', {
      events: [
        ev('a', local(2026, 10, 20, 9), local(2026, 10, 20, 10, 30)),
        ev('b', local(2026, 10, 20, 10), local(2026, 10, 20, 11)),
        ev('c', local(2026, 10, 20, 10, 15), local(2026, 10, 20, 10, 45)),
        ev('d', local(2026, 10, 20, 13), local(2026, 10, 20, 14)),
      ],
    })
    const { blocks } = eventLayout(calEvents([work]), w)
    const by = Object.fromEntries(blocks.map((b) => [b.key, b]))
    assert.equal(by.a.day, 1)
    assert.equal(by.a.top, 9 / 24)
    assert.equal(by.a.bottom, 10.5 / 24)
    assert.deepEqual([by.a.lane, by.b.lane, by.c.lane], [0, 1, 2], 'three at once: three lanes')
    assert.deepEqual([by.a.lanes, by.b.lanes, by.c.lanes], [3, 3, 3])
    assert.deepEqual([by.d.lane, by.d.lanes], [0, 1], 'a lone meeting has the width')
  })
})

test('a meeting over midnight is cut at each day; an all-day event rides the band of every day it covers', () => {
  withZone('America/Phoenix', () => {
    const w = weekWindow(local(2026, 10, 19, 8))
    const work = cal('personal.cal.example.test', {
      events: [
        ev('late', local(2026, 10, 21, 23), local(2026, 10, 22, 1)),
        ev('trip', local(2026, 10, 22), local(2026, 10, 24), { all_day: true }),
        ev('fair', local(2026, 10, 22), local(2026, 10, 23), { all_day: true }),
      ],
    })
    const { blocks, allDay } = eventLayout(calEvents([work]), w)
    const late = blocks.filter((b) => b.key === 'late')
    assert.deepEqual(late.map((b) => [b.day, b.top, b.bottom]), [[2, 23 / 24, 1], [3, 0, 1 / 24]])
    assert.deepEqual(allDay.filter((a) => a.key === 'trip').map((a) => a.day), [3, 4], 'Google’s end is exclusive')
    assert.deepEqual(allDay.filter((a) => a.day === 3).map((a) => [a.key, a.row]), [['fair', 0], ['trip', 1]], 'two that day: two rows of the band')
    assert.equal(blocks.some((b) => b.key === 'trip'), false, 'an all-day event is never a block')
  })
})

test('a DST week keeps its wall-clock rows: 09:00 sits at 09:00 on the 23- and the 25-hour day', () => {
  withZone('America/New_York', () => {
    const spring = weekWindow(local(2026, 3, 5, 12))
    assert.deepEqual(spring.days.map((d) => d.hours), [24, 24, 24, 23, 24, 24, 24])
    const fall = weekWindow(local(2026, 10, 29, 12))
    assert.deepEqual(fall.days.map((d) => d.hours), [24, 24, 24, 25, 24, 24, 24])
    for (const [w, y, m, d] of [[spring, 2026, 3, 8], [fall, 2026, 11, 1]] as const) {
      const e = calEvents([cal('work.cal.example.test', { events: [ev('nine', local(y, m, d, 9), local(y, m, d, 10))] })])
      const b = eventLayout(e, w).blocks[0]
      assert.equal(b.day, 3)
      assert.equal(b.top, 9 / 24, `${m}/${d}`)
      assert.equal(b.bottom, 10 / 24)
    }
  })
})

test('the night folds 22:00 to 07:00, kept open where a meeting or one-off mark is, and counts the series’ ticks inside', () => {
  withZone('America/Phoenix', () => {
    const w = weekWindow(local(2026, 10, 19, 8))
    const nightly = [mark('r1', 'past', local(2026, 10, 20, 3), { outcome: 'ok' }), mark('r2', 'run', local(2026, 10, 21, 3))]
    assert.deepEqual(nightFold(w, nightly, []), { early: 7, late: 22, count: 2 }, 'two runs at 03:00: folded, and counted')
    const reminder = [...nightly, mark('rem', 'reminder', local(2026, 10, 22, 6, 30))]
    assert.deepEqual(nightFold(w, reminder, []), { early: 6, late: 22, count: 2 }, 'a reminder at 06:30 opens from 06:00')
    const meeting = eventLayout(calEvents([cal('work.cal.example.test', { events: [ev('m', local(2026, 10, 23, 21), local(2026, 10, 23, 22, 30))] })]), w).blocks
    assert.deepEqual(nightFold(w, [], meeting), { early: 7, late: 23, count: 0 }, 'a meeting to 22:30 keeps the hour to 23:00')
    // The rows under the fold: a folded band each side, the open hours at their full height, and back again.
    const fold = { early: 7, late: 22 }
    assert.equal(yOf(7, fold, 20, 18), 18)
    assert.equal(yOf(12, fold, 20, 18), 18 + 5 * 20)
    assert.equal(heightOf(fold, 20, 18), 18 + 15 * 20 + 18)
    assert.equal(heightOf(null, 20, 18), 480)
    for (const h of [0, 3.5, 7, 9.25, 21.5, 22, 23, 24]) assert.ok(Math.abs(hourAt(yOf(h, fold, 20, 18), fold, 20, 18) - h) < 1e-9, `hour ${h} round trips`)
  })
})

test('the ran bars: each routine run and each task’s execution, start to end, by outcome, cut at midnight', () => {
  withZone('America/Phoenix', () => {
    const now = local(2026, 10, 21, 12)
    const w = weekWindow(local(2026, 10, 19, 8))
    const run = (n: number, state: string, start: number, dur: number): RunRow => ({ task: `ses_run${n}`, short: `r${n}`, run: n, state, started_ms: start, started_local: '', duration_ms: dur, cost_usd: 0.01 })
    const runs = { ses_routine: [run(1, 'complete', local(2026, 10, 19, 9), 30 * 60_000), run(2, 'failed', local(2026, 10, 19, 23, 30), 60 * 60_000)] }
    const view = (id: string, over: Partial<WorkView>): WorkView => ({ id, kind: 'task', position: 0, at_ms: 0, root: id, title: id, state: 'done', attention: 'none', started_at_ms: 0, state_since_ms: 0, ...over } as WorkView)
    const views = [
      view('tsk_live', { state: 'running', started_at_ms: local(2026, 10, 21, 11) }),
      view('tsk_waiting', { state: 'queued', started_at_ms: local(2026, 10, 21, 11), next_at_ms: local(2026, 10, 22, 9) }),
      view('ses_talk', { kind: 'conversation', started_at_ms: local(2026, 10, 21, 8) }),
    ]
    const bars = ranBars(views, runs, w, now)
    const by = (k: string) => bars.filter((b) => b.key.startsWith(k))
    assert.deepEqual(by('ran:ses_routine:ses_run1').map((b) => [b.day, b.top, b.bottom, b.tone]), [[0, 9 / 24, 9.5 / 24, 'ok']])
    assert.deepEqual(by('ran:ses_routine:ses_run2').map((b) => [b.day, b.tone]), [[0, 'fault'], [1, 'fault']], 'over midnight: both days')
    assert.deepEqual(by('ran:tsk_live').map((b) => [b.day, b.top, b.bottom, b.tone]), [[2, 11 / 24, 12 / 24, 'live']], 'running: to now')
    assert.equal(by('ran:tsk_waiting').length, 0, 'a deferred task has not run')
    assert.equal(by('ran:ses_talk').length, 0, 'a conversation is not a run')
  })
})

test('clashes: a mark or deferred start inside a meeting, two meetings that overlap; all-day and declined events do not', () => {
  withZone('America/Phoenix', () => {
    const { personal, work, secret } = twoCalendars()
    const busy = cal('work.cal.example.test', { id: 'busy.cal.example.test', status: 'busy_only', events: [ev('evk_busy', local(2026, 10, 23, 14, 30), local(2026, 10, 23, 16), { busy: true })] })
    const extra = cal('personal.cal.example.test', {
      id: 'more.cal.example.test',
      events: [ev('evk_allday', local(2026, 10, 23), local(2026, 10, 24), { all_day: true }), ev('evk_declined', local(2026, 10, 23, 14), local(2026, 10, 23, 15), { self_response: 'declined' })],
    })
    const events = calEvents([personal, work, busy, extra])
    const marks = [
      mark('rem', 'reminder', local(2026, 10, 23, 14, 10)),
      mark('start', 'start', local(2026, 10, 23, 18, 45)),
      mark('free', 'check', local(2026, 10, 23, 12)),
      mark('ran', 'past', local(2026, 10, 23, 14, 20), { outcome: 'ok' }),
      mark('edge', 'reminder', local(2026, 10, 23, 15)),
    ]
    const c = clashes(marks, events)
    assert.ok(c.keys.has(secret.key) && c.keys.has('evk_busy'), 'the work meeting and the busy block overlap')
    assert.ok(c.keys.has('rem'), 'a reminder inside the work meeting')
    assert.ok(c.keys.has('start'), 'a deferred start inside the pottery class')
    assert.ok(!c.keys.has('free') && !c.keys.has('ran'), 'no meeting at noon; a past run is not a clash')
    assert.ok(c.keys.has('edge'), 'at 15:00 the busy block still runs')
    assert.ok(!c.keys.has('evk_allday') && !c.keys.has('evk_declined'), 'all-day and declined events never clash')
    // Pairs: work×busy, rem×work (14:10 is before the busy block), start×pottery, edge×busy.
    assert.equal(c.count, 4)
    assert.match(c.why.get('rem')!, /inside a Work meeting/)
    const w = weekWindow(local(2026, 10, 19, 8))
    assert.equal(meetingCount(events, w), 3, 'the work meeting, the busy block and the pottery class')
  })
})

test('the clash sweep is quick on a busy week: 400 meetings and 400 marks in under 20 ms', () => {
  const evs = Array.from({ length: 400 }, (_, i) => ev(`e${i}`, i * 900_000, i * 900_000 + 1_800_000))
  const marks = Array.from({ length: 400 }, (_, i) => mark(`m${i}`, 'reminder', i * 800_000))
  const events = calEvents([cal('work.cal.example.test', { events: evs })])
  // The best of five: the sweep's own cost, not a busy machine's scheduling (it flaked under the load recipe).
  let best = Infinity
  let c = clashes(marks, events)
  for (let i = 0; i < 5; i++) { const t0 = performance.now(); c = clashes(marks, events); best = Math.min(best, performance.now() - t0) }
  assert.ok(best < 20, `${best} ms`)
  assert.ok(c.count > 400)
})

test('the Day zoom: one day of the week shown, read a day either side; Coming up reads today and tomorrow', () => {
  withZone('America/Phoenix', () => {
    const now = local(2026, 10, 23, 13)
    const w = weekWindow(now)
    const z = zoomed(w, 2)
    assert.equal(z.days.length, 1)
    assert.equal(z.from_ms, local(2026, 10, 25))
    assert.equal(z.days[0].index, 0)
    assert.equal(zoomed(w, null), w)
    assert.deepEqual(readSpans(w, now), [{ from_ms: local(2026, 10, 23), to_ms: local(2026, 10, 30) }], 'this week holds today: one span')
    assert.deepEqual(readSpans(z, now, true), [{ from_ms: local(2026, 10, 23), to_ms: local(2026, 10, 27) }], 'a day either side, joined with today')
    const far = weekWindow(now, 10)
    assert.deepEqual(readSpans(far, now), [{ from_ms: far.from_ms, to_ms: far.to_ms }, { from_ms: local(2026, 10, 23), to_ms: local(2026, 10, 25) }], 'ten weeks on: two spans')
    assert.deepEqual(readSpans(null, now), [{ from_ms: local(2026, 10, 23), to_ms: local(2026, 10, 25) }], 'another lens: Coming up alone')
    for (const s of readSpans(weekWindow(now, -52), now)) assert.ok(s.to_ms - s.from_ms <= 62 * 86_400_000)
    assert.equal(zoomDay(w, local(2026, 10, 26, 9), now), 3, 'the focused mark’s day')
    assert.equal(zoomDay(w, null, now), 0, 'else today')
  })
})

test('j and k step the marks and events by time; the Horizon keys find the mark’s own verb', () => {
  withZone('America/Phoenix', () => {
    const { work, personal, secret } = twoCalendars()
    const w = weekWindow(local(2026, 10, 19, 8))
    const marks = [mark('m1', 'reminder', local(2026, 10, 23, 9)), mark('m2', 'run', local(2026, 10, 23, 16)), mark('m0', 'check', local(2026, 10, 1))]
    const order = timelineOrder(marks, calEvents([work, personal]), w)
    assert.deepEqual(order, ['m1', `ev:${secret.key}`, 'm2', 'ev:evk_p1'], 'by time; outside the window, not at all')
    assert.equal(stepOrder(order, null, 1), 'm1')
    assert.equal(stepOrder(order, null, -1), 'ev:evk_p1')
    assert.equal(stepOrder(order, 'm1', 1), `ev:${secret.key}`)
    assert.equal(stepOrder(order, 'ev:evk_p1', 1), 'ev:evk_p1', 'the end holds')
    assert.equal(stepOrder([], 'm1', 1), null)
    const rem = marks[0]
    assert.deepEqual([horizonVerb(rem, 'z')?.method, horizonVerb(rem, 'z')?.params], ['wake.snooze', { wake: 'wak_m1', when: '10m' }])
    assert.equal(horizonVerb(rem, 'p')?.method, 'wake.pause')
    assert.equal(horizonVerb({ ...rem, paused: true }, 'p')?.method, 'wake.resume')
    assert.equal(horizonVerb(rem, 'r'), null, 'a reminder does not run')
    assert.equal(horizonVerb(rem, 'x')?.method, 'wake.cancel')
    assert.equal(horizonVerb(marks[1], 'r')?.method, 'routine.run_now')
    assert.equal(horizonVerb(marks[1], 'z'), null)
    assert.equal(horizonVerb(mark('st', 'start', 1), 'x')?.method, 'task.cancel')
  })
})

test('the table holds the window’s marks and events by time, a sensitive event redacted as the strip is', () => {
  withZone('America/Phoenix', () => {
    const { work, personal } = twoCalendars()
    const w = weekWindow(local(2026, 10, 19, 8))
    const marks = [mark('m1', 'reminder', local(2026, 10, 23, 14, 10))]
    const events = calEvents([work, personal])
    const c = clashes(marks, events)
    const rows = tableRows(marks, events, w, true, c, (m) => `${m.title} · reminder`)
    assert.deepEqual(rows.map((r) => r.key), ['ev:evk_w1', 'm1', 'ev:evk_p1'])
    assert.deepEqual(rows.map((r) => r.clash), [true, true, false])
    const text = rows.map((r) => `${r.what} ${r.calendar} ${r.line}`).join('\n')
    for (const s of [SECRET.title, SECRET.location, String(SECRET.attendees)]) assert.ok(!text.includes(s), s)
    assert.match(text, /Lantern-maker pottery class · Studio Brambleworth/)
    assert.ok(tableRows(marks, events, w, false, c, (m) => m.title).some((r) => r.line.includes(SECRET.title)), 'off: the title shows')
  })
})

test('a mark on the fold’s edge opens no extra hour', () => {
  withZone('America/Phoenix', () => {
    const w = weekWindow(local(2026, 10, 19, 8))
    const at = (h: number, m = 0, kind: Mark['kind'] = 'reminder') => [mark('edge', kind, local(2026, 10, 22, h, m))]
    assert.deepEqual(nightFold(w, at(22), []), { early: 7, late: 22, count: 0 }, '22:00 sits on the edge line, drawn, not counted')
    assert.deepEqual(nightFold(w, at(7), []), { early: 7, late: 22, count: 0 }, '07:00 likewise')
    assert.deepEqual(nightFold(w, at(22, 30), []), { early: 7, late: 23, count: 0 }, '22:30 opens through 23:00 and is drawn')
    assert.deepEqual(nightFold(w, at(23, 30, 'run'), []), { early: 7, late: 22, count: 1 }, 'a series tick at 23:30 stays folded and is counted')
    assert.equal(yOf(22, { early: 7, late: 22 }, 20, 18), 18 + 15 * 20, 'the edge line')
  })
})

test('the first view centres on now when today is shown, else the first meeting or one-off mark, else 08:00 (item 8)', () => {
  withZone('America/Phoenix', () => {
    const w = weekWindow(local(2026, 10, 19, 8))
    const at = (h: number, kind: Mark['kind'] = 'reminder') => mark('c', kind, local(2026, 10, 22, h))
    assert.equal(centerHour(14.5, [9]), 14.5, 'now wins when today is shown')
    assert.equal(centerHour(null, []), 8, 'nothing: 08:00')
    assert.deepEqual(openHours(w, [at(13), at(6)], [{ top: 10 / 24 }]).sort((a, b) => a - b), [6, 10, 13])
    assert.equal(centerHour(null, openHours(w, [at(13), at(15)], [{ top: 10 / 24 }])), 10, 'the earliest')
    assert.deepEqual(openHours(w, [at(3, 'run'), at(4, 'past'), at(5, 'watch')], []), [], 'a series tick alone does not pick the hour')
    assert.equal(centerHour(null, openHours(w, [at(3, 'run')], [])), 8)
    assert.deepEqual(openHours(w, [mark('far', 'reminder', local(2026, 11, 30, 2))], []), [], 'outside the window')
  })
})

test('a meeting that began on an earlier day does not hold the week open at 00:00; one that starts at midnight does (theseus-ka0lv)', () => {
  withZone('America/Phoenix', () => {
    const w = weekWindow(local(2026, 10, 19, 8))
    const over = eventLayout(calEvents([cal('personal.cal.example.test', { events: [ev('late', local(2026, 10, 21, 23), local(2026, 10, 22, 1)), ev('nine', local(2026, 10, 24, 9), local(2026, 10, 24, 10))] })]), w).blocks
    assert.deepEqual(over.filter((b) => b.key === 'late').map((b) => b.continues), [false, true], 'the second day of an overnight event continues')
    assert.deepEqual(openHours(w, [], over).sort((a, b) => a - b), [9, 23], 'only starts count: 23:00 and 09:00')
    assert.equal(centerHour(null, openHours(w, [], over)), 9)
    const midnight = eventLayout(calEvents([cal('personal.cal.example.test', { events: [ev('zero', local(2026, 10, 22), local(2026, 10, 22, 1)), ev('long', local(2026, 10, 21, 20), local(2026, 10, 22, 6))] })]), w).blocks
    assert.deepEqual(midnight.filter((b) => b.key === 'zero').map((b) => b.continues), [false], 'starting exactly at 00:00 is a start, not a continuation')
    assert.deepEqual(openHours(w, [], midnight).sort((a, b) => a - b), [0, 20], 'the meeting at 00:00 opens 00:00; the long one only its 20:00 start')
  })
})

test('the scroll puts an hour in the middle of the box, inside the content (item 8)', () => {
  assert.equal(scrollFor(800, 400, 24 * 44), 600)
  assert.equal(scrollFor(100, 400, 24 * 44), 0, 'near the top: no negative scroll')
  assert.equal(scrollFor(1050, 400, 24 * 44), 24 * 44 - 400, 'near the bottom: the last screenful')
  assert.equal(scrollFor(10, 400, 300), 0, 'a box taller than its content')
  // The strip starts unfolded and its hours scroll inside it.
  const s = readFileSync(new URL('../src/ops/WeekStripV2.tsx', import.meta.url), 'utf8').replace(/^\s*\/\/.*$/gm, '')
  assert.match(s, /const \[foldNight, setFoldNight\] = useState\(false\)/, 'all 24 hours by default')
  assert.match(s, /ref=\{box\} className="overflow-auto[^"]*"[^>]*maxHeight/, 'the hours scroll inside the strip')
  assert.match(s, /sticky top-0/)
})
