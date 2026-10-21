// The ontology pack's line (theseus-wzvxp, R7d; the learning report's, theseus-0vhyn, R7e): judgments, acts, confirms and
// undos counted from the ledger's `judge.call`, `onto.filed` and `ontology.undo` rows, in `src/lib/ontostats.ts`. Run by
// `npm test` with node alone. Invented rows.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { LedgerEntry } from '@protocol'
import { ONTO_PACK, ontoLine, ontoStatsOf } from '../src/lib/ontostats.ts'

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
  assert.equal(ontoStatsOf(ledger).undos, 1)
})

test('a filing row whose judgment the history does not hold is not counted, so the parts never exceed the whole', () => {
  const s = ontoStatsOf(ledger)
  assert.ok(s.acts + s.confirms <= s.judgments)
})

test('an empty ledger has nothing, and the line says so without a rate', () => {
  const s = ontoStatsOf([])
  assert.deepEqual([s.judgments, s.acts, s.confirms, s.undos], [0, 0, 0, 0])
  assert.equal(ontoLine(s), 'ontology: no judgments yet')
})

test('the line counts judgments, acts, confirms and undos, and the undo rate over the acts', () => {
  assert.equal(ontoLine(ontoStatsOf(ledger)), 'ontology: 7 judgments · 4 acts · 1 confirm · 1 undo (25% of acts)')
  const plural = ontoStatsOf([call('a', 1), filed('a', 1, { kind: 'topic', outcome: 'asked' }), call('b', 2), filed('b', 2, { kind: 'topic', outcome: 'asked' })])
  assert.equal(ontoLine(plural), 'ontology: 2 judgments · 0 acts · 2 confirms · 0 undos')
})

test('the Judgment view draws the line under the ontology pack’s row from the shared ledger history', () => {
  const v = readFileSync(new URL('../src/views/Judgment.tsx', import.meta.url), 'utf8')
  assert.match(v, /ontoStatsOf\(/)
  assert.match(v, /ontoLine\(/)
  assert.match(v, /useHistoryRows\(/)
  assert.match(v, /ONTO_PACK/)
})
