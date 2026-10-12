// The nav rail's accordion groups (theseus-onxwh): every view of the rail is in exactly one group (so each is one
// click from an open group), the group of the current view opens on load, and the operator's own folds are kept.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { entered, groupOf, isOpen, NAV_GROUPS, parseChoices, pressed } from '../src/lib/navgroups.ts'

const shell = readFileSync(new URL('../src/components/Shell.tsx', import.meta.url), 'utf8')
const railViews = [...shell.slice(shell.indexOf('const NAV = ['), shell.indexOf('] as const')).matchAll(/to: '([^']+)'/g)].map((m) => m[1])
const none = new Set<string>()

test('every view of the rail is in exactly one group, and every group item is a view of the rail', () => {
  assert.ok(railViews.length >= 16, `${railViews.length} views in the rail`)
  const placed = NAV_GROUPS.flatMap((g) => g.items)
  assert.deepEqual([...placed].sort(), [...railViews].sort())
  assert.equal(new Set(placed).size, placed.length, 'no view in two groups')
  assert.ok(NAV_GROUPS.length >= 4 && NAV_GROUPS.length <= 8, 'a few named groups')
  for (const g of NAV_GROUPS) assert.ok(g.label.length > 2 && g.items.length >= 2, g.id)
})

test('the group holding the current view opens on load, the others stay folded', () => {
  for (const to of railViews) {
    const x = entered(none, none, to)
    const g = groupOf(to)
    assert.ok(g, `${to} is in a group`)
    for (const group of NAV_GROUPS) assert.equal(isOpen({}, x.shut, x.visited, group.id), group.id === g, `${to}: ${group.id}`)
  }
})

test('a view below a rail item, and a session’s deck, belong to a group', () => {
  assert.equal(groupOf('/benchmarks/some-report'), 'bench')
  assert.equal(groupOf('/session/ses_abc'), 'live')
  assert.equal(groupOf('/'), 'live')
  assert.equal(groupOf('/nowhere'), null)
})

test('a press folds or opens a group, is kept, and the page entering a view reopens its group', () => {
  let v = entered(none, none, '/money')
  assert.equal(isOpen({}, v.shut, v.visited, 'money'), true)
  // Fold the open one: kept as folded, and shut for this page.
  let p = pressed({}, v.shut, v.visited, 'money')
  assert.deepEqual(p.choices, { money: false })
  assert.equal(isOpen(p.choices, p.shut, v.visited, 'money'), false)
  // Open another: kept as open, and it stays open after the current view moves elsewhere.
  p = pressed(p.choices, p.shut, v.visited, 'memory')
  assert.deepEqual(p.choices, { money: false, memory: true })
  v = entered(p.shut, v.visited, '/ledger')
  assert.equal(isOpen(p.choices, v.shut, v.visited, 'memory'), true)
  assert.equal(isOpen(p.choices, v.shut, v.visited, 'system'), true)
  assert.equal(isOpen(p.choices, v.shut, v.visited, 'money'), false, 'folded by the operator, and the view is not in it')
  // Going back into the folded group opens it again.
  v = entered(v.shut, v.visited, '/economics')
  assert.equal(isOpen(p.choices, v.shut, v.visited, 'money'), true)
})

test('what the browser kept is read defensively: only known groups with true or false', () => {
  assert.deepEqual(parseChoices(null), {})
  assert.deepEqual(parseChoices('not json'), {})
  assert.deepEqual(parseChoices('[1,2]'), {})
  assert.deepEqual(parseChoices(JSON.stringify({ memory: true, money: false, nonsense: true, safety: 'yes' })), { memory: true, money: false })
  const kept = pressed({}, none, none, 'safety').choices
  assert.deepEqual(parseChoices(JSON.stringify(kept)), kept)
})

test('the shortcuts still reach views in a group, and the rail renders every group with its items', () => {
  const go = [...shell.matchAll(/(\w): '(\/[a-z]+)'/g)].filter((m) => shell.slice(shell.indexOf('const GO'), shell.indexOf('export function Shell')).includes(m[0])).map((m) => m[2])
  assert.ok(go.length >= 10)
  for (const to of go) assert.ok(groupOf(to), `${to} has a group`)
  assert.match(shell, /NAV_GROUPS\.map/)
  assert.match(shell, /aria-expanded=\{open\}/)
})
