import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
const src = (p: string) => readFileSync(new URL('../src/' + p, import.meta.url), 'utf8')
test('mode notifications refresh mode reads and reconnect refreshes them too', () => {
  const s = src('lib/rpc.ts'); assert.match(s, /PUSHED = new Set\(\[[^\]]*'session.mode'/); assert.match(s, /method === 'session.mode'/)
})
