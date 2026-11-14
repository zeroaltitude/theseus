// Benchmarks at a glance: one line a benchmark, saying where Theseus stands against the best harness measured beside
// it, and under it what every other run found in a sentence. It is the Ship's benchmark cards read plainly
// (`lib/benchcards.ts`'s `cardsOf`, nothing of its own), so a number here is the number on the card, with the card's
// own rules: a pass rate is printed only where the run supports one, and a run it does not speak for says what it
// found instead. The reports are the build's own files; nothing here is live.
import { useMemo } from 'react'
import { Link } from 'react-router'
import { FileText, Trophy } from 'lucide-react'
import { readIndex, runs } from '@/lib/bench'
import { cardsOf, type ArmLine, type Card } from '@/lib/benchcards'
import { LEAD } from '@/lib/benchindex'
import { REPORTS, useBenchFile, useBenchManifest, useBenchSets } from '@/lib/benchfiles'
import { cn } from '@/lib/format'
import { Empty, Panel, Pill } from '@/components/ui'

const pc = (v: number) => `${(v * 100).toFixed(1)}%`

/** How Theseus compares with another arm on one card. It is said outright only when their intervals do not overlap;
 *  when they do, the lean of the two rates is said with the fact that the run does not settle it. */
function standing(mine: ArmLine, other: ArmLine): { words: string; tone: 'ok' | 'fault' | 'wait' | 'idle' } {
  const a = mine.row.pass, b = other.row.pass
  const gap = Math.abs(a.value - b.value) * 100
  const points = `${gap.toFixed(1)} ${gap.toFixed(1) === '1.0' ? 'point' : 'points'}`
  if (a.lo > b.hi) return { words: `ahead by ${points}`, tone: 'ok' }
  if (a.hi < b.lo) return { words: `behind by ${points}`, tone: 'fault' }
  if (gap < 0.05) return { words: 'level', tone: 'idle' }
  return { words: `${points} ${a.value > b.value ? 'ahead' : 'behind'}, not settled: the ranges overlap`, tone: a.value > b.value ? 'idle' : 'wait' }
}

export default function BenchBoard() {
  const readme = useBenchFile('README', 'md')
  const index = useMemo(() => readIndex(readme.data ?? ''), [readme.data])
  const list = useMemo(() => runs(REPORTS, index), [index])
  const { loading, manifest } = useBenchManifest()
  const { sets } = useBenchSets()
  const cards = useMemo(() => cardsOf(manifest, list, REPORTS, LEAD, sets), [manifest, list, sets])
  // A card speaks in numbers when the run supports a pass rate and Theseus is one of its arms.
  const speaks = (c: Card) => !c.weak && !c.legacy && c.lines.some((l) => l.theseus)
  const measured = cards.filter(speaks)
  const rest = cards.filter((c) => !speaks(c))
  if (loading) return <div className="p-6 text-[13px] text-ink-faint">Loading…</div>
  return (
    <div className="flex flex-col gap-4">
      <Panel help="simple.bench.standing" title="Where Theseus stands" icon={<Trophy size={14} />}>
        {measured.length === 0 ? <Empty>No run in this build supports a pass rate for Theseus.</Empty> : (
          <div className="overflow-x-auto">
            <table className="w-full min-w-[880px] border-collapse text-left text-[13px]">
              <thead>
                <tr className="simple-eyebrow border-b border-line">
                  <th className="px-4 py-2 font-semibold">Benchmark</th>
                  <th className="px-3 py-2 font-semibold">Theseus</th>
                  <th className="px-3 py-2 font-semibold">Best measured beside it</th>
                  <th className="px-3 py-2 font-semibold">Where it stands</th>
                  <th className="px-3 py-2 font-semibold">As of</th>
                  <th className="px-4 py-2" />
                </tr>
              </thead>
              <tbody>{measured.map((c) => <Standing key={c.id} c={c} />)}</tbody>
            </table>
          </div>
        )}
      </Panel>
      <Panel help="simple.bench.findings" title="What every other run found" icon={<FileText size={14} />} bodyClassName="divide-y divide-line">
        {rest.length === 0 ? <Empty>Every run in this build is in the table above.</Empty> : rest.map((c) => (
          <div key={c.id} className="flex flex-wrap items-baseline gap-x-4 gap-y-1 px-4 py-2.5">
            <div className="w-[230px] shrink-0">
              <div className="text-[13.5px] font-semibold text-ink">{c.title}{c.tier ? <span className="font-normal text-ink-dim">, {c.tier} tier</span> : null}</div>
              <div className="text-[12px] text-ink-faint">{c.asOf ? `as of ${c.asOf}` : 'undated'}{c.legacy ? ' · an earlier run, kept as history' : ''}</div>
            </div>
            <div className="min-w-[280px] flex-1">
              <p className="line-clamp-3 text-[13px] leading-snug text-ink-dim" title={c.finding ?? undefined}>{c.finding ?? c.noMetric ?? c.headline ?? ''}</p>
              {c.weak && <p className="mt-0.5 text-[12px] text-ink-faint">No pass rate is led with: {c.weak}.</p>}
            </div>
            <Open c={c} />
          </div>
        ))}
      </Panel>
    </div>
  )
}

function Standing({ c }: { c: Card }) {
  const mine = c.lines.find((l) => l.theseus)!
  // The best arm that is not Theseus: the lines are best first.
  const other = c.lines.find((l) => !l.theseus)
  const s = other ? standing(mine, other) : null
  return (
    <tr className="border-b border-line/60 align-top last:border-b-0">
      <td className="px-4 py-3">
        <div className="text-[13.5px] font-semibold text-ink">{c.title}{c.tier ? <span className="font-normal text-ink-dim">, {c.tier} tier</span> : null}</div>
        <div className="text-[12px] text-ink-faint">{c.arms !== null ? `${c.arms} ${c.arms === 1 ? 'arm' : 'arms'} measured` : ''}</div>
      </td>
      <td className="px-3 py-3"><Rate l={mine} strong /></td>
      <td className="px-3 py-3">{other ? <Rate l={other} named /> : <span className="text-ink-faint">nothing else was measured</span>}</td>
      <td className="px-3 py-3">{s ? <Pill tone={s.tone} className="!whitespace-normal">{s.words}</Pill> : <span className="text-ink-faint">no comparison</span>}</td>
      <td className="num px-3 py-3 text-[12.5px] text-ink-dim">{c.asOf || '—'}</td>
      <td className="px-4 py-3 text-right"><Open c={c} /></td>
    </tr>
  )
}

/** One arm's pass rate with its interval and what is behind it; a different model than Theseus ran is said. */
function Rate({ l, strong, named }: { l: ArmLine; strong?: boolean; named?: boolean }) {
  const r = l.row
  return (
    <div>
      {named && <div className="text-[12.5px] text-ink">{l.name}{l.framing === 'B' && <span className="text-ink-faint"> · a different model</span>}</div>}
      <div className={cn('num', strong ? 'text-[18px] font-semibold text-ink' : 'text-[14px] text-ink')}>
        {pc(r.pass.value)} <span className="text-[12px] font-normal text-ink-faint">[{pc(r.pass.lo)}, {pc(r.pass.hi)}]</span>
      </div>
      <div className="text-[12px] text-ink-faint">{r.trials} trials{r.tasks !== null ? ` on ${r.tasks} tasks` : ''}{l.chip ? ` · ${l.chip}` : ''}{strong && l.framing === 'T' ? ' · all models, its own choice' : ''}</div>
    </div>
  )
}

function Open({ c }: { c: Card }) {
  return c.report ? <Link to={`/benchmarks/${c.report}`} className="shrink-0 whitespace-nowrap text-[12.5px] text-live hover:underline">its report →</Link> : null
}
