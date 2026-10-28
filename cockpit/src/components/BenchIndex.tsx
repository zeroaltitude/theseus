// The browse surface (theseus-sam3s): one card per benchmark above the frontier, newest first, the omnibus pinned.
// A measured benchmark shows who is on top with its trials and tasks, but only where the run supports a headline
// (`headlineWhy`); every other card shows what its run found. The cards are `lib/benchcards.ts`.
//
// The cards flow down columns rather than sitting in grid rows (theseus-s5qwv): a grid row is as tall as its tallest
// card, which left wide gaps of empty space beside the short ones. Each card is kept whole inside its column, in the
// page's own masonry (`views/Systems.tsx`). The columns are why the prose below says which way "newest" runs.
import { Link } from 'react-router'
import { LEGACY_WORDS, type ArmLine, type Card } from '@/lib/benchcards'
import type { Skipped } from '@/lib/benchindex'
import { byFraming } from '@/lib/benchframing'
import { cn } from '@/lib/format'

const pc = (x: number) => `${(x * 100).toFixed(1)}%`
const n = (c: number, w: string) => `${c} ${w}${c === 1 ? '' : 's'}`
const counts = (r: ArmLine['row']) => `${n(r.trials, 'trial')}${r.tasks ? `, ${n(r.tasks, 'task')}` : ''}`

export function BenchIndex({ cards, reason, planned = [], skipped = [] }: { cards: Card[]; reason: string | null; planned?: string[]; skipped?: readonly Skipped[] }) {
  return (
    <section className="panel px-4 py-3" aria-label="the benchmarks">
      <h2 className="font-display text-[11px] font-bold uppercase tracking-[0.12em] text-gold/90">The benchmarks</h2>
      <p className="mt-0.5 text-[11px] text-ink-faint">
        one card each, newest first down each column. A pass rate appears only where the run supports one — the
        manifest holds it with its interval, over more than one task and enough trials; every other card says what
        its run found. A run from before its benchmark's tiers reads last, labelled.
        {reason && <> The manifest is not shown: {reason}.</>}
      </p>
      <div className="mt-2 columns-1 gap-2 md:columns-2 xl:columns-3 [&>*]:mb-2 [&>*]:break-inside-avoid">
        {cards.map((c) => <CardView key={c.id} c={c} />)}
      </div>
      {planned.length > 0 && <p className="mt-2 text-[11px] text-ink-faint">not yet measured: {planned.join(', ')}</p>}
      {/* A run the manifest holds but this reader cannot score is said here, so it is never silently absent. */}
      {skipped.length > 0 && (
        <p className="mt-1 text-[11px] text-ink-faint">
          measured another way, so no pass rate here: {skipped.map((s) => `${s.benchmark} (${s.report})`).join(', ')}
        </p>
      )}
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
      <span className="text-ink-faint">[{pc(l.row.pass.lo)}, {pc(l.row.pass.hi)}]</span>, {counts(l.row)}
      {/* OWNER CALL: whether Theseus-general is the as-shipped arm or a line beside the Claude-routed series is open; this chip describes it and chooses nothing. */}
      {l.framing === 'T' && <Chip>all models, its own choice</Chip>}
      {l.chip && <Chip>{l.chip}</Chip>}
    </>
  )
  const blocks = byFraming(c.lines, (l) => l.framing)
  return (
    <article
      className={cn('rounded-md bg-white/[0.03] px-3 py-2 ring-1',
        c.kind === 'pinned' ? 'ring-gold/50' : c.legacy ? 'ring-line/50' : 'ring-line')}
      aria-label={c.title}
    >
      <h3 className="text-[13px] font-semibold text-ink">
        {c.title}{c.tier ? <span className="font-normal text-ink-dim">, {c.tier} tier</span> : null}
        {c.kind === 'pinned' && <span className="ml-2 rounded bg-gold/15 px-1.5 py-px text-[10px] font-medium uppercase tracking-wide text-gold">start here</span>}
        {c.legacy && <Chip>{LEGACY_WORDS}</Chip>}
      </h3>
      <div className="text-[11px] text-ink-faint">
        {c.asOf ? <>as of {c.asOf}</> : null}{c.arms !== null && <> · {c.arms} arm{c.arms === 1 ? '' : 's'}</>}
      </div>
      {/* A run the numbers do not speak for leads with what it found; its arms keep their numbers below. */}
      {c.weak
        ? <>
            <p className="mt-1 line-clamp-3 text-[12px] text-ink-dim">{c.finding ?? c.noMetric ?? ''}</p>
            {/* Its own line, never inside the clamped finding, or the reason is the first thing cut. */}
            <p className="mt-0.5 text-[11px] text-ink-faint">no headline: {c.weak}</p>
          </>
        : <>
            {mine && <p className="mt-1 text-[12px] text-ink-dim">{lead(mine)}</p>}
            {top && top !== mine && <p className={cn('text-[12px] text-ink-dim', mine ? 'mt-0.5' : 'mt-1')}>on top: {lead(top)}</p>}
          </>}
      {c.lines.length > (c.weak ? 0 : 1) && blocks.map((b) => (
        <div key={b.framing} className="mt-1">
          {blocks.length > 1 && <div className="text-[10px] font-semibold uppercase tracking-wide text-ink-faint">{b.words}{b.items.length === 1 ? ' · one arm measured so far' : ''}</div>}
          <ul className="space-y-px text-[11px] text-ink-dim" aria-label={b.words}>
            {b.items.map((l) => (
              <li key={l.arm}>
                <span className={l.theseus ? 'text-ink' : undefined}>{l.name}</span> {pc(l.row.pass.value)}
                {c.weak && <span className="text-ink-faint"> [{pc(l.row.pass.lo)}, {pc(l.row.pass.hi)}]</span>}
                <span className="text-ink-faint"> · {counts(l.row)} · {l.row.date}</span>
                {c.weak && l.framing === 'T' && <Chip>all models, its own choice</Chip>}
                {l.chip && <Chip>{l.chip}</Chip>}
              </li>
            ))}
          </ul>
        </div>
      ))}
      {/* A card with no measurement of its own says what its run found, and only then the old words. */}
      {!c.weak && c.lines.length === 0 && (c.finding
        ? <p className="mt-1 line-clamp-3 text-[12px] text-ink-dim">{c.finding}</p>
        : c.noMetric && <p className="mt-1 text-[12px] text-ink-dim">{c.noMetric}</p>)}
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
