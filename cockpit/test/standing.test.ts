// The standing records' pure parts (`src/lib/standing.ts`), run by `npm test` with node's own runner.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { groupsOf, lapses, revokeQuestion, scopeWords, tallyWords } from '../src/lib/standing.ts'

const rec = (id: string, kind: string, scope: any, since_ms: number, state = 'live', extra: any = {}) =>
  ({ id, kind, text: `text of ${id}`, scope, since_ms, source: 'the CLI', state, compiles: 3, missing: 0, ...extra }) as any

test('a scope is said in words, a surface’s as where it speaks', () => {
  assert.equal(scopeWords({ kind: 'everywhere' }), 'everywhere')
  assert.equal(scopeWords({ kind: 'session', of: 'ses_a' }), 'session ses_a')
  assert.equal(scopeWords({ kind: 'place', of: 'discord:dm:7' }), 'place discord:dm:7')
  assert.equal(scopeWords({ kind: 'surface', of: 'terminal' }), 'on terminal')
})

test('the live records are grouped by kind in the model’s order, oldest first, and the ended ones are left out', () => {
  const groups = groupsOf([
    rec('std_c', 'fact', { kind: 'everywhere' }, 30),
    rec('std_b', 'order', { kind: 'surface', of: 'terminal' }, 20),
    rec('std_a', 'order', { kind: 'everywhere' }, 10),
    rec('std_x', 'order', { kind: 'everywhere' }, 5, 'revoked'),
  ])
  assert.deepEqual(groups.map((g) => g.title), ['Orders', 'Facts about the owner'])
  assert.deepEqual(groups[0].rows.map((r) => r.id), ['std_a', 'std_b'])
  assert.deepEqual(groupsOf([]), [])
})

test('a record that lapses says when, and a revoke asks with the record’s kind and scope', () => {
  const r = rec('std_a', 'preference', { kind: 'surface', of: 'voice' }, 1, 'live', { until_ms: Date.UTC(2026, 9, 20, 9, 30) })
  assert.equal(lapses(r, Date.UTC(2026, 9, 14)), 'lapses 2026-10-20 09:30 UTC')
  assert.equal(lapses(r, Date.UTC(2026, 9, 21)), 'lapsed')
  assert.equal(lapses(rec('std_b', 'order', { kind: 'everywhere' }, 1), 0), null)
  assert.equal(revokeQuestion(r), 'Revoke std_a (preference, on voice)? It leaves every prompt of its scope at the next turn.')
})

test('the tally counts the compiles that carried a record, and a surface’s has none to count', () => {
  const at = (extra: any, scope: any = { kind: 'everywhere' }) => rec('std_a', 'order', scope, 1, 'live', extra)
  assert.equal(tallyWords(at({ compiles: 0 })), 'not compiled yet')
  assert.equal(tallyWords(at({ compiles: 4, missing: 0 })), 'all 4 compiles carried it')
  assert.equal(tallyWords(at({ compiles: 4, missing: 1 })), '3 of 4 compiles carried it')
  assert.equal(tallyWords(at({}, { kind: 'surface', of: 'discord' })), 'in the tail of that surface’s turns')
})
