// The route footer and the correction layer in words (`src/lib/routefooter.ts`, theseus-q31l), run by `npm test`.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { RouteCorrectionsResult } from '@protocol'
import { correctionChoices, correctionLine, layerHead, personaChoices, personaWords, routeLine } from '../src/lib/routefooter.ts'

test("a turn's route line names its mode, or the correction that placed it", () => {
  assert.equal(routeLine(undefined, 'sonnet'), null)
  assert.equal(routeLine({ mode: 'quick', reason: 'detour', from: 'sonnet' }, 'haiku'), 'routed: quick · haiku (from sonnet)')
  assert.equal(routeLine({ mode: 'chat', reason: 'verdict', from: 'sonnet' }, 'sonnet'), 'routed: chat · sonnet')
  assert.equal(routeLine({ reason: 'no_verdict', from: 'sonnet' }, 'sonnet'), 'routed: no_verdict · sonnet')
  assert.equal(
    routeLine({ mode: 'chat', reason: 'correction', from: 'sonnet', source: 'correction' }, 'fable'),
    'routed: correction · fable (from sonnet)',
  )
  assert.equal(
    routeLine({ mode: 'routine_coding', reason: 'verdict', from: 'fable', source: 'task brief' }, 'haikuhi'),
    'routed: routine_coding (task brief) · haikuhi (from fable)',
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

test("a turn's line says who answered and, when Jev was less than sure, why (route.v5)", () => {
  const sure = { key: 'birch', name: 'Birch', how: 'jev', why: '' }
  const shown = { key: 'birch', name: 'Birch', how: 'jev_shown', why: '"CVE", "sandbox escape" · 0.94 · next alder 0.04' }
  assert.equal(routeLine({ mode: 'deep_coding', reason: 'verdict', from: 'opus', persona: sure }, 'opus'), 'routed: Birch · deep_coding · opus')
  assert.equal(
    routeLine({ mode: 'deep_coding', reason: 'verdict', from: 'opus', persona: shown }, 'opus'),
    'routed: Birch · "CVE", "sandbox escape" · 0.94 · next alder 0.04 · deep_coding · opus',
  )
  // A turn routing decided nothing for still says who answered.
  assert.equal(routeLine({ reason: '', from: '', persona: sure }, 'opus'), 'answered by: Birch')
  assert.equal(personaWords({ reason: 'verdict', from: 'opus' }), null)
})

test("the footer's persona menu is every persona but the one that answered", () => {
  const all = [{ key: 'birch', name: 'Birch' }, { key: 'alder', name: 'Alder' }, { name: 'Theseus' }]
  assert.deepEqual(personaChoices(all, 'birch'), [{ to: 'persona:alder', text: 'Alder' }])
})
