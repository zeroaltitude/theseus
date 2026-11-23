import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
const src = (p: string) => readFileSync(new URL('../src/' + p, import.meta.url), 'utf8')
test('Ship rebuilds its bounded reader and clears freshness after a gap', () => {
  const s = src('ship/useShipData.ts'); assert.match(s, /read: new Set<string>\(\)/); assert.match(s, /\[open, store, epoch\]/); assert.match(s, /\[open, planKey, store, epoch\]/); assert.match(s, /case 'events.lost':[\s\S]*?setEpoch\(\(n\) => n \+ 1\)/)
})
