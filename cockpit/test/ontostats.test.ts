// The ontology pack's line (theseus-wzvxp, R7d; the learning report's, theseus-0vhyn, R7e): judgments, acts and confirms
// counted from the ledger's `judge.call` and `onto.filed` rows, in `src/lib/ontostats.ts`; the undo rate per kind is the
// stored report's (theseus-ukmbn, theseus-2tqqs), which the daemon's `onto_undo::undo_rates` computes and its tests hold
// (a distinct write, a sweep's not counted, a repeat once, a write of two kinds once for each), and the cockpit words.
// Run by `npm test` with node alone. Invented rows.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { LedgerEntry, UndoRate } from '@protocol'
import { ONTO_PACK, ontoKindWords, ontoLine, ontoStatsOf } from '../src/lib/ontostats.ts'

let pos = 0
const at = (daysAgo: number, hour = 9) => new Date(2026, 9, 20 - daysAgo, hour).getTime()
const row = (kind: string, when: number, data: unknown): LedgerEntry =>
  ({ position: ++pos, at_unix_ms: when, kind, session_id: 'ses_a', turn_id: null, data })
const call = (id: string, when: number, pack = ONTO_PACK, outcome = 'answered') => row('judge.call', when, { id, pack, version: 1, mode: 'live', outcome: { outcome } })
type Step = { kind: string; outcome: string; category?: string; confidence?: number }
const filed = (judgment: string, when: number, ...steps: Step[]) => row('onto.filed', when, { judgment, steps })
const undo = (write: string, when: number) => row('ontology.undo', when, { write, label: 'wrong', keys: ['k'], who: 'x', via: 'y' })

const ledger: LedgerEntry[] = [
  call('j1', at(3)), filed('j1', at(3), { kind: 'topic', outcome: 'filed', category: 'topic:kelp', confidence: 0.9 }),      // an act
  call('j2', at(3)), filed('j2', at(3), { kind: 'topic', outcome: 'filed', category: 'topic:tides', confidence: 0.9 }),     // an act, later undone
  call('j3', at(2)), filed('j3', at(2), { kind: 'topic', outcome: 'asked', confidence: 0.7 }),                              // a confirm
  call('j4', at(2)),                                                                                                       // below the confirm band: only its own row
  call('j5', at(1), ONTO_PACK, 'failed'),                                                                                  // failed: counted as a judgment, nothing more
  call('j6', at(1), 'security.v1'),                                                                                        // another pack's judgment
  call('j7', at(1)), filed('j7', at(1), { kind: 'topic', outcome: 'asked' }, { kind: 'project', outcome: 'attached', confidence: 0.95 }), // acts on one kind, asks on another: an act
  call('j8', at(1)), filed('j8', at(1), { kind: 'topic', outcome: 'asked' }), filed('j8', at(1), { kind: 'topic', outcome: 'filed' }), // asked, then filed: an act, not also a confirm
  undo('j2', at(1)),
  undo('j2', at(0)),                                                                                                       // a second undo of the same write counts once
  undo('s1', at(0)),                                                                                                       // a sweep's write: not this pack's
  filed('gone', at(0), { kind: 'topic', outcome: 'filed' }),                                                               // a filing with no judgment row in the history
]

test('the pack’s judgments are its judge.call rows, whatever their outcome, and no other pack’s', () => {
  assert.equal(ontoStatsOf(ledger).judgments, 7)
})

test('an act is a judgment that filed or attached; a confirm is one that only asked; nothing else is either', () => {
  const s = ontoStatsOf(ledger)
  assert.equal(s.acts, 4, 'j1, j2, j7 (it attached a project though it asked for a topic), and j8 (asked, then filed)')
  assert.equal(s.confirms, 1, 'j3 only asked')
})

test('an undo is a distinct write of the pack’s that was taken back; a sweep’s write and a repeat are not', () => {
  // That rule is the daemon's alone now (`onto_undo::undo_rates`): the day line counts no undo, so its rows change nothing.
  assert.deepEqual(ontoStatsOf(ledger), ontoStatsOf(ledger.filter((r) => r.kind !== 'ontology.undo')))
})

test('a filing row whose judgment the history does not hold is not counted, so the parts never exceed the whole', () => {
  const s = ontoStatsOf(ledger)
  assert.ok(s.acts + s.confirms <= s.judgments)
})

test('an empty ledger has nothing, and the line says so without a rate', () => {
  const s = ontoStatsOf([])
  assert.deepEqual([s.judgments, s.acts, s.confirms], [0, 0, 0])
  assert.equal(ontoLine(s), 'ontology: no judgments yet')
})

test('the line counts judgments, acts and confirms, the day’s activity, with no undo count or rate of its own', () => {
  assert.equal(ontoLine(ontoStatsOf(ledger)), 'ontology: 7 judgments · 4 acts · 1 confirm')
  const plural = ontoStatsOf([call('a', 1), filed('a', 1, { kind: 'topic', outcome: 'asked' }), call('b', 2), filed('b', 2, { kind: 'topic', outcome: 'asked' })])
  assert.equal(ontoLine(plural), 'ontology: 2 judgments · 0 acts · 2 confirms')
})

test('the Judgment view draws the line under the ontology pack’s row from the shared ledger history', () => {
  const v = readFileSync(new URL('../src/views/Judgment.tsx', import.meta.url), 'utf8')
  assert.match(v, /ontoStatsOf\(/)
  assert.match(v, /ontoLine\(/)
  assert.match(v, /useHistoryRows\(/)
  assert.match(v, /ONTO_PACK/)
})

// ---------------------------------------------------------------- the learning report's line (theseus-0vhyn, R7e)

const two: LedgerEntry[] = [
  call('k1', at(1)), filed('k1', at(1), { kind: 'topic', outcome: 'filed' }),
  call('k2', at(1)), filed('k2', at(1), { kind: 'topic', outcome: 'filed' }, { kind: 'person', outcome: 'filed' }),        // acts on two kinds: one act of each
  call('k3', at(1)), filed('k3', at(1), { kind: 'project', outcome: 'attached' }),
  call('k4', at(1)), filed('k4', at(1), { kind: 'topic', outcome: 'asked' }),                                              // asked: no act of any kind
  undo('k2', at(0)),
]

// What the daemon stores for `two` (`onto_undo::undo_rates`): k2's write is one of each kind it wrote, and k3's project
// attach is no write (it carries no undo row, so `ontology.undo` cannot take it back).
const stored: UndoRate[] = [
  { kind: 'person', writes: 1, undone: 1, rate: 1 },
  { kind: 'topic', writes: 2, undone: 1, rate: 0.5 },
]

test('each kind’s acts and undos are counted, a write that acted on two kinds once for each', () => {
  const s = ontoStatsOf(two)
  assert.equal(ontoKindWords(stored, '2026-10-20'), "undo rate by kind (Jev's writes, 7 days to 2026-10-20): person 1 of 1 (100%) · topic 1 of 2 (50%)")
  assert.match(ontoKindWords(stored, '2026-10-20', 3), /^undo rate by kind \(Jev's writes, 3 days to 2026-10-20\)/, 'the window is the report’s')
  assert.equal(s.acts, 3, 'the writes are 3 though the kind acts are 4')
  assert.ok(!('kinds' in s) && !('undos' in s), 'the cockpit keeps no tally of its own')
})

test('a kind whose acts were never taken back has a rate of zero, and a ledger with no acts has no kinds', () => {
  assert.equal(ontoKindWords([{ kind: 'topic', writes: 3, undone: 0, rate: 0 }], '2026-10-20'), "undo rate by kind (Jev's writes, 7 days to 2026-10-20): topic 0 of 3 (0%)")
  assert.equal(ontoKindWords([], '2026-10-20'), '')
})

test('the report’s day keeps the judgments made that local day, with their filings whenever they came', () => {
  const day = (daysAgo: number) => `2026-10-${String(20 - daysAgo).padStart(2, '0')}`
  const a = ontoStatsOf(ledger, { day: day(3) })
  assert.deepEqual([a.judgments, a.acts, a.confirms], [2, 2, 0], 'j1 and j2')
  const b = ontoStatsOf(ledger, { day: day(2) })
  assert.deepEqual([b.judgments, b.acts, b.confirms], [2, 0, 1])
  assert.equal(ontoStatsOf(ledger, { day: '2025-01-01' }).judgments, 0)
  assert.equal(ontoStatsOf(ledger, {}).judgments, 7, 'no day: the whole ledger')
})

test('the undo rate per kind reads as words, with its window, kinds as the report holds them, and nothing when there were no acts', () => {
  assert.equal(ontoKindWords(stored, '2026-10-21'), "undo rate by kind (Jev's writes, 7 days to 2026-10-21): person 1 of 1 (100%) · topic 1 of 2 (50%)")
  assert.equal(ontoKindWords([], '2026-10-21'), '')
})

test('the learning report draws the pack’s line from the ledger for the report’s day, and the rate by kind from the report', () => {
  const v = readFileSync(new URL('../src/components/LearningReport.tsx', import.meta.url), 'utf8')
  assert.match(v, /ontoStatsOf\([^)]*day: date/)
  assert.match(v, /ontoLine\(/)
  assert.match(v, /ontoKindWords\(p\.undo \?\? \[\], date, p\.undo_days\)/)
  assert.match(v, /p\.pack === ONTO_PACK/)
})
