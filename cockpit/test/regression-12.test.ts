import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
const src = (p: string) => readFileSync(new URL('../src/' + p, import.meta.url), 'utf8')
test('rehome sheet uses the original-parent budget explanation', () => {
  assert.match(src('ops/TaskSheets.tsx'), /note=\{budgetWords\(/)
})
