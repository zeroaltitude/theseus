import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
const src = (p: string) => readFileSync(new URL('../src/' + p, import.meta.url), 'utf8')
test('the compact composer is owned by its session, including queued text and files', () => {
  assert.match(src('ops/Inspector.tsx'), /<JoinPane key=\{r.sessionId\} sessionId=\{r.sessionId\}/)
})
