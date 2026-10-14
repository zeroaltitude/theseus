import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { RouteCell } from '../src/protocol.gen/RouteCell.ts'
import type { RouteReportResult } from '../src/protocol.gen/RouteReportResult.ts'
import { effortWord, footer, rows, verdictWords } from '../src/lib/yourwork.ts'

const cell = (over: Partial<RouteCell> = {}): RouteCell => ({
  model: 'claude-sonnet-5-5', effort: 'high', turns: 40, spend_usd: 12.5, rung_down: 'haiku', replayed: 9, matched: 8, checked: 3,
  cost_ratio: 0.2, match_floor: 0.63, movable_usd: 6.3, replay_usd: 0.31, ...over,
})

const report = (cells: RouteCell[], over: Partial<RouteReportResult> = {}): RouteReportResult => ({
  window_days: 7, bar: 0.8, confidence: 0.9, cells, total_spend_usd: 16.5, total_movable_usd: 6.3, replay_today_usd: 0.31, replay_day_usd: 1, ...over,
})

test('a cell says what matched a rung down, or why it says nothing yet', () => {
  const r = report([])
  assert.equal(verdictWords(cell(), r), '8 of 9 replayed on haiku matched (same or better at 80%+); at 90% confidence at least 63% match')
  assert.equal(verdictWords(cell({ replayed: 0, matched: 0 }), r), 'replays on haiku not judged yet')
  assert.equal(verdictWords(cell({ rung_down: undefined, replayed: 0 }), r), 'not replayed yet')
  assert.equal(effortWord(''), 'default')
  assert.equal(effortWord('xhigh'), 'xhigh')
})

test('the rows share the dearest cell’s spend, and the footer names the cap', () => {
  const r = report([cell(), cell({ model: 'claude-fable-5-1', effort: '', spend_usd: 4, rung_down: undefined, replayed: 0, matched: 0 })])
  const got = rows(r)
  assert.deepEqual(got.map((g) => g.share), [1, 0.32])
  assert.equal(footer(r), 'movable at 90% confidence: $6.30 of $16.50; replays today $0.31 of $1.00')
  assert.equal(footer(report([], { sampler: '[route] replay_day_usd = 0: the sampler is off' })), 'movable at 90% confidence: $6.30 of $16.50; replays today $0.31 ([route] replay_day_usd = 0: the sampler is off)')
  assert.deepEqual(rows(report([])), [])
})
