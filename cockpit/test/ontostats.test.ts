// The ontology pack's line (theseus-wzvxp, R7d; the learning report's, theseus-0vhyn, R7e): judgments, acts and confirms
// counted from the ledger's `judge.call` and `onto.filed` rows, in `src/lib/ontostats.ts`; the undo rate per kind is the
// stored report's (theseus-ukmbn, theseus-2tqqs), which the daemon's `onto_undo::undo_rates` computes and its tests hold
// (a distinct write, a sweep's not counted, a repeat once, a write of two kinds once for each), and the cockpit words.
// Run by `npm test` with node alone. Invented rows. Each fixture names its pack version as a literal: the cockpit reads
// `onto.v1` and `onto.v2` (issue 46), each for the rows that name it, so a fixture that took the wired constant would
// silently move its history to the new version.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { LedgerEntry, UndoRate } from '@protocol'
import { ontoKindWords, ontoLine, ontoStatsOf } from '../src/lib/ontostats.ts'

let pos = 0
const at = (daysAgo: number, hour = 9) => new Date(2026, 9, 20 - daysAgo, hour).getTime()
const row = (kind: string, when: number, data: unknown): LedgerEntry =>
  ({ position: ++pos, at_unix_ms: when, kind, session_id: 'ses_a', turn_id: null, data })
const V1 = 'onto.v1'
const call = (id: string, when: number, pack: string = V1, outcome = 'answered') =>
  row('judge.call', when, { id, pack, version: Number(pack.split('.v')[1]) || 1, mode: 'live', outcome: { outcome } })
type Step = { kind: string; outcome: string; category?: string; confidence?: number }
const filed = (judgment: string, when: number, ...steps: Step[]) => row('onto.filed', when, { judgment, steps })
const undo = (write: string, when: number) => row('ontology.undo', when, { write, label: 'wrong', keys: ['k'], who: 'x', via: 'y' })

const ledger: LedgerEntry[] = [
  call('j1', at(3)), filed('j1', at(3), { kind: 'topic', outcome: 'filed', category: 'topic:kelp', confidence: 0.9 }),      // an act
  call('j2', at(3)), filed('j2', at(3), { kind: 'topic', outcome: 'filed', category: 'topic:tides', confidence: 0.9 }),     // an act, later undone
  call('j3', at(2)), filed('j3', at(2), { kind: 'topic', outcome: 'asked', confidence: 0.7 }),                              // a confirm
  call('j4', at(2)),                                                                                                       // below the confirm band: only its own row
  call('j5', at(1), V1, 'failed'),                                                                                         // failed: counted as a judgment, nothing more
  call('j6', at(1), 'security.v1'),                                                                                        // another pack's judgment
  call('j7', at(1)), filed('j7', at(1), { kind: 'topic', outcome: 'asked' }, { kind: 'project', outcome: 'attached', confidence: 0.95 }), // acts on one kind, asks on another: an act
  call('j8', at(1)), filed('j8', at(1), { kind: 'topic', outcome: 'asked' }), filed('j8', at(1), { kind: 'topic', outcome: 'filed' }), // asked, then filed: an act, not also a confirm
  undo('j2', at(1)),
  undo('j2', at(0)),                                                                                                       // a second undo of the same write counts once
  undo('s1', at(0)),                                                                                                       // a sweep's write: not this pack's
  filed('gone', at(0), { kind: 'topic', outcome: 'filed' }),                                                               // a filing with no judgment row in the history
]

test('the pack’s judgments are its judge.call rows, whatever their outcome, and no other pack’s', () => {
  assert.equal(ontoStatsOf(ledger, { pack: V1 })!.judgments, 7)
})

test('an act is a judgment that filed or attached; a confirm is one that only asked; nothing else is either', () => {
  const s = ontoStatsOf(ledger, { pack: V1 })!
  assert.equal(s.acts, 4, 'j1, j2, j7 (it attached a project though it asked for a topic), and j8 (asked, then filed)')
  assert.equal(s.confirms, 1, 'j3 only asked')
})

test('an undo is a distinct write of the pack’s that was taken back; a sweep’s write and a repeat are not', () => {
  // That rule is the daemon's alone now (`onto_undo::undo_rates`): the day line counts no undo, so its rows change nothing.
  assert.deepEqual(ontoStatsOf(ledger, { pack: V1 })!, ontoStatsOf(ledger.filter((r) => r.kind !== 'ontology.undo'), { pack: V1 }))
})

test('a filing row whose judgment the history does not hold is not counted, so the parts never exceed the whole', () => {
  const s = ontoStatsOf(ledger, { pack: V1 })!
  assert.ok(s.acts + s.confirms <= s.judgments)
})

test('an empty ledger has nothing, and the line says so without a rate', () => {
  const s = ontoStatsOf([], { pack: V1 })!
  assert.deepEqual([s.judgments, s.acts, s.confirms], [0, 0, 0])
  assert.equal(ontoLine(s), 'ontology: no judgments yet')
})

test('the line counts judgments, acts and confirms, the day’s activity, with no undo count or rate of its own', () => {
  assert.equal(ontoLine(ontoStatsOf(ledger, { pack: V1 })!), 'ontology: 7 judgments · 4 acts · 1 confirm')
  const plural = ontoStatsOf([call('a', 1), filed('a', 1, { kind: 'topic', outcome: 'asked' }), call('b', 2), filed('b', 2, { kind: 'topic', outcome: 'asked' })], { pack: V1 })!
  assert.equal(ontoLine(plural), 'ontology: 2 judgments · 0 acts · 2 confirms')
})

test('the Judgment view draws the line under the ontology pack’s row from the shared ledger history', () => {
  const v = readFileSync(new URL('../src/views/Judgment.tsx', import.meta.url), 'utf8')
  assert.match(v, /ontoStatsOf\(/)
  assert.match(v, /ontoLine\(/)
  assert.match(v, /useHistoryRows\(/)
  assert.match(v, /ONTO_READS/)
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
  const s = ontoStatsOf(two, { pack: V1 })!
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
  const a = ontoStatsOf(ledger, { pack: V1, day: day(3) })!
  assert.deepEqual([a.judgments, a.acts, a.confirms], [2, 2, 0], 'j1 and j2')
  const b = ontoStatsOf(ledger, { pack: V1, day: day(2) })!
  assert.deepEqual([b.judgments, b.acts, b.confirms], [2, 0, 1])
  assert.equal(ontoStatsOf(ledger, { pack: V1, day: '2025-01-01' })!.judgments, 0)
  assert.equal(ontoStatsOf(ledger, { pack: V1 })!.judgments, 7, 'no day: the whole ledger')
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
  assert.match(v, /ontoStatsOf\(rows, \{ pack: p\.pack, day: date \}\)/, 'the line is the version the report names')
})

// ---------------------------------------------------------------- each version's own activity (issue 46)
// The wired version moved from onto.v1 to onto.v2. What onto.v1 judged is still the ledger's: each version's activity is
// read for the version its rows name, not folded into the wired one's, and not counted twice.

const counts = (s: ReturnType<typeof ontoStatsOf>) => (s ? [s.judgments, s.acts, s.confirms] : null)
const day = (daysAgo: number) => `2026-10-${String(20 - daysAgo).padStart(2, '0')}`

// The day of the upgrade: onto.v1 judged the morning, onto.v2 the afternoon.
const mixed: LedgerEntry[] = [
  call('m1', at(5, 8), 'onto.v1'), filed('m1', at(5, 8), { kind: 'topic', outcome: 'filed' }),
  call('m2', at(5, 9), 'onto.v1'), filed('m2', at(5, 9), { kind: 'topic', outcome: 'asked' }),
  call('m3', at(5, 10), 'onto.v1'),
  call('m4', at(5, 15), 'onto.v2'), filed('m4', at(5, 15), { kind: 'project', outcome: 'attached' }),
  call('m5', at(5, 16), 'onto.v2'),
]

test('the judgments onto.v1 made are read as onto.v1’s, though the wired version is onto.v2', () => {
  const old: LedgerEntry[] = [
    call('o1', at(9), 'onto.v1'), filed('o1', at(9), { kind: 'topic', outcome: 'filed' }),
    call('o2', at(9), 'onto.v1'), filed('o2', at(9), { kind: 'topic', outcome: 'asked' }),
  ]
  assert.deepEqual(counts(ontoStatsOf(old, { pack: 'onto.v1' })), [2, 1, 1])
  assert.deepEqual(counts(ontoStatsOf(old, { pack: 'onto.v2' })), [0, 0, 0], 'the wired version claims none of them')
})

test('onto.v2’s judgments are onto.v2’s alone, and onto.v1 claims none of them', () => {
  const now: LedgerEntry[] = [
    call('n1', at(1), 'onto.v2'), filed('n1', at(1), { kind: 'project', outcome: 'attached' }),
    call('n2', at(1), 'onto.v2'),
  ]
  assert.deepEqual(counts(ontoStatsOf(now, { pack: 'onto.v2' })), [2, 1, 0])
  assert.deepEqual(counts(ontoStatsOf(now, { pack: 'onto.v1' })), [0, 0, 0])
})

test('a day both versions judged gives each its own counts, which together are the day’s and never twice', () => {
  const v1 = ontoStatsOf(mixed, { pack: 'onto.v1', day: day(5) })
  const v2 = ontoStatsOf(mixed, { pack: 'onto.v2', day: day(5) })
  assert.deepEqual(counts(v1), [3, 1, 1], 'm1 acted, m2 asked, m3 only its row')
  assert.deepEqual(counts(v2), [2, 1, 0], 'm4 attached a project')
  assert.equal(v1!.judgments + v2!.judgments, mixed.filter((r) => r.kind === 'judge.call').length)
  assert.equal(ontoLine(v1!), 'ontology: 3 judgments · 1 act · 1 confirm')
  assert.equal(ontoLine(v2!), 'ontology: 2 judgments · 1 act · 0 confirms')
})

test('only onto.v1 and onto.v2 are read: a learned version, or another pack, has no ontology line', () => {
  const rows: LedgerEntry[] = [call('l1', at(1), 'onto.v3'), filed('l1', at(1), { kind: 'topic', outcome: 'filed' }), call('s1', at(1), 'security.v1')]
  assert.equal(ontoStatsOf(rows, { pack: 'onto.v3' }), null, 'a learned version’s rows mean what its lineage decides; not read here')
  assert.equal(ontoStatsOf(rows, { pack: 'security.v1' }), null)
})

test('an old learning report whose pack is onto.v1 keeps its ontology activity line', () => {
  // The report stored for the day before the upgrade names onto.v1; its block reads the rows for that version and day.
  const old: LedgerEntry[] = [...mixed, call('p1', at(6), 'onto.v1'), filed('p1', at(6), { kind: 'person', outcome: 'filed' })]
  assert.equal(ontoLine(ontoStatsOf(old, { pack: 'onto.v1', day: day(6) })!), 'ontology: 1 judgment · 1 act · 0 confirms')
  assert.equal(ontoLine(ontoStatsOf(old, { pack: 'onto.v2', day: day(6) })!), 'ontology: no judgments yet')
  const v = readFileSync(new URL('../src/components/LearningReport.tsx', import.meta.url), 'utf8')
  assert.match(v, /ontoStatsOf\(rows, \{ pack: p\.pack, day: date \}\)/, 'each report block reads its own version')
  assert.doesNotMatch(v, /p\.pack === ONTO_PACK/, 'not only the wired version’s block')
})

test('the time machine on an old day shows onto.v1’s activity under onto.v1’s own row', () => {
  // As of the old day's noon, only onto.v1 had judged; as of its end, both had, each under its own row.
  const asOf = (t: number) => mixed.filter((r) => r.at_unix_ms <= t)
  assert.deepEqual(counts(ontoStatsOf(asOf(at(5, 12)), { pack: 'onto.v1' })), [3, 1, 1])
  assert.deepEqual(counts(ontoStatsOf(asOf(at(5, 12)), { pack: 'onto.v2' })), [0, 0, 0])
  assert.deepEqual(counts(ontoStatsOf(asOf(at(5, 23)), { pack: 'onto.v2' })), [2, 1, 0])
  const v = readFileSync(new URL('../src/views/Judgment.tsx', import.meta.url), 'utf8')
  assert.match(v, /ONTO_READS\.map\(\(v\) => \[v, ontoStatsOf\(asOf, \{ pack: v \}\)/, 'a line per version, as of the moment')
  assert.match(v, /onto\.get\(p\)/, 'drawn under the row of the version it counts')
  assert.match(v, /ONTO_READS\.filter\(\(v\) => onto\.get\(v\)/, 'a version with activity has its row, though the list’s window holds none of it')
  assert.doesNotMatch(v, /p === ONTO_PACK/, 'not only under the wired version’s row')
})
