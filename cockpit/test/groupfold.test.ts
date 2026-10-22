// The Ontology view's group folds (theseus-5kd37), in `src/lib/groupfold.ts`: a filter shows every group and leaves the
// reader's folds alone, and a fold button folds only without one. Run by `npm test` with node alone.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { openGroups, toggled } from '../src/lib/groupfold.ts'

const all = ['topic', 'person', 'resource']

test('under a filter every group is open and a fold button changes nothing', () => {
  const mine = new Set(['person'])
  assert.deepEqual([...openGroups(mine, null, true, all)].sort(), [...all].sort())
  assert.equal(toggled(mine, 'topic', true), mine, 'the reader’s own set, unchanged')
})

test('after the filter clears, the reader’s own open set returns, whatever was pressed under it', () => {
  let mine = new Set(['person'])
  for (const k of all) mine = toggled(mine, k, true)
  assert.deepEqual([...openGroups(mine, null, false, all)], ['person'])
})

test('without a filter a fold button folds an open group and opens a folded one, in the reader’s own set', () => {
  const mine = new Set(['person', 'topic'])
  assert.deepEqual([...toggled(mine, 'person', false)], ['topic'])
  assert.deepEqual([...toggled(mine, 'resource', false)].sort(), ['person', 'resource', 'topic'])
  assert.deepEqual([...mine].sort(), ['person', 'topic'], 'the set given is not changed')
})

test('the selected category’s group is open without being the reader’s', () => {
  const mine = new Set<string>()
  assert.deepEqual([...openGroups(mine, 'resource', false, all)], ['resource'])
  assert.equal(mine.size, 0)
})

test('the view takes its folds from groupfold and disables a fold button while filtering', () => {
  const v = readFileSync(new URL('../src/views/Ontology.tsx', import.meta.url), 'utf8')
  assert.match(v, /openGroups\(groupsOpen, /)
  assert.match(v, /toggled\(s, key, /)
  assert.match(v, /disabled=\{filtering\}/)
  assert.doesNotMatch(v, /new Set\(gopen\)/, 'the forced set is never copied into the reader’s')
})
