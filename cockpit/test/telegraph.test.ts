// The Ship's engine telegraph (`src/ship/telegraph.ts`), run by `npm test` (theseus-h70f): the engine says what the
// kernel runs, as the header's "running" and the kernel lamp do, and its ceiling is the one health reports from the
// config. Invented health; no ceiling is pinned beyond the ones a case sets.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { engineOrder, engineReading, engineSub, engineTitle } from '../src/ship/telegraph.ts'
import { kernelLamp } from '../src/lib/healthwords.ts'

type D = Record<string, any>
const health = (kernel: D = {}): any => ({
  kernel: { accepting: true, admission_ceiling: 32, turns_held: 0, executions_by_state: { complete: 1, waiting: 11 }, ...kernel },
})

test('with nothing running the engine says 0, though two ships are drawn working in the background', () => {
  // The daemon lists only the states that have executions, so `running` is absent here (the reviewer's scratch daemon).
  const h = health()
  assert.equal(h.kernel.executions_by_state.running, undefined)
  const r = engineReading(h, undefined)
  assert.deepEqual(r, { accepting: true, running: 0, held: 0, ceiling: 32 })
  assert.equal(engineSub(r), 'accepting · 0 of 32')
  assert.equal(engineTitle(r), 'The kernel: accepting; 0 of 32 executions running. SLOW is idle, HALF up to half the ceiling, FULL past it.')
  assert.equal(engineOrder(r), 1)
})

test('the engine reads the kernel: the running executions, the turns held, and the config\'s ceiling', () => {
  const h = health({ admission_ceiling: 5, turns_held: 3, executions_by_state: { running: 3, waiting: 4 } })
  const r = engineReading(h, undefined)
  assert.deepEqual(r, { accepting: true, running: 3, held: 3, ceiling: 5 })
  assert.equal(engineSub(r), 'accepting · 3 of 5 · 3 held')
  assert.equal(engineOrder(r), 3)
  // The ceiling follows the config: the same load against a bigger one is half ahead, not full.
  const big = engineReading(health({ admission_ceiling: 6, turns_held: 3, executions_by_state: { running: 3 } }), undefined)
  assert.equal(big.ceiling, 6)
  assert.equal(engineOrder(big), 2)
})

test('the engine, the kernel lamp and the header agree on the count', () => {
  for (const [by, want] of [[{ waiting: 11 }, 0], [{ running: 4 }, 4]] as const) {
    const h = health({ executions_by_state: by })
    const header = h.kernel.executions_by_state.running ?? 0 // Heartbeat's "running" readout
    assert.equal(engineReading(h, undefined).running, want)
    assert.equal(engineReading(h, undefined).running, header)
    assert.match(kernelLamp(h).detail, new RegExp(`· ${want} running of 32 at once`))
  }
})

test('a stopped kernel says holding, whatever runs', () => {
  const r = engineReading(health({ accepting: false, executions_by_state: {} }), undefined)
  assert.deepEqual(r, { accepting: false, running: 0, held: 0, ceiling: 32 })
  assert.equal(engineSub(r), 'holding · 0 of 32')
  assert.equal(engineOrder(r), 0)
})

test('before health is read the engine claims no count and no ceiling', () => {
  const r = engineReading(undefined, undefined)
  assert.deepEqual(r, { accepting: undefined, running: null, held: 0, ceiling: null })
  assert.equal(engineSub(r), 'not read yet')
  assert.equal(engineOrder(r), 1)
})

test('the time machine reads the fold\'s running count, and today\'s ceiling', () => {
  const r = engineReading(health({ admission_ceiling: 32, turns_held: 7 }), { accepting: true, running: 2 })
  assert.deepEqual(r, { accepting: true, running: 2, held: 0, ceiling: 32 })
  assert.equal(engineSub(r), 'accepting · 2 of 32')
  const down = engineReading(health(), { accepting: false, running: 0 })
  assert.equal(engineSub(down), 'holding · 0 of 32')
})
