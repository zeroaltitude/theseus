import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
const src = (p: string) => readFileSync(new URL('../src/' + p, import.meta.url), 'utf8')
test('label tooltips and settings no longer promise origin filtering or threads', () => {
  assert.match(src('simple/config/settings.ts'), /tables: \[[^\]]*'threads'/)
  for (const p of ['ops/ModelMenu.tsx', 'ops/Inspector.tsx', 'ops/GraphNode.tsx']) assert.doesNotMatch(src(p), /US.origin models|models of US origin/); assert.doesNotMatch(src('simple/config/settings.ts'), /sessions and threads/)
})
