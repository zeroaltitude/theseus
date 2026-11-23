import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'
const src = (p: string) => readFileSync(new URL('../src/' + p, import.meta.url), 'utf8')
test("tightening feedback comes from current policy, not a previous tool's success", () => {
  const s = src('components/ShouldHaveAsked.tsx'); assert.doesNotMatch(s, /if \(done\)/); assert.match(s, /if \(tightened\)/)
})

test('every transcript receives current tightening feedback', () => {
  const root = new URL('../src/', import.meta.url)
  let mounts = 0
  for (const f of readdirSync(root, { recursive: true }).map(String).filter((f) => f.endsWith('.tsx'))) {
    for (const m of readFileSync(new URL(f, root), 'utf8').matchAll(/<Transcript\b[^>]*\/>/g)) {
      mounts++
      assert.match(m[0], /tightened=\{tightened\}/, f)
    }
  }
  assert.ok(mounts >= 2)
  assert.match(src('ops/JoinPane.tsx'), /useRpc<Health>\('health', undefined, 5000\)/)
  assert.match(src('ops/JoinPane.tsx'), /new Map\(\(health\?\.tightenings \?\? \[\]\)\.map/)
})
