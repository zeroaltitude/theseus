import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
const src = (p: string) => readFileSync(new URL('../src/' + p, import.meta.url), 'utf8')
test("tightening feedback comes from current policy, not a previous tool's success", () => {
  const s = src('components/ShouldHaveAsked.tsx'); assert.doesNotMatch(s, /if \(done\)/); assert.match(s, /if \(tightened\)/)
})
