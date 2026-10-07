// A turn's trace for the flame chart and the timeline (`src/lib/spans.ts`, theseus-hnof): the spans in order with their
// depths, each kind's colour as a mark, and where the turn's time went.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { flatten, kindTone, spanColor, timeByKind } from '../src/lib/spans.ts'
import { OTHER, TONE_MARK } from '../src/lib/viz.ts'

const span = (name: string, kind: string, start_us: number, end_us: number | null, children: unknown[] = []) =>
  ({ name, kind, start_us, end_us, attrs: null, children }) as any

const trace = span('turn', 'turn', 0, 1000, [
  span('admission.wait', 'lock', 0, 100),
  span('loop 0', 'loop', 100, 1000, [
    span('compile', 'compile', 100, 150),
    span('provider.messages', 'provider', 150, 600, [span('first token', 'mark', 400, 400)]),
    span('tools', 'tools', 600, 900, [span('tool fs.read', 'tool', 600, 700), span('tool proc.run', 'tool', 600, 900)]),
    span('action.settle', 'store', 900, 950),
    span('open', 'store', 960, null),
  ]),
])

test('a trace flattens in order, each span with its depth; an open span ends where it starts', () => {
  const flat = flatten(trace)
  assert.deepEqual(flat.map((f) => [f.name, f.depth]), [
    ['turn', 0], ['admission.wait', 1], ['loop 0', 1], ['compile', 2], ['provider.messages', 2], ['first token', 3],
    ['tools', 2], ['tool fs.read', 3], ['tool proc.run', 3], ['action.settle', 2], ['open', 2],
  ])
  assert.equal(flat.at(-1)!.end, 960)
})

test('where the time went: the leaf-ish spans by kind, the most first; the turn, loops, tools spans, and marks hold the rest', () => {
  assert.deepEqual(timeByKind(flatten(trace)), [['provider', 450], ['tool', 400], ['lock', 100], ['compile', 50], ['store', 50]])
})

test('a span kind is drawn in its tone\'s step for marks, an unknown kind in the gray', () => {
  for (const [kind, tone] of Object.entries(kindTone)) assert.equal(spanColor(kind), TONE_MARK[tone])
  assert.equal(spanColor('advancer'), OTHER)
})
