// The Ship's flare for a failure (`src/ship/flares.ts`, theseus-1skt), run by `npm test`: a failure flares its ship
// once, whichever of its push, its execution's change and its row in the page's one copy of the ledger tells the page
// first; a row from before the page opened is history.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { isStoppingFailure, flareOf, flaresOfRows, newFlares, SAME_FAILURE_MS } from '../src/ship/flares.ts'

/** When the page opened (ms). */
const OPENED = 1_000_000
const row = (kind: string, sid: string | null, turn: string | null, at: number) => ({ kind, session_id: sid, turn_id: turn, at_unix_ms: at })
const failed = (sid: string, turn: string, at: number) => row('turn.failed', sid, turn, at)

test('a failure only the ledger tells (a session the Ship does not watch) flares its ship from its row', () => {
  const f = newFlares()
  // A model the daemon cannot price fails before the turn runs: no push reaches the page, its row does.
  assert.deepEqual(flaresOfRows(f, [failed('s9', 't1', OPENED + 5000)], OPENED, OPENED + 7000), ['s9'])
  // Other rows flare nothing: a turn that ended, a failed call, an execution's own row (its push reaches every page),
  // a row with no session.
  const others = [row('turn.ended', 's8', 't2', OPENED + 6000), row('action.failed', 's8', 't2', OPENED + 6000),
    row('execution.failed', 's8', null, OPENED + 6000), row('turn.failed', null, 't3', OPENED + 6000)]
  assert.deepEqual(flaresOfRows(f, others, OPENED, OPENED + 8000), [])
})

test('one failure the push and the ledger both tell flares once, whichever comes first', () => {
  const f = newFlares()
  // The push first (a session the Ship watches): its row, a few seconds later, is the same failure.
  assert.equal(flareOf(f, { session: 's1', turn: 't1', heard: OPENED + 1000 }), true)
  assert.deepEqual(flaresOfRows(f, [failed('s1', 't1', OPENED + 990)], OPENED, OPENED + 3500), [])
  // The row first: the push after it is the same failure too.
  assert.deepEqual(flaresOfRows(f, [failed('s2', 't2', OPENED + 4000)], OPENED, OPENED + 6000), ['s2'])
  assert.equal(flareOf(f, { session: 's2', turn: 't2', heard: OPENED + 6100 }), false)
  // Another failed turn of the same session is another failure, however soon: it flares, and its row then doesn't.
  assert.equal(flareOf(f, { session: 's1', turn: 't3', heard: OPENED + 1500 }), true)
  assert.deepEqual(flaresOfRows(f, [failed('s1', 't3', OPENED + 1490), failed('s1', 't4', OPENED + 9000)], OPENED, OPENED + 11_000), ['s1'])
})

test('a row from before the page opened is history, not news', () => {
  const f = newFlares()
  // The first walk reads the whole ledger: the failures in it from before the page opened stay still.
  const walk = [failed('s1', 't1', OPENED - 60_000), failed('s2', 't2', OPENED - 1), failed('s3', 't3', OPENED)]
  assert.deepEqual(flaresOfRows(f, walk, OPENED, OPENED + 2500), ['s3'])
  assert.deepEqual(flaresOfRows(f, [failed('s1', 't9', OPENED - 5)], OPENED, OPENED + 5000), [])
})

test("an execution's change names no turn: it and its turn's push or row are one failure", () => {
  const f = newFlares()
  // A task fails in a session the Ship does not watch: its execution's change reaches every page, its row later.
  assert.equal(flareOf(f, { session: 'task1', heard: OPENED + 1000, at: OPENED + 990 }), true)
  assert.deepEqual(flaresOfRows(f, [failed('task1', 't1', OPENED + 985)], OPENED, OPENED + 3500), [])
  // Judged on the daemon's clock: a row read long after (a slow follow) is still the change's failure.
  assert.equal(flareOf(f, { session: 'task2', heard: OPENED + 1000, at: OPENED + 990 }), true)
  assert.deepEqual(flaresOfRows(f, [failed('task2', 't2', OPENED + 985)], OPENED, OPENED + 45_000), [])
  // Its turn's push and its change, a moment apart, in either order: once.
  assert.equal(flareOf(f, { session: 'task3', turn: 't3', heard: OPENED + 2000 }), true)
  assert.equal(flareOf(f, { session: 'task3', heard: OPENED + 2004, at: OPENED + 1999 }), false)
  assert.equal(flareOf(f, { session: 'task4', heard: OPENED + 2000, at: OPENED + 1995 }), true)
  assert.equal(flareOf(f, { session: 'task4', turn: 't4', heard: OPENED + 2003 }), false)
  // Once a source names its turn, another failed turn of the session flares again; so does a failure long after.
  assert.equal(flareOf(f, { session: 'task4', turn: 't5', heard: OPENED + 2500 }), true)
  const later = SAME_FAILURE_MS + 5000
  assert.equal(flareOf(f, { session: 'task1', heard: OPENED + 1000 + later, at: OPENED + 990 + later }), true)
})

test("the Ship hears the ledger copy's new rows from when it opened: its flare, and its horn", () => {
  const data = readFileSync(new URL('../src/ship/useShipData.ts', import.meta.url), 'utf8')
  assert.match(data, /const since = Date\.now\(\)\s+return onNewRows\(\(rows\) => \{[\s\S]{0,120}?flaresOfRows\(flares, rows, since, now\)[\s\S]{0,120}?failedAt/,
    'the ledger copy flares the ship, from when it opened')
  assert.match(data, /case 'turn\.failed':[\s\S]{0,120}?const flare = !isStoppingFailure\(p\) && flareOf\(flares, [\s\S]{0,240}?failedAt: flare \? withMap/,
    'the push flares once with the row')
  assert.match(data, /case 'execution\.changed':[\s\S]{0,240}?&& flareOf\(flares, [\s\S]{0,400}?failedAt: failed \? withMap/,
    "the execution's change flares once with the row")
  const sound = readFileSync(new URL('../src/ship/useShipSound.ts', import.meta.url), 'utf8')
  assert.match(sound, /onNewRows\(\(rows\) => \{ for \(const r of rows\) sound\(cueOfRow\(ear, r, Date\.now\(\), since\)\)/, 'the horn hears the same rows')
})

test('a turn the stop held back (provider:stopping) flares and sounds nothing', () => {
  const f = newFlares()
  const held = { ...failed('s5', 't9', OPENED + 5000), data: { reason: 'provider:stopping' } }
  assert.deepEqual(flaresOfRows(f, [held], OPENED, OPENED + 6000), [])
  assert.deepEqual(flaresOfRows(f, [failed('s5', 't10', OPENED + 5000)], OPENED, OPENED + 6000), ['s5'])
  assert.equal(isStoppingFailure({ class: 'stopping' }), true)
  assert.equal(isStoppingFailure({ reason: 'unpriced: x' }), false)
})
