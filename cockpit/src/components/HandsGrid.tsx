// The hands' grid (AWS design §3.3, "Watching a hundred hands"; step 40 part 2, theseus-mgw.11): one row per group from
// `hands.list`, a cell per hand coloured by its state, its cost against its cap, and its one line as Discord shows it.
import { Hand } from 'lucide-react'
import type { HandsGroupInfo, HandsListResult } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { ago } from '@/lib/format'
import { Empty, Field, Panel, Pill } from './ui'

/** Each cell's colour and its word, by the hand's state. */
const CELL: Record<string, [string, string]> = {
  waiting: ['var(--color-ink-faint, #6b7280)', 'not launched yet'],
  running: ['var(--color-live)', 'running'],
  stopping: ['var(--color-wait)', 'being stopped'],
  succeeded: ['var(--color-ok)', 'succeeded'],
  failed: ['var(--color-fault)', 'failed'],
  unknown: ['var(--color-wait)', 'unknown'],
  cancelled: ['var(--color-brass)', 'cancelled after its launch'],
  not_launched: ['transparent', 'never launched'],
}

const usd = (micros: number) => `$${(micros / 1e6).toFixed(micros >= 10e6 ? 0 : 2)}`

function Group({ g, now }: { g: HandsGroupInfo; now: number }) {
  const cap = g.cap_micros ?? g.worst_micros
  const share = cap > 0 ? Math.min(1, g.spent_micros / cap) : 0
  const tone = g.settled === undefined ? 'wait' : g.settled === 'met' ? 'ok' : g.settled === 'cancelled' ? 'idle' : 'fault'
  return (
    <div className="mb-2 border-b border-line/50 pb-2 last:mb-0 last:border-0 last:pb-0">
      <div className="flex items-center gap-2">
        <span className="num text-[12px] text-ink">{g.group.slice(-8)}</span>
        <Pill tone={tone}>{g.settled ?? 'open'}</Pill>
        <span className="text-[11px] text-ink-faint">{g.backend} · until {g.until} · {ago(g.created_at_unix_ms, now)}</span>
      </div>
      <div className="mt-1 flex flex-wrap gap-[2px]" role="img" aria-label={`${g.cells.length} hands`}>
        {g.cells.map((c, i) => {
          const [colour, words] = CELL[c] ?? ['var(--color-wait)', c]
          return <span key={i} title={`#${i} ${words}`} className="inline-block h-2.5 w-2.5 rounded-[2px] ring-1 ring-inset ring-line" style={{ background: colour }} />
        })}
      </div>
      <Field label={g.cap_micros !== undefined ? 'spent of its cap' : 'spent of its worst case'} mono>
        {usd(g.spent_micros)} of {usd(cap)}
        {g.reserved_micros > 0 && <span className="text-ink-faint"> ({usd(g.reserved_micros)} reserved)</span>}
        <span className="ml-2 inline-block h-1.5 w-24 rounded bg-line/50 align-middle">
          <span className="block h-1.5 rounded" style={{ width: `${share * 100}%`, background: 'var(--color-money)' }} />
        </span>
      </Field>
      <div className="text-[11px] text-ink-faint">{g.line}</div>
    </div>
  )
}

/** The newest hands groups, read every 3 s while the Systems view is open. */
export function HandsGrid({ now }: { now: number }) {
  const { data } = useRpc<HandsListResult>('hands.list', { limit: 10 }, 3000)
  if (!data) return null
  return (
    <Panel title="AWS hands" icon={<Hand size={13} />} bodyClassName="px-3.5 py-2.5">
      {data.groups.length === 0 && <Empty>no hands group yet</Empty>}
      {data.groups.map((g) => <Group key={g.group} g={g} now={now} />)}
    </Panel>
  )
}
