// The last start and the store in words (`src/lib/startupwords.ts`), run by `npm test` (theseus-vm3n.6).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { phaseOutcome, startFigures, storeLines } from '../src/lib/startupwords.ts'

const phase = (name: string, start_us: number, end_us: number | null, detail: Record<string, unknown> | null = null, background = false) =>
  ({ name, start_us, end_us, detail, background }) as any

test('a phase says what it found', () => {
  assert.equal(phaseOutcome(phase('store', 0, 10, { outcome: 'ok', last_position: 12345, replayed_into_index: 3 })), 'ok · 12,345 positions · 3 replayed')
  assert.equal(phaseOutcome(phase('secrets', 0, 10, { state: 'failed', failed: ['aws_key', 'aws_secret'] })), 'failed · failed: aws_key, aws_secret')
  assert.equal(phaseOutcome(phase('kernel', 0, 10, { steps: [{ name: 'load', us: 1500 }] })), 'load 1.5 ms')
  assert.equal(phaseOutcome(phase('x', 0, 10)), '')
})

test('the start path names its phases, and the time between them is the unnamed part', () => {
  const f = startFigures([phase('config', 0, 2000), phase('store', 3000, 5000), phase('after', 0, 90000, null, true)])
  assert.deepEqual(f, { serving: 5000, between: 1000, slow: false })
  assert.equal(startFigures([phase('a', 0, 1000), phase('b', 4000, 5000)]).slow, true)
  assert.deepEqual(startFigures([]), { serving: 0, between: 0, slow: false })
})

test('a corrupt history, skipped records, and a crash that ended the last run are faults', () => {
  const lines = storeLines(
    [phase('store.verify', 0, 5, { outcome: 'corrupt', error: 'frame 7 checksum' })],
    { refused_records: 2, refused_positions: [7], repair: 'theseusd restore --repair with a copy' } as any,
    { at_unix_ms: 0, pid: 9, version: '0.1.0', thread: 'main', location: 'a.rs:1:2', file: 'crashes/x', this_start: true } as any,
  )
  assert.deepEqual(lines.map((l) => l.tone), ['fault', 'fault', 'fault'])
  assert.match(lines[0].text, /CORRUPT history, frame 7 checksum/)
  assert.match(lines[1].text, /2 records skipped .*positions 7, …\) · theseusd restore --repair/)
  assert.match(lines[2].text, /which ended the last run/)
  assert.deepEqual(storeLines([], { refused_records: 0, refused_positions: [] } as any, undefined), [])
})
