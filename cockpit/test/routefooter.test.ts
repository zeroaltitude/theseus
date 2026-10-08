// The route footer and the correction layer in words (`src/lib/routefooter.ts`, theseus-q31l), run by `npm test`.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { RouteCorrectionsResult } from '@protocol'
import { correctionChoices, correctionLine, layerHead, routeLine } from '../src/lib/routefooter.ts'

test("a turn's route line names its mode, or the correction that placed it", () => {
  assert.equal(routeLine(undefined, 'sonnet'), null)
  assert.equal(routeLine({ mode: 'quick', reason: 'detour', from: 'sonnet' }, 'haiku'), 'routed: quick · haiku (from sonnet)')
  assert.equal(routeLine({ mode: 'chat', reason: 'verdict', from: 'sonnet' }, 'sonnet'), 'routed: chat · sonnet')
  assert.equal(routeLine({ reason: 'no_verdict', from: 'sonnet' }, 'sonnet'), 'routed: no_verdict · sonnet')
  assert.equal(
    routeLine({ mode: 'chat', reason: 'correction', from: 'sonnet', source: 'correction' }, 'fable'),
    'routed: correction · fable (from sonnet)',
  )
})

test("the footer's controls are a direction each way, then every other profile", () => {
  const c = correctionChoices(['sonnet', 'opus', 'fable'], 'sonnet')
  assert.deepEqual(c.map((x) => x.to), ['stronger', 'cheaper', 'opus', 'fable'])
  assert.equal(c[0].text, '⬆ stronger')
})

test('the layer reads a head and a line an entry', () => {
  const r: RouteCorrectionsResult = {
    pack: 'route.v2',
    entries: [{
      id: 'rcx_1', label: 'lbl_1', pack: 'route.v2', session_id: 'ses_a', turn_id: 'turn_1', to: 'fable',
      words: ['format', 'lighthouse', 'log', 'parser', 'rust', 'write', 'x', 'y', 'z'], at_ms: 1,
    }],
    max_entries: 64, similarity: 0.5, retired: 2, enabled: true,
  }
  assert.equal(layerHead(r), 'on · 1 of 64 under route.v2 · close at 50% of words in common · 2 retired')
  assert.equal(correctionLine(r.entries[0]), '→ fable · format lighthouse log parser rust write x y +1')
})
