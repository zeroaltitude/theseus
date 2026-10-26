// The browse surface (theseus-sam3s): one card per benchmark above the frontier, newest first, the omnibus pinned.
// A measured benchmark shows who is on top with its trials and tasks; every other suite shows its newest report and
// says there is no comparable headline metric yet. The cards are `lib/benchcards.ts`.
import { Link } from 'react-router'
import type { Card } from '@/lib/benchcards'
import { cn } from '@/lib/format'

const pc = (x: number) => `${(x * 100).toFixed(1)}%`

export function BenchIndex({ cards, reason }: { cards: Card[]; reason: string | null }) {
  return (
    <section className="panel px-4 py-3" aria-label="the benchmarks">
      <h2 className="font-display text-[11px] font-bold uppercase tracking-[0.12em] text-gold/90">The benchmarks</h2>
      <p className="mt-0.5 text-[11px] text-ink-faint">
        one card each, newest first. A pass rate appears only where the manifest holds one with its interval; every other suite says so.
        {reason && <> The manifest is not shown: {reason}.</>}
      </p>
      <div className="mt-2 grid gap-2 md:grid-cols-2 xl:grid-cols-3">
        {cards.map((c) => <CardView key={c.id} c={c} />)}
      </div>
    </section>
  )
}

function CardView({ c }: { c: Card }) {
  const top = c.lines[0]
  return (
    <article className={cn('rounded-md bg-white/[0.03] px-3 py-2 ring-1', c.kind === 'pinned' ? 'ring-gold/50' : 'ring-line')} aria-label={c.title}>
      <h3 className="text-[13px] font-semibold text-ink">
        {c.title}{c.tier ? <span className="font-normal text-ink-dim">, {c.tier} tier</span> : null}
        {c.kind === 'pinned' && <span className="ml-2 rounded bg-gold/15 px-1.5 py-px text-[10px] font-medium uppercase tracking-wide text-gold">start here</span>}
      </h3>
      <div className="text-[11px] text-ink-faint">
        {c.asOf ? <>as of {c.asOf}</> : null}{c.arms !== null && <> · {c.arms} arm{c.arms === 1 ? '' : 's'}</>}
      </div>
      {top && (
        <p className="mt-1 text-[12px] text-ink-dim">
          on top: <b className="font-semibold text-ink">{top.name}</b> {pc(top.row.pass.value)}{' '}
          <span className="text-ink-faint">[{pc(top.row.pass.lo)}, {pc(top.row.pass.hi)}]</span>, {top.row.trials} trials{top.row.tasks ? `, ${top.row.tasks} tasks` : ''}
          {top.chip && <Chip>{top.chip}</Chip>}
        </p>
      )}
      {c.lines.length > 1 && (
        <ul className="mt-1 space-y-px text-[11px] text-ink-dim" aria-label="every arm">
          {c.lines.map((l) => (
            <li key={l.arm}>
              <span className={l.theseus ? 'text-ink' : undefined}>{l.name}</span> {pc(l.row.pass.value)}
              <span className="text-ink-faint"> · {l.row.trials} trials{l.row.tasks ? `, ${l.row.tasks} tasks` : ''} · {l.row.date}</span>
              {l.chip && <Chip>{l.chip}</Chip>}
            </li>
          ))}
        </ul>
      )}
      {c.noMetric && <p className="mt-1 text-[12px] text-ink-dim">{c.noMetric}</p>}
      {c.headline && <p className="mt-0.5 text-[11px] text-ink-faint">newest report: {c.headline}</p>}
      {c.newer && (
        <p className="mt-0.5 text-[11px] text-ink-faint">
          a newer report, {c.newer.date}, is not in the manifest yet: <Link to={`/benchmarks/${c.newer.report}`} className="underline decoration-line underline-offset-2 hover:text-live">{c.newer.title}</Link>
        </p>
      )}
      <div className="mt-1.5 flex flex-wrap gap-x-3 text-[11px]">
        {c.report && <Link to={`/benchmarks/${c.report}`} className="text-ink-faint underline decoration-line underline-offset-2 hover:text-live">its report</Link>}
        {c.github && <a href={c.github} target="_blank" rel="noreferrer" className="text-ink-faint underline decoration-line underline-offset-2 hover:text-live">on GitHub</a>}
      </div>
    </article>
  )
}

const Chip = ({ children }: { children: React.ReactNode }) => (
  <span className="ml-1.5 rounded bg-white/10 px-1.5 py-px text-[10px] text-ink-dim">{children}</span>
)
