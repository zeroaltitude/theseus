// The browse surface (theseus-sam3s): one card per benchmark above the frontier, newest first, the omnibus pinned.
// A measured benchmark shows who is on top with its trials and tasks; every other suite shows its newest report and
// says there is no comparable headline metric yet. The cards are `lib/benchcards.ts`.
import { Link } from 'react-router'
import type { ArmLine, Card } from '@/lib/benchcards'
import { byFraming } from '@/lib/benchframing'
import { cn } from '@/lib/format'

const pc = (x: number) => `${(x * 100).toFixed(1)}%`

export function BenchIndex({ cards, reason, planned = [] }: { cards: Card[]; reason: string | null; planned?: string[] }) {
  return (
    <section className="panel px-4 py-3" aria-label="the benchmarks">
      <h2 className="font-display text-[11px] font-bold uppercase tracking-[0.12em] text-gold/90">The benchmarks</h2>
      <p className="mt-0.5 text-[11px] text-ink-faint">
        one card each, newest first. A pass rate appears only where the manifest holds one with its interval; every other suite says so.
        {reason && <> The manifest is not shown: {reason}.</>}
      </p>
      <div className="mt-2 grid items-start gap-2 md:grid-cols-2 xl:grid-cols-3">
        {cards.map((c) => <CardView key={c.id} c={c} />)}
      </div>
      {planned.length > 0 && <p className="mt-2 text-[11px] text-ink-faint">not yet measured: {planned.join(', ')}</p>}
    </section>
  )
}

function CardView({ c }: { c: Card }) {
  const top = c.lines[0]
  // Theseus is named first in the lead whatever its place (it is always labelled); the arm on top follows when it is another.
  const mine = c.lines.find((l) => l.theseus)
  const lead = (l: ArmLine) => (
    <>
      <b className="font-semibold text-ink">{l.name}</b> {pc(l.row.pass.value)}{' '}
      <span className="text-ink-faint">[{pc(l.row.pass.lo)}, {pc(l.row.pass.hi)}]</span>, {l.row.trials} trials{l.row.tasks ? `, ${l.row.tasks} tasks` : ''}
      {/* OWNER CALL: whether Theseus-general is the as-shipped arm or a line beside the Claude-routed series is open; this chip describes it and chooses nothing. */}
      {l.framing === 'T' && <Chip>all models, its own choice</Chip>}
      {l.chip && <Chip>{l.chip}</Chip>}
    </>
  )
  const blocks = byFraming(c.lines, (l) => l.framing)
  return (
    <article className={cn('rounded-md bg-white/[0.03] px-3 py-2 ring-1', c.kind === 'pinned' ? 'ring-gold/50' : 'ring-line')} aria-label={c.title}>
      <h3 className="text-[13px] font-semibold text-ink">
        {c.title}{c.tier ? <span className="font-normal text-ink-dim">, {c.tier} tier</span> : null}
        {c.kind === 'pinned' && <span className="ml-2 rounded bg-gold/15 px-1.5 py-px text-[10px] font-medium uppercase tracking-wide text-gold">start here</span>}
      </h3>
      <div className="text-[11px] text-ink-faint">
        {c.asOf ? <>as of {c.asOf}</> : null}{c.arms !== null && <> · {c.arms} arm{c.arms === 1 ? '' : 's'}</>}
      </div>
      {mine && <p className="mt-1 text-[12px] text-ink-dim">{lead(mine)}</p>}
      {top && top !== mine && <p className={cn('text-[12px] text-ink-dim', mine ? 'mt-0.5' : 'mt-1')}>on top: {lead(top)}</p>}
      {c.lines.length > 1 && blocks.map((b) => (
        <div key={b.framing} className="mt-1">
          {blocks.length > 1 && <div className="text-[10px] font-semibold uppercase tracking-wide text-ink-faint">{b.words}{b.items.length === 1 ? ' · one arm measured so far' : ''}</div>}
          <ul className="space-y-px text-[11px] text-ink-dim" aria-label={b.words}>
            {b.items.map((l) => (
              <li key={l.arm}>
                <span className={l.theseus ? 'text-ink' : undefined}>{l.name}</span> {pc(l.row.pass.value)}
                <span className="text-ink-faint"> · {l.row.trials} trials{l.row.tasks ? `, ${l.row.tasks} tasks` : ''} · {l.row.date}</span>
                {l.chip && <Chip>{l.chip}</Chip>}
              </li>
            ))}
          </ul>
        </div>
      ))}
      {c.noMetric && <p className="mt-1 text-[12px] text-ink-dim">{c.noMetric}</p>}
      {c.headline && <p className="mt-0.5 text-[11px] text-ink-faint">newest report: {c.headline}</p>}
      {c.newer && (
        <p className="mt-0.5 text-[11px] text-ink-faint">
          a newer report, {c.newer.date}, is not in the manifest yet: <Link to={`/benchmarks/${c.newer.report}`} className="underline decoration-line underline-offset-2 hover:text-live">{c.newer.title}</Link>
        </p>
      )}
      <div className="mt-1.5 flex flex-wrap gap-x-3 text-[11px]">
        {c.report && <Link to={`/benchmarks/${c.report}`} className="text-ink-faint underline decoration-line underline-offset-2 hover:text-live">its report</Link>}
        {c.taskSet && <Link to={`/benchmarks?set=${encodeURIComponent(c.taskSet)}&tasks=on`} className="text-ink-faint underline decoration-line underline-offset-2 hover:text-live">every test</Link>}
        {c.github && <a href={c.github} target="_blank" rel="noreferrer" className="text-ink-faint underline decoration-line underline-offset-2 hover:text-live">on GitHub</a>}
      </div>
    </article>
  )
}

const Chip = ({ children }: { children: React.ReactNode }) => (
  <span className="ml-1.5 rounded bg-white/10 px-1.5 py-px text-[10px] text-ink-dim">{children}</span>
)
