import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { ago, ms, span, uptime, until, when } from '../src/lib/figures.ts'
import { comingTimes } from '../src/ops/facts.ts'

const S = 1000
const M = 60 * S
const H = 60 * M
const D = 24 * H
const NOW = new Date(2026, 9, 24, 14, 30, 0).getTime() // local time, so the zone does not matter

test('a span follows the rule: floor each unit, drop a zero, days only from a week', () => {
  const rows: [number, string][] = [
    [0, '0s'], [4 * S, '4s'], [40 * S, '40s'], [59 * S, '59s'], [60 * S, '1m'], [12 * M, '12m'], [59 * M + 59 * S, '59m'],
    [H, '1h'], [166 * M, '2h 46m'], [2 * H + 46 * M + 59 * S, '2h 46m'], [23 * H + 59 * M, '23h 59m'], [24 * H, '1d'],
    [45 * H, '1d 21h'], [6 * D + 23 * H, '6d 23h'], [7 * D, '7d'], [9 * D + 5 * H, '9d'], [-5, '0s'],
  ]
  for (const [n, want] of rows) assert.equal(span(n), want, `span(${n})`)
})

test('ago reads now under five seconds, then the span', () => {
  const rows: [number, string][] = [
    [0, 'now'], [4 * S, 'now'], [5 * S, '5s ago'], [40 * S, '40s ago'], [59 * S, '59s ago'], [60 * S, '1m ago'], [12 * M, '12m ago'],
    [H, '1h ago'], [166 * M, '2h 46m ago'], [23 * H + 59 * M, '23h 59m ago'], [24 * H, '1d ago'], [45 * H, '1d 21h ago'],
    [6 * D + 23 * H, '6d 23h ago'], [7 * D, '7d ago'], [9 * D + 5 * H, '9d ago'],
  ]
  for (const [d, want] of rows) assert.equal(ago(NOW - d, NOW), want, `ago ${d}`)
  assert.equal(ago(null, NOW), '—')
})

test('until reads in <span> for the future and keeps the overdue words', () => {
  const rows: [number, string][] = [
    [40 * S, 'in 40s'], [59 * S, 'in 59s'], [60 * S, 'in 1m'], [12 * M, 'in 12m'], [59 * M + 59 * S, 'in 59m'], [H, 'in 1h'],
    [166 * M, 'in 2h 46m'], [23 * H + 59 * M, 'in 23h 59m'], [24 * H, 'in 1d'], [45 * H, 'in 1d 21h'], [6 * D + 23 * H, 'in 6d 23h'],
    [7 * D, 'in 7d'], [9 * D + 5 * H, 'in 9d'],
  ]
  for (const [d, want] of rows) assert.equal(until(NOW + d, NOW), want, `until ${d}`)
  assert.equal(until(NOW - 3 * M, NOW), 'due 3m ago')
  assert.equal(until(NOW - 2 * S, NOW), 'due now')
})

test('ms keeps its latency style under an hour and follows the rule from one', () => {
  assert.equal(ms(340), '340 ms')
  assert.equal(ms(4400), '4.40 s')
  assert.equal(ms(192_000), '3m 12s')
  assert.equal(ms(H), '1h')
  assert.equal(ms(166 * M), '2h 46m')
  assert.equal(ms(45 * H), '1d 21h')
  assert.equal(ms(9 * D), '9d')
})

test('uptime follows the rule from a day', () => {
  assert.equal(uptime(59), '59s')
  assert.equal(uptime(3725), '1h 2m 5s')
  assert.equal(uptime(26 * 3600 + 5 * 60), '1d 2h')
  assert.equal(uptime(10 * 86400), '10d')
})

test('when says a clock time without seconds, with a day word when it is not today', () => {
  const at = (day: number, h: number, m: number) => new Date(2026, 9, day, h, m, 59).getTime()
  assert.equal(when(at(24, 20, 0), NOW), '20:00')
  assert.equal(when(at(24, 0, 5), NOW), '00:05')
  assert.equal(when(at(25, 8, 0), NOW), 'tomorrow 08:00')
  assert.equal(when(at(23, 17, 10), NOW), 'yesterday 17:10')
  assert.equal(when(at(26, 9, 0), NOW), 'Mon 09:00')
  assert.equal(when(at(30, 9, 0), NOW), 'Fri 09:00')
  assert.equal(when(at(18, 9, 0), NOW), 'Sun 09:00')
  assert.equal(when(at(31, 9, 0), NOW), 'Oct 31 09:00')
  assert.equal(when(at(17, 9, 0), NOW), 'Oct 17 09:00')
  assert.equal(when(null, NOW), '—')
})

test('Coming up words a 166-minute wait as in 2h 46m, and its row uses the shared words', () => {
  const t = comingTimes(NOW + 166 * M, NOW)
  assert.equal(t.rel, 'in 2h 46m')
  assert.equal(t.at, '17:16')
  assert.equal(comingTimes(NOW + 40 * S, NOW).rel, 'in 40s')
  const src = readFileSync(new URL('../src/ops/Queue.tsx', import.meta.url), 'utf8')
  assert.ok(src.includes('comingTimes('), 'Coming up reads comingTimes')
  assert.ok(!/in \{Math/.test(src) && !src.includes('60_000'), 'no inline minutes in Queue.tsx')
})
