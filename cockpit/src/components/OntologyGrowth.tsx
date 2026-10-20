// What the machines wrote into the ontology (theseus-dd3kw, R7): the growth chart over the ledger's `ontology.*` rows, and
// the list of what a machine origin minted, each with its judgment's link and an undo. It needs no read of its own: the
// ledger is the page's shared history (`useHistoryRows`), so the chart is every row there is. Its pure half is
// `lib/ontology.ts`, tested by `test/ontology.test.ts`.
import { useMemo, useState } from 'react'
import { Link } from 'react-router'
import { ChartColumnStacked, Sprout, Undo2 } from 'lucide-react'
import type { OntologyUndoParams, OntologyUndoResult } from '@protocol'
import { confidenceWords, growthOf, growthWords, judgmentHref, mintedOf, seriesOf, undoAsk, type Minted } from '@/lib/ontology'
import { useHistoryRows } from '@/lib/history'
import { cn, ago } from '@/lib/format'
import { CHROME, MARK, TIME_LABELS, TIP_FRAME, TONE_MARK, baseAxis, barRadius, countTick, slotColor, slots, stackTop, valueAxis } from '@/lib/viz'
import { tip, type TipRow } from '@/lib/viztip'
import type { EChartsOption } from '@/lib/chart'
import { Echart } from '@/components/Echart'
import { Empty, Panel, Pill } from '@/components/ui'
import { Act, Refused, useOntologyWrite } from '@/components/OntologyParts'

const C = CHROME.dark

/** The growth chart: columns, a day each, stacked by the origin that made the category; the kinds added as a line over
 *  its own count; and a day on which a write was later undone marked by a ring in the fault tone, with the count in its tip. */
export function GrowthPanel() {
  const { rows, ready, error } = useHistoryRows()
  // The window ends at the day the view was opened (a page left open past midnight reads on from it until reloaded).
  const [now] = useState(() => new Date())
  const growth = useMemo(() => growthOf(rows, now), [rows, now])
  const s = useMemo(() => seriesOf(growth), [growth])
  const option = useMemo<EChartsOption>(() => {
    const { slot } = slots(s.stacks.map((x) => x.origin))
    const color = (o: string) => slotColor(slot, o)
    const tops = stackTop(s.stacks.map((x) => x.values))
    const axis = baseAxis(), vaxis = valueAxis()
    const top = Math.max(1, s.max, ...s.kinds)
    return {
      grid: { left: 36, right: 14, top: 16, bottom: 24 },
      tooltip: {
        ...TIP_FRAME, trigger: 'axis', axisPointer: { type: 'line', lineStyle: { color: C.axis, width: 1, type: 'solid' } },
        formatter: (ps: any) => {
          const i = (Array.isArray(ps) ? ps[0] : ps)?.dataIndex
          if (i === undefined || s.days[i] === undefined) return ''
          const r: TipRow[] = s.stacks.filter((x) => x.values[i] > 0).map((x) => ({ value: String(x.values[i]), label: `${x.origin} categories`, color: color(x.origin), mark: 'rect' }))
          if (s.kinds[i]) r.push({ value: String(s.kinds[i]), label: 'kinds added', color: C.text, mark: 'line' })
          if (s.undone[i]) r.push({ value: String(s.undone[i]), label: 'since taken back', color: TONE_MARK.fault, mark: 'ring' })
          return tip(s.days[i], r.length ? r : [{ value: '0', label: 'nothing made', strong: false }])
        },
      },
      xAxis: { type: 'category', data: s.days, ...axis, axisLabel: { ...axis.axisLabel, formatter: (d: string) => TIME_LABELS.day.replace('{MMM}', d.slice(5, 7)).replace('{d}', d.slice(8)) } },
      yAxis: { ...vaxis, min: 0, max: Math.max(2, top), minInterval: 1, axisLabel: { ...vaxis.axisLabel, formatter: countTick } },
      series: [
        ...s.stacks.map((x, i) => ({
          type: 'bar' as const, name: x.origin, stack: 'made', barMaxWidth: MARK.bar, itemStyle: { color: color(x.origin), borderColor: C.surface, borderWidth: MARK.gap / 2 },
          data: x.values.map((v, j) => ({ value: v, itemStyle: { borderRadius: tops[j] === i ? barRadius(false) : 0 } })),
        })),
        { type: 'line' as const, name: 'kinds added', data: s.kinds, symbol: 'circle', symbolSize: MARK.marker, showSymbol: false, lineStyle: { width: MARK.line, color: C.text }, itemStyle: { color: C.text } },
        {
          type: 'scatter' as const, name: 'taken back', symbol: 'circle', symbolSize: MARK.marker + 2 * MARK.ring, z: 4,
          itemStyle: { color: 'transparent', borderColor: TONE_MARK.fault, borderWidth: MARK.ring },
          data: s.undone.map((n, j) => (n > 0 ? [s.days[j], (s.stacks.reduce((a, x) => a + x.values[j], 0)) + 0.35] : null)).filter((p) => p !== null),
        },
      ],
    }
  }, [s])
  const legend = slots(s.stacks.map((x) => x.origin)).slot
  return (
    <Panel help="ontology.growth" title={<>growth · {growth.made.toLocaleString()} made · {growth.kinds.toLocaleString()} kinds</>} icon={<ChartColumnStacked size={13} />} bodyClassName="p-2">
      {error ? <Empty>the ledger could not be read: {error}</Empty> : !ready && !rows.length ? <Empty>reading the ledger…</Empty> : (
        <div className="flex flex-col gap-1">
          <div className="num flex flex-wrap items-center gap-x-3 gap-y-1 px-1 text-[11px] text-ink-faint">
            <span>{growthWords(growth)}</span>
            {s.stacks.map((x) => <span key={x.origin} className="inline-flex items-center gap-1"><span className="inline-block h-2 w-2 rounded-sm" style={{ background: slotColor(legend, x.origin) }} />{x.origin}</span>)}
            <span className="inline-flex items-center gap-1"><span className="inline-block h-0.5 w-3" style={{ background: C.text }} />kinds added</span>
            <span className="inline-flex items-center gap-1"><span className="inline-block h-2 w-2 rounded-full ring-2 ring-inset" style={{ boxShadow: `inset 0 0 0 2px ${TONE_MARK.fault}` }} />taken back</span>
          </div>
          <div className="h-48" role="img" aria-label={growthWords(growth)}><Echart option={option} /></div>
        </div>
      )}
    </Panel>
  )
}

/** What a machine origin minted, newest first: the category, its origin and confidence, when, the session it filed, and the
 *  judgment's link. An undo is `ontology.undo` of the write, confirmed with the words it sends; an undone mint is struck
 *  through, with when it was taken back. One write may mint several categories, and an undo takes them all. */
export function MintedPanel({ disabled }: { disabled: boolean }) {
  const { rows } = useHistoryRows()
  const minted = useMemo(() => mintedOf(rows), [rows])
  const { write, busy, refused } = useOntologyWrite()
  const undo = async (m: Minted) => {
    const p: OntologyUndoParams = { write: m.write }
    await write<OntologyUndoResult>(undoAsk(m), 'ontology.undo', p)
  }
  const live = minted.filter((m) => !m.undone).length
  return (
    <Panel help="ontology.minted" title={<>minted by machines · {live.toLocaleString()}{live !== minted.length && <span className="text-ink-faint"> of {minted.length.toLocaleString()}</span>}</>} icon={<Sprout size={13} />} bodyClassName="max-h-96 overflow-auto">
      {minted.length === 0 ? <Empty>nothing minted yet: Jev, the sweep and the dream write here when they make a category</Empty> : (
        <div className="flex flex-col gap-2 p-2">
          <Refused words={refused} />
          <table className="w-full text-[11.5px]">
            <thead className="text-[10px] uppercase tracking-wider text-ink-faint">
              <tr><th className="py-1 text-left">category</th><th className="px-1 text-left">by</th><th className="px-1 text-right" title="the origin's confidence in the membership it set">conf.</th><th className="px-1 text-left">session</th><th className="px-1 text-left">when</th><th className="px-1 text-left">why</th><th className="pl-1 text-right" /></tr>
            </thead>
            <tbody>
              {minted.map((m) => (
                <tr key={`${m.write}:${m.category}`} className="border-t border-line/50">
                  <td className={cn('py-1', m.undone && 'text-ink-faint line-through')}>
                    <span className="text-ink">{m.name}</span>
                    <div className="num text-[10px] text-ink-faint">{m.category}</div>
                  </td>
                  <td className="px-1"><Pill tone={m.origin === 'jev' ? 'think' : 'idle'}>{m.origin}</Pill></td>
                  <td className="num px-1 text-right text-ink-dim">{confidenceWords(m.confidence)}</td>
                  <td className="num px-1 text-ink-faint">{m.session ? <Link className="hover:text-live" to={`/session/${encodeURIComponent(m.session)}`}>{m.session}</Link> : '—'}</td>
                  <td className="num whitespace-nowrap px-1 text-ink-faint" title={new Date(m.at).toLocaleString()}>{ago(m.at)}</td>
                  <td className="px-1"><Link className="num text-ink-faint hover:text-live" to={judgmentHref(m.write)} title="the judgment that made it">{m.write}</Link></td>
                  <td className="whitespace-nowrap pl-1 text-right">
                    {m.undone
                      ? <span className="text-[11px] text-wait" title={m.undoneAt ? new Date(m.undoneAt).toLocaleString() : undefined}>taken back{m.undoneAt ? ` ${ago(m.undoneAt)}` : ''}</span>
                      : <Act onClick={() => void undo(m)} busy={busy} off={disabled} title="ontology.undo: confirmed first, with the words it sends"><Undo2 size={12} /> Undo</Act>}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Panel>
  )
}
