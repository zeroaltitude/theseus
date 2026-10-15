// The tool families per place (theseus-7m7w.2): which a kind of place loads and which it defers, why, and what its use
// and misses are, over `tool.defer`. Read on a timer, as the postures are; the time machine does not reach it (the
// selector's counts are the present's).
import { useState } from 'react'
import { Layers } from 'lucide-react'
import type { DeferListResult } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { cn } from '@/lib/format'
import { orderFamilies, placeWords, reachWords, usageWords } from '@/lib/families'
import { Empty, Panel, Pill } from '@/components/ui'

export function ToolFamilies() {
  const { data } = useRpc<DeferListResult>('tool.defer', undefined, 5000)
  const [place, setPlace] = useState<string | null>(null)
  const places = data?.places ?? []
  const shown = places.find((p) => p.place === place) ?? places[0]
  return (
    <Panel help="actions.families" title="Tool families" icon={<Layers size={13} />} bodyClassName="max-h-[520px] overflow-auto"
      actions={data ? <span className="text-[11px] text-ink-faint" title={`a family loads at ${(data.load_share * 100).toFixed(0)}% of a place kind’s turns over ${data.window_days} days, and hides after ${data.hide_after_days} unused`}>{reachWords(data)}</span> : null}>
      {!data ? <Empty>reading the families…</Empty> : !shown ? <Empty>no places</Empty> : (
        <>
          <div className="flex flex-wrap items-center gap-1 border-b border-line/50 px-3 py-1.5">
            {places.map((p) => (
              <button key={p.place} type="button" aria-pressed={p.place === shown.place} onClick={() => setPlace(p.place)}
                className={cn('num rounded-md px-1.5 py-0.5 text-[11px] ring-1 ring-inset', p.place === shown.place ? 'bg-white/[0.06] text-ink ring-live/40' : 'text-ink-dim ring-line hover:text-ink')}>{p.place}</button>
            ))}
            <span className="num ml-2 text-[11px] text-ink-faint">{placeWords(shown)}{data.warmed ? '' : ' · the ledger is not read yet'}</span>
          </div>
          <table className="w-full whitespace-nowrap text-[12px]">
            <thead className="sticky top-0 bg-hull/95 text-[10px] uppercase tracking-wider text-ink-faint backdrop-blur">
              <tr><th className="px-3 py-1.5 text-left">family</th><th className="px-2 py-1.5 text-left">now</th><th className="px-2 py-1.5 text-left">why</th><th className="px-2 py-1.5 text-left">use</th></tr>
            </thead>
            <tbody>
              {orderFamilies(shown).map((f) => (
                <tr key={f.family} className="border-t border-line/50 hover:bg-white/[0.02]" title={`${f.summary}\n\n${f.tools.join('\n')}`}>
                  <td className="num px-3 py-1 text-tool">{f.family} <span className="text-ink-faint">· {f.tools.length}</span></td>
                  <td className="px-2 py-1"><Pill tone={f.loaded ? 'ok' : 'wait'}>{f.loaded ? 'loaded' : 'deferred'}</Pill></td>
                  <td className="px-2 py-1 text-ink-dim">{f.why}</td>
                  <td className={cn('px-2 py-1', f.missed > 0 ? 'text-wait' : 'text-ink-faint')}>{usageWords(f)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
    </Panel>
  )
}
