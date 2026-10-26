// The data behind the arm points (theseus-2nscq), on the real v1-high table and the fixgit set.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { frontier, interval, points, property, staircase, TASK_SETS, setsFrom, type Point } from '../src/lib/bench.ts'
import { cloudCaption, cloudTip, cloudUnit } from '../src/lib/benchcloud.ts'

const DIR = new URL('../../docs/benchmarks/', import.meta.url)
const read = (f: string) => readFileSync(new URL(f, DIR), 'utf8')
const REPORT = '2026-10-18-terminal-bench-v1-high'
const high = () => setsFrom([{ report: REPORT, ui: JSON.parse(read(`${REPORT}.json`)).ui }])[0]
const of = (by?: 'arm' | 'task' | 'trial') => points(high(), read(`${REPORT}.csv`), read(`${REPORT}.json`), read(`${REPORT}.md`), '2026-10-18', by)
const rowsIn = read(`${REPORT}.csv`).trim().split('\n').length - 1

test('by task: one point per arm and task, and the arm rate is the task points’ k over n', () => {
  const arms = of(), tasks = of('task')
  const tasksN = new Set(tasks.map((p) => p.task)).size
  assert.equal(tasksN, 89)
  assert.equal(tasks.length, arms.length * tasksN)
  for (const a of arms) {
    const mine = tasks.filter((p) => p.harness === a.harness)
    assert.ok(mine.every((p) => p.n <= 3))
    let k = 0, n = 0
    for (const p of mine) { const v = p.values.pass; k += Math.round(v.v * v.n); n += v.n }
    assert.equal(n, a.n)
    assert.equal(k / n, a.values.pass.v)
  }
  assert.equal(arms.reduce((x, a) => x + a.n, 0), rowsIn)
})

test('a task point’s interval is a range, never a bootstrap', () => {
  for (const p of of('task')) { const v = p.values.usd; if (v) assert.equal(interval(property('usd')!, v).kind, 'range') }
})

test('by trial on fixgit: one point per row', () => {
  const set = TASK_SETS.find((s) => s.id === 'fixgit')!
  const csv = read(`${set.report}.csv`)
  const kept = points(set, csv, read(`${set.report}.json`), read(`${set.report}.md`), set.report.slice(0, 10), 'trial')
  const arms = points(set, csv, read(`${set.report}.json`), read(`${set.report}.md`), set.report.slice(0, 10))
  assert.equal(kept.length, arms.reduce((a, p) => a + p.n, 0))
  assert.ok(kept.every((p) => p.n === 1))
  assert.equal(cloudUnit(arms), 'trial')
  assert.equal(cloudUnit(of()), 'task')
})

test('the frontier and its staircase do not depend on the cloud', () => {
  const [px, py] = [property('usd')!, property('pass')!]
  const run = () => { const pl = frontier(of(), px, py); return JSON.stringify([pl.map((p) => [p.point.id, p.frontier, p.dominators.map((d) => d.id)]), staircase(pl, px, py)]) }
  const before = run()
  of('task'); of('trial')
  assert.equal(run(), before)
  assert.deepEqual(of().map((p) => p.id), of('arm').map((p) => p.id))
})

test('the caption states the fold with counted numbers; the tip has no percent', () => {
  const arms: Point[] = of(), tasks = of('task')
  assert.equal(cloudCaption(arms, tasks, 'task'), `a point is one arm in one run: ${rowsIn.toLocaleString('en-US')} trials folded into ${arms.length} points; ${tasks.length} task points behind them`)
  const t = cloudTip(tasks[0], 'usd', 'pass')
  assert.ok(!t.lines.join(' ').includes('%'))
  assert.match(t.lines[0], /^\d of \d attempts? solved$/)
})
