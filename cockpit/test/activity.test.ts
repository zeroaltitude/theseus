// The activity strip's folding (`src/lib/activity.ts`), run by `npm test` (theseus-hnof.5): lines that say the same
// thing fold into one with a count, at the place of the newest, and every line stays in its fold. Invented lines.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { foldKey, foldRepeats, stripOpen, type StripLine } from '../src/lib/activity.ts'

let n = 0
const line = (at: number, part: string, text: string, session: string | null = 'ses_a'): StripLine =>
  ({ key: `k${n++}`, at, part, tone: 'live', session, text })

const JEV = 'Jev, in shadow, judged the probe: yes (0.50, escalate band). The baseline decided; recorded, not acted on.'

test('five hundred copies of one line fold into one, with the count and the first time', () => {
  const lines = Array.from({ length: 500 }, (_, i) => line(10_000 - i, 'turn', JEV, i % 2 ? 'ses_a' : 'ses_b'))
  const folds = foldRepeats(lines)
  assert.equal(folds.length, 1)
  assert.equal(folds[0].count, 500)
  assert.equal(folds[0].line, lines[0], 'the fold shows its newest line')
  assert.equal(folds[0].last, 10_000)
  assert.equal(folds[0].first, 10_000 - 499)
  assert.deepEqual(folds[0].sessions, ['ses_b', 'ses_a'])
  assert.equal(folds[0].members.length, 500)
})

test('lines that differ only in their numbers are one line; another kind or other words are not', () => {
  assert.equal(foldKey({ part: 'web.dev_origin', text: 'web UI served the dev page 6×' }), foldKey({ part: 'web.dev_origin', text: 'web UI served the dev page 8×' }))
  assert.equal(foldKey({ part: 'judge.call', text: 'answered_by=jev-1.13.0 · cost_micros=13' }), foldKey({ part: 'judge.call', text: 'answered_by=jev-1.13.0 · cost_micros=139' }))
  assert.notEqual(foldKey({ part: 'turn', text: JEV }), foldKey({ part: 'narrative', text: JEV }))
  assert.notEqual(foldKey({ part: 'turn', text: 'judged the probe' }), foldKey({ part: 'turn', text: 'judged the stop' }))
})

test('folds keep the order of their newest lines, and every line is in exactly one fold', () => {
  const lines = [
    line(900, 'web.dev_origin', 'web UI served the dev page 1×', null),
    line(800, 'turn', JEV),
    line(700, 'judge.call', 'cost_micros=13'),
    line(600, 'turn', JEV),
    line(500, 'judge.call', 'cost_micros=14'),
    line(400, 'web.dev_origin', 'web UI served the dev page 8×', null),
    line(300, 'turn.ended', 'Turn 3 ended in 2.1 s'),
  ]
  const folds = foldRepeats(lines)
  assert.deepEqual(folds.map((f) => [f.line.part, f.count]), [['web.dev_origin', 2], ['turn', 2], ['judge.call', 2], ['turn.ended', 1]])
  assert.equal(folds.reduce((a, f) => a + f.count, 0), lines.length)
  const keys = folds.flatMap((f) => f.members.map((m) => m.key)).sort()
  assert.deepEqual(keys, lines.map((l) => l.key).sort())
  assert.deepEqual(folds[0].sessions, [], 'a line with no session adds none')
  assert.deepEqual(folds[0].members.map((m) => m.at), [900, 400], 'members stay newest first')
})

test('nothing to fold is nothing', () => {
  assert.deepEqual(foldRepeats([]), [])
})

test('the strip starts folded, and opens only where this browser left it open', () => {
  assert.equal(stripOpen(null), false)
  assert.equal(stripOpen(undefined), false)
  assert.equal(stripOpen('closed'), false)
  assert.equal(stripOpen('open'), true)
})
