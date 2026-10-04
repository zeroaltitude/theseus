// The prompt picker's pure parts (`src/lib/prompts.ts`), run by `npm test` with node's own runner.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { fieldLabel, missingRequired, promptRef, sortedPrompts } from '../src/lib/prompts.ts'

const greet = {
  server: 'fake', prompt: 'greet', name: 'fake/greet', digest: 'd1', stored: false,
  arguments: [{ name: 'name', required: true }, { name: 'tone', required: false }],
} as any

test('a required argument left empty is named, and a filled one is not', () => {
  assert.deepEqual(missingRequired(greet, {}), ['name'])
  assert.deepEqual(missingRequired(greet, { name: '  ' }), ['name'])
  assert.deepEqual(missingRequired(greet, { name: 'Ada' }), [])
  assert.deepEqual(missingRequired({ ...greet, arguments: [] }, {}), [])
})

test('the prompt sent names its server and prompt, and carries only the arguments filled in', () => {
  assert.deepEqual(promptRef(greet, { name: 'Ada', tone: '' }), { server: 'fake', name: 'greet', arguments: { name: 'Ada' } })
  assert.deepEqual(promptRef(greet, { name: 'Ada', tone: 'dry', stray: 'x' }).arguments, { name: 'Ada', tone: 'dry' })
})

test('a required field is marked, and the list is in name order', () => {
  assert.equal(fieldLabel({ name: 'name', required: true }), 'name *')
  assert.equal(fieldLabel({ name: 'tone', required: false }), 'tone')
  const names = sortedPrompts([{ ...greet, name: 'z/a' }, { ...greet, name: 'a/b' }]).map((p) => p.name)
  assert.deepEqual(names, ['a/b', 'z/a'])
})
