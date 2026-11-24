import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
const src = (p: string) => readFileSync(new URL('../src/' + p, import.meta.url), 'utf8')
test('People and Topics keep a stable controlled filter input', () => {
  const s = src('simple/memories/Kinds.tsx'); assert.match(s, /<input value=\{f\} onChange=/); assert.doesNotMatch(s, /key=\{f\}/)
})
