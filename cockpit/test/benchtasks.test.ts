// The per-test pop-out's pure half (theseus-1de57): the ladder, its rules, and the real v1-high table.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { points, setsFrom } from '../src/lib/bench.ts'
import { assess, everyAssessment, taskRows, WORDS, type Side } from '../src/lib/benchtasks.ts'

const s = (k: number, n: number, name = 'Claude Code'): Side => ({ arm: 'claude-code', name, k, n })

test('every branch of the ladder, with counts and the arm compared against', () => {
  assert.deepEqual(assess(s(1, 1, 'Theseus'), s(1, 3)), { word: WORDS.few, counts: '1 of 1, against 1 of 3', against: 'Claude Code' })
  assert.equal(assess(s(2, 3), s(1, 1)).word, WORDS.few)
  assert.equal(assess(undefined, s(2, 3)).word, WORDS.few)
  assert.equal(assess(s(0, 3), s(0, 3)).word, WORDS.none)
  assert.equal(assess(s(2, 3), s(0, 3)).word, WORDS.only)
  assert.equal(assess(s(2, 3), s(2, 3)).word, WORDS.level)
  assert.equal(assess(s(3, 3), s(2, 3)).word, WORDS.ahead)
  assert.equal(assess(s(1, 3), s(2, 3)).word, WORDS.behind)
  assert.equal(assess(s(0, 3), s(2, 3)).word, WORDS.behind)
  const a = assess(s(2, 3, 'Theseus'), s(3, 3))
  assert.equal(a.counts, '2 of 3, against 3 of 3')
  assert.equal(a.against, 'Claude Code')
  assert.equal(WORDS.few, 'too few attempts to say')
  assert.equal(WORDS.only, 'only Theseus solved it')
})

test('nothing assess can say carries a percentage or a superlative', () => {
  for (const a of everyAssessment()) {
    const all = `${a.word} ${a.counts} ${a.against}`
    assert.ok(!/%/.test(all), all)
    assert.ok(!/\b(wins?|beats?|best-in-class|winner|better than all)\b/i.test(all), all)
  }
})

test('the real v1-high table: 89 rows, counts that sum to each arm’s trials', () => {
  const report = '2026-10-18-terminal-bench-v1-high'
  const read = (f: string) => readFileSync(new URL(`../../docs/benchmarks/${f}`, import.meta.url), 'utf8')
  const set = setsFrom([{ report, ui: JSON.parse(read(`${report}.json`)).ui }])[0]
  const args = [set, read(`${report}.csv`), read(`${report}.json`), read(`${report}.md`), '2026-10-18'] as const
  const rows = taskRows(points(...args, 'task'))
  assert.equal(rows.length, 89)
  const arms = points(...args)
  for (const a of arms) {
    const mine = rows.map((r) => r.sides.find((x) => x.arm === a.harness)!)
    assert.equal(mine.reduce((t, x) => t + x.n, 0), a.n)
    assert.equal(mine.reduce((t, x) => t + x.k, 0) / a.n, a.values.pass.v)
  }
  for (const r of rows) {
    assert.ok(r.theseus && r.best)
    assert.ok(Object.values(WORDS).includes(r.assessment.word as never))
  }
})

test('the catalogue is read like the manifest: an untrusted file gives no descriptions and a reason', async () => {
  const { readCatalogue } = await import('../src/lib/benchtasks.ts')
  for (const bad of [null, '', '{', '[]', '{"version":2,"tasks":{}}', '{"version":1}']) {
    const c = readCatalogue(bad)
    assert.equal(c.tasks.size, 0)
    assert.ok(c.reason)
  }
  const ok = readCatalogue(JSON.stringify({ version: 1, suite: 's', tasks: {
    a: { title: 'a task', difficulty: 'hard', tags: ['x'], expert_min: 5, summary: 'Do it.', source: 'javascript:alert(1)' },
    b: { difficulty: 7, tags: [1] }, c: 'nope' } }))
  assert.equal(ok.reason, null)
  assert.deepEqual([...ok.tasks.keys()], ['a', 'b'])
  assert.equal(ok.tasks.get('a')!.expertMin, 5)
  assert.equal(ok.tasks.get('a')!.source, undefined)
  assert.equal(ok.tasks.get('b')!.difficulty, undefined)
  assert.equal(ok.tasks.get('b')!.tags, undefined)
})

// theseus-yh48a: a listed ablation is a side of the per-task table, never a rival, and can stand for Theseus only when
// no plain Theseus ran. The slice mirrors 2026-10-03-harbor-worth-spike (theseus, theseus-batching, claude-code).
const thrice = (arm: string, reward: number) => [1, 2, 3].map(() => `t,${arm},${reward.toFixed(1)},0.1`)
const ablationSet = (rows: string[]) => {
  const csv = ['task,arm,reward,dollars', ...rows].join('\n')
  const set = { id: 'spike', report: 'x', keep: () => true, arm: (r: Record<string, string>) => r.arm, run: () => ({ id: 'r', label: 'r' }) } as never
  return taskRows(points(set, csv, undefined, undefined, '2026-10-03', 'task'))
}

test('a listed ablation is a side but not a rival: Theseus is judged against other harnesses', () => {
  const [r] = ablationSet([...thrice('theseus', 1), ...thrice('theseus-batching', 1), ...thrice('claude-code', 0)])
  assert.deepEqual(r.sides.map((x) => x.arm), ['theseus', 'claude-code', 'theseus-batching'])
  assert.equal(r.theseus?.arm, 'theseus')
  assert.equal(r.best?.arm, 'claude-code')
  assert.equal(r.assessment.word, WORDS.only)
})

test('with only an ablation and a rival, the ablation still stands for Theseus', () => {
  const [r] = ablationSet([...thrice('theseus-batching', 1), ...thrice('claude-code', 0)])
  assert.equal(r.theseus?.arm, 'theseus-batching')
  assert.equal(r.best?.arm, 'claude-code')
})
