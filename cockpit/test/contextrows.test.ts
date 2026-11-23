// The context compiles the instruments count (`src/lib/contextRows.ts`), run by `npm test`: the per-call
// `context.compiled` row is the one they read, and an old store's `context.recompiled` rows (no longer written,
// issue 46) change no series and are still said in words. Invented ids.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { contextSeries, isContextCompile } from '../src/lib/contextRows.ts'
import { summarize } from '../src/lib/summary.ts'

const row = (kind: string, at: number, data: Record<string, unknown>) =>
  ({ position: at, at_unix_ms: at, kind, session_id: 'ses_a', turn_id: 'turn_a', data }) as any

const compiled = (at: number, est: number) =>
  row('context.compiled', at, { decision: 'new', trigger: 'view_turn', est_tokens: est, messages: 3, strategy: 'view', compilation_id: `cmp_${at}` })
const recompiled = (at: number) =>
  row('context.recompiled', at, { compilation_id: `cmp_${at}`, trigger: 'view_turn', strategy: 'view', as_of: 9, includes: 4 })

test("an old store's recompiled rows change no series: the per-call row is the one counted", () => {
  const now = [compiled(1, 1200), compiled(2, 1350)]
  const old = [recompiled(1), compiled(1, 1200), recompiled(2), compiled(2, 1350)]
  assert.deepEqual([...contextSeries(old)], [...contextSeries(now)])
  assert.deepEqual(contextSeries(now).get('ses_a'), [[1, 1200], [2, 1350]])
  assert.equal(isContextCompile('context.compiled'), true)
  assert.equal(isContextCompile('context.recompiled'), false)
})

test("an old store's recompiled row is still said in words, and the per-call row says the same compilation", () => {
  assert.equal(summarize(recompiled(1)), 'view_turn · view · 4 node(s)')
  assert.match(summarize(compiled(1, 1200)), /^context new \(view_turn\): .* tokens, 3 messages \(view\)/)
})
