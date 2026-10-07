// The chart method's pure parts (`src/lib/viz.ts`, theseus-hnof), run by `npm test`: the palettes' shape, the slots that
// follow an entity, the axis figures that never print a tick twice, and the derivations the Economics charts draw.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  CATEGORICAL, OTHER, TOKEN_KINDS, TONE_SERIES, barRadius, bucketEnd, bucketFor, bucketStart, jitter, kindTokens, latencyByModel,
  msLogTick, msTick, niceScale, niceStep, numTick, quantile, shares, slotColor, slots, spendByBucket, spendBySession, spendTree,
  stackTop, stepDecimals, tokenTick, usTick, usdTick,
} from '../src/lib/viz.ts'

const HEX = /^#[0-9a-f]{6}$/

test('the palettes are the validated hexes: eight categorical slots a mode, five token kinds, eight tone steps', () => {
  for (const mode of ['dark', 'light'] as const) {
    assert.equal(CATEGORICAL[mode].length, 8)
    assert.equal(new Set(CATEGORICAL[mode]).size, 8)
    for (const c of CATEGORICAL[mode]) assert.match(c, HEX)
    assert.deepEqual(TONE_SERIES[mode].map(([t]) => t).sort(), ['fault', 'live', 'model', 'money', 'ok', 'think', 'tool', 'wait'])
    for (const [, c] of TONE_SERIES[mode]) assert.match(c, HEX)
  }
  assert.deepEqual(TOKEN_KINDS.map((k) => k.key), ['input', 'cacheRead', 'cacheWrite', 'cacheWrite1h', 'output'])
  assert.match(OTHER, HEX)
})

test('a series keeps its slot as the record grows, and past eight the tail folds into other', () => {
  const a = slots(['sonnet', 'glm', 'sonnet', 'haiku'])
  assert.deepEqual([...a.slot], [['sonnet', 0], ['glm', 1], ['haiku', 2]])
  assert.deepEqual(a.folded, [])
  // A new model appends: the others keep their colours.
  const b = slots(['sonnet', 'glm', 'haiku', 'opus'])
  for (const k of ['sonnet', 'glm', 'haiku']) assert.equal(slotColor(a.slot, k), slotColor(b.slot, k))
  // Eight fit; nine keep seven slots and fold two.
  assert.equal(slots('abcdefgh'.split('')).folded.length, 0)
  const nine = slots('abcdefghi'.split(''))
  assert.equal(nine.slot.size, 7)
  assert.deepEqual(nine.folded, ['h', 'i'])
  assert.equal(slotColor(nine.slot, 'i'), OTHER)
  assert.equal(slotColor(nine.slot, 'a', 'light'), CATEGORICAL.light[0])
})

test('an axis never prints the same tick twice, from a hundredth of a cent to millions', () => {
  for (let e = -6; e <= 6; e++) {
    for (const m of [1, 1.3, 2.2, 3.7, 4.99, 7.9]) {
      const max = m * 10 ** e
      const s = niceScale(max)
      assert.ok(s.max >= max, `top ${s.max} under ${max}`)
      const n = Math.round(s.max / s.interval)
      assert.ok(n >= 1 && n <= 5, `${n} steps for ${max}`)
      const ticks = Array.from({ length: n + 1 }, (_, k) => k * s.interval)
      for (const fmt of [usdTick(s.interval), msTick(s.interval)]) {
        const labels = ticks.map(fmt)
        assert.equal(new Set(labels).size, labels.length, `${max}: ${labels.join(', ')}`)
      }
    }
  }
  // The economics page's own case: a cent and a half over a day read in tenths of a cent, not "$0.01, $0.01".
  assert.deepEqual([0, 0.002, 0.004, 0.006, 0.008].map(usdTick(niceScale(0.0079).interval)), ['$0.000', '$0.002', '$0.004', '$0.006', '$0.008'])
  assert.deepEqual([0, 1000, 2000].map(usdTick(1000)), ['$0', '$1,000', '$2,000'])
  assert.equal(stepDecimals(0.0025), 4)
  // A count's step is at least 1; a plain number keeps its step's decimals.
  assert.deepEqual(niceScale(2, 4, 1), { max: 2, interval: 1 })
  assert.deepEqual([0, 2.5, 5].map(numTick(2.5)), ['0.0', '2.5', '5.0'])
  assert.equal(niceStep(0), 1)
  assert.deepEqual(niceScale(0), { max: 1, interval: 1 })
})

test('tokens and a turn\'s microseconds on an axis: each tick in its unit, never two alike', () => {
  for (let e = 0; e <= 8; e++) {
    for (const m of [1, 1.3, 2.2, 3.7, 7.9]) {
      const s = niceScale(m * 10 ** e)
      const ticks = Array.from({ length: Math.round(s.max / s.interval) + 1 }, (_, k) => k * s.interval)
      for (const fmt of [tokenTick(s.interval), usTick(s.interval)]) {
        const labels = ticks.map(fmt)
        assert.equal(new Set(labels).size, labels.length, `${m * 10 ** e}: ${labels.join(', ')}`)
      }
    }
  }
  assert.deepEqual([0, 2500, 5000, 7500].map(tokenTick(2500)), ['0', '2.5k', '5.0k', '7.5k'])
  assert.deepEqual([0, 20, 40].map(tokenTick(20)), ['0', '20', '40'])
  assert.deepEqual([0, 500_000, 1_000_000].map(tokenTick(500_000)), ['0', '500k', '1,000k'])
  assert.deepEqual([0, 2_000_000, 4_000_000].map(tokenTick(2_000_000)), ['0', '2M', '4M'])
  assert.deepEqual([0, 20_000, 40_000].map(usTick(20_000)), ['0 ms', '20 ms', '40 ms'])
  assert.deepEqual([0, 250, 500].map(usTick(250)), ['0 µs', '250 µs', '500 µs'])
  assert.deepEqual([0, 1_500_000, 3_000_000].map(usTick(1_500_000)), ['0.0 s', '1.5 s', '3.0 s'])
})

test('a log axis of milliseconds reads each power of ten in its own unit', () => {
  const labels = [0.01, 0.1, 1, 10, 100, 1000, 10_000, 100_000].map(msLogTick)
  assert.deepEqual(labels, ['10 µs', '100 µs', '1 ms', '10 ms', '100 ms', '1 s', '10 s', '100 s'])
  assert.equal(new Set([2, 3, 5, 20, 30, 50].map(msLogTick)).size, 6)
  assert.deepEqual([0, 500, 1000, 1500].map(msTick(500)), ['0 ms', '500 ms', '1,000 ms', '1,500 ms'])
  assert.deepEqual([0, 2000, 4000].map(msTick(2000)), ['0 s', '2 s', '4 s'])
})

test('marks: the data end is rounded and the baseline square; a stack rounds only its last segment with a value', () => {
  assert.deepEqual(barRadius(false), [4, 4, 0, 0])
  assert.deepEqual(barRadius(true), [0, 4, 4, 0])
  assert.deepEqual(stackTop([[1, 1, 0], [2, 0, 0], [0, 0, 0]]), [1, 0, -1])
  const js = Array.from({ length: 200 }, (_, i) => jitter(i))
  assert.ok(js.every((j) => j >= -0.5 && j < 0.5))
  assert.equal(jitter(17), jitter(17))
  assert.deepEqual(shares([1, 3, 0]), [0.25, 0.75, 0])
  assert.deepEqual(shares([0, 0]), [0, 0])
  assert.equal(quantile([5, 1, 3], 0.5), 3)
})

test('spend by bucket: local hours, each key in the order first named, and the running total', () => {
  const h = 3600_000
  const t0 = bucketStart(Date.UTC(2026, 9, 6, 12, 0), 'hour')
  const calls = [
    { at: t0 + 60_000, model: 'sonnet', cost: 0.01 },
    { at: t0 + 120_000, model: 'glm', cost: 0.002 },
    { at: t0 + h + 5, model: 'sonnet', cost: 0.004 },
    { at: t0 + 3 * h, model: 'glm', cost: 0.001 },
  ]
  const s = spendByBucket(calls, 'hour')
  assert.deepEqual(s.starts, [t0, t0 + h, t0 + 3 * h])
  assert.deepEqual(s.ends, [t0 + h, t0 + 2 * h, t0 + 4 * h])
  assert.deepEqual(s.series.map((x) => x.key), ['sonnet', 'glm'])
  assert.deepEqual(s.series[0].values, [0.01, 0.004, 0])
  assert.ok(Math.abs(s.cumulative[2] - 0.017) < 1e-12)
  // Folding: every model past the slots counts under one key.
  const f = spendByBucket(calls, 'hour', (m) => (m === 'glm' ? 'other' : m))
  assert.deepEqual(f.series.map((x) => x.key), ['sonnet', 'other'])
  // A day bucket is the local calendar day; five minutes start on a multiple of five.
  const d = bucketStart(t0, 'day')
  assert.equal(new Date(d).getHours(), 0)
  assert.equal(bucketStart(bucketEnd(d, 'day') - 1, 'day'), d)
  const m = bucketStart(t0 + 7 * 60_000 + 30_000, '5 min')
  assert.equal(m, t0 + 5 * 60_000)
  assert.equal(bucketEnd(m, '5 min'), t0 + 10 * 60_000)
  // The bucket follows the record's span: never one column for an hour of record.
  assert.deepEqual([40 * 60_000, 20 * h, 9 * 24 * h].map(bucketFor), ['5 min', 'hour', 'day'])
})

test('the spend tree is provider, model, session, most first; sessions stay apart by id', () => {
  const tree = spendTree([
    { provider: 'anthropic', model: 'sonnet', session_id: 's1', cost: 0.01 },
    { provider: 'anthropic', model: 'sonnet', session_id: 's2', cost: 0.03 },
    { provider: 'anthropic', model: 'haiku', session_id: 's1', cost: 0.001 },
    { provider: 'zai', model: 'glm', session_id: null, cost: 0.05 },
  ])
  assert.deepEqual(tree.map((p) => [p.key, +p.cost.toFixed(6), p.calls]), [['zai', 0.05, 1], ['anthropic', 0.041, 3]])
  assert.deepEqual(tree[1].children.map((m) => m.key), ['sonnet', 'haiku'])
  assert.deepEqual(tree[1].children[0].children.map((s) => [s.key, s.cost]), [['s2', 0.03], ['s1', 0.01]])
  assert.equal(tree[0].children[0].children[0].key, '—')
})

test('spend by session splits each by model, in the models\' own order', () => {
  const rows = spendBySession([
    { session_id: 'a', model: 'glm', cost: 0.002 },
    { session_id: 'a', model: 'sonnet', cost: 0.001 },
    { session_id: 'b', model: 'sonnet', cost: 0.01 },
    { session_id: null, model: 'sonnet', cost: 1 },
  ], ['sonnet', 'glm'])
  assert.deepEqual(rows.map((r) => r.session), ['b', 'a'])
  assert.deepEqual(rows[1].byModel.map((x) => x.model), ['sonnet', 'glm'])
  assert.equal(rows[1].calls, 2)
})

test('latency by model: each call\'s first token and total, with p50 and p95, and models with none left out', () => {
  const calls = [10, 20, 30, 40].map((v, i) => ({ at: i, model: 'm', first_token_ms: v, total_ms: v * 10 }))
  const [l, ...rest] = latencyByModel([...calls, { at: 9, model: 'n' }], ['m', 'n'])
  assert.equal(rest.length, 0)
  assert.equal(l.first.length, 4)
  assert.equal(l.firstP50, 30)
  assert.equal(l.totalP95, 400)
})

test('tokens by kind keep the 1-hour cache writes apart', () => {
  const t = kindTokens([
    { usage: { input_tokens: 10, output_tokens: 5, cache_read_input_tokens: 100, cache_creation_input_tokens: 40, cache_creation_1h_input_tokens: 15 } },
    { usage: { input_tokens: 1, output_tokens: 1, cache_read_input_tokens: 0, cache_creation_input_tokens: 2 } },
  ] as any)
  assert.deepEqual(t, { input: 11, cacheRead: 100, cacheWrite: 27, cacheWrite1h: 15, output: 6 })
})
