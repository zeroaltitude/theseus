import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
const src = (p: string) => readFileSync(new URL('../src/' + p, import.meta.url), 'utf8')
test('historical boundaries disable both answers and refuse at the action boundary', () => {
  const s = src('views/Boundaries.tsx'); assert.match(s, /<Gate disabled=\{world !== null\}/); assert.match(s, /if \(disabled\) return false/); assert.equal((s.match(/<Btn disabled=\{disabled\} tone=/g) ?? []).length, 2)
})
