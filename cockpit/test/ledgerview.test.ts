// The Ledger view's pure parts (`src/lib/ledgerview.ts`, M7 42b), run by `npm test`.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { applySaved, exportName, exportOf, filterOf, filterRows, newerThan, queryOf, readSaved, withoutSaved, withSaved, writeSaved } from '../src/lib/ledgerview.ts'

const row = (position: number, kind: string, at: number, session: string | null = 'ses_a', data: unknown = {}) =>
  ({ position, kind, at_unix_ms: at, session_id: session, turn_id: null, data }) as any
const rows = [
  row(1, 'turn.started', 100), row(2, 'tool.called', 200, 'ses_a', { tool: 'fs.read' }), row(3, 'tool.completed', 300, 'ses_b'),
  row(4, 'provider.call', 400, 'ses_a', { model: 'm' }), row(5, 'budget.reset', 500, 'ses_b', { by: 'cli' }),
]
const f = (q: string) => filterOf(new URLSearchParams(q))

test('the filter shows the newest first, by kind, family, session, range, and search', () => {
  assert.deepEqual(filterRows(rows, f('')).map((r) => r.position), [5, 4, 3, 2, 1])
  assert.deepEqual(filterRows(rows, f('kind=tool.called,provider.call')).map((r) => r.position), [4, 2])
  assert.deepEqual(filterRows(rows, f('family=tool')).map((r) => r.position), [3, 2])
  assert.deepEqual(filterRows(rows, f('session=ses_b')).map((r) => r.position), [5, 3])
  assert.deepEqual(filterRows(rows, f('from=200&to=400')).map((r) => r.position), [4, 3, 2])
  assert.deepEqual(filterRows(rows, f('q=FS.READ')).map((r) => r.position), [2])
  assert.deepEqual(filterRows(rows, f('session=ses_a&family=tool&q=fs')).map((r) => r.position), [2])
})

test('the export is exactly the rows shown, in the order shown', () => {
  const filter = f('session=ses_b')
  const shown = filterRows(rows, filter)
  assert.deepEqual(JSON.parse(exportOf(rows, filter)), shown)
  assert.equal(JSON.parse(exportOf(rows, filter)).length, 2)
  assert.notEqual(JSON.parse(exportOf(rows, filter)).length, rows.length)
  assert.equal(JSON.parse(exportOf(rows, f(''))).length, rows.length)
  assert.equal(exportName(2, Date.UTC(2026, 9, 5, 3, 30, 0)), 'ledger-2-rows-2026-10-05T03-30-00.json')
})

test('a saved filter round-trips through the browser and back into the address', () => {
  const p = new URLSearchParams('view=nodes&follow=1&q=fs&kind=tool.called&session=ses_a&from=1&to=9&row=5')
  const q = queryOf(p)
  assert.equal(q, 'q=fs&kind=tool.called&session=ses_a&from=1&to=9')
  const saved = withSaved(withSaved([], 'my tools', q), 'other', 'family=tool')
  const back = readSaved(writeSaved(saved))
  assert.deepEqual(back, saved)
  // Applied over an address, the filter is replaced and the view stays.
  const next = applySaved(new URLSearchParams('follow=1&family=judge&q=old'), back[0])
  assert.equal(next.get('follow'), '1')
  assert.equal(next.get('family'), null)
  assert.equal(next.get('q'), 'fs')
  assert.deepEqual(filterRows(rows, filterOf(next)).map((r) => r.position), [])
  assert.equal(queryOf(next), q)
  // The same name replaces; a blank name adds nothing; a delete removes it.
  assert.deepEqual(withSaved(saved, 'other', 'q=x')[1], { name: 'other', query: 'q=x' })
  assert.equal(withSaved(saved, '  ', 'q=x').length, 2)
  assert.deepEqual(withoutSaved(saved, 'other').map((s) => s.name), ['my tools'])
})

test('what the browser held is read back, or dropped, never thrown', () => {
  assert.deepEqual(readSaved(null), [])
  assert.deepEqual(readSaved('{not json'), [])
  assert.deepEqual(readSaved('{"a":1}'), [])
  assert.deepEqual(readSaved('[{"name":"a","query":"q=1"},{"name":3},null,"x"]'), [{ name: 'a', query: 'q=1' }])
})

test('follow counts the rows that landed after the list paused', () => {
  const shown = filterRows(rows, f(''))
  assert.equal(newerThan(shown, 5), 0)
  assert.equal(newerThan(shown, 3), 2)
  assert.equal(newerThan(shown, 0), 5)
  assert.equal(newerThan([], 0), 0)
})
