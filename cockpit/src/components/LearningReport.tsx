// The learning report (M5 25c; design §2.9, §2.13): per pack version and question, calls, labels, precision and recall
// per class, calibration (Brier, ECE, and a reliability strip), agreement with the baseline, bands, cost, latency per
// class, and the frozen holdout, as the core computed them (`learning.report`). It reads the report stored for the day
// shown (`?report=<date>`, today by default); "run now" runs it, confirmed first, since a run writes its rows.
import { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import type { LearningReport as Report, PackReport, QuestionReport } from '@protocol'
import { call, useRpc } from '@/lib/rpc'
import { cn, ms, usd } from '@/lib/format'
import { Empty } from '@/components/ui'

const pct = (v: number | null | undefined) => (v == null ? '—' : `${Math.round(v * 100)}%`)

export function LearningReport({ date, pack, onDate }: { date: string; pack: string; onDate: (d: string) => void }) {
  const qc = useQueryClient()
  const [busy, setBusy] = useState(false)
  const { data, error } = useRpc<Report>('learning.report', { date, pack: pack || null }, 60_000)
  const run = async () => {
    if (!window.confirm('Run the learning report now? It derives the system labels and writes the report\'s rows.')) return
    setBusy(true)
    try { const r = await call<Report>('learning.report', {}); onDate(r.date); await qc.invalidateQueries() } catch (x: any) { window.alert(x?.message ?? String(x)) } finally { setBusy(false) }
  }
  return (
    <div className="flex flex-col gap-2 p-3 text-[12px]">
      <div className="flex items-center gap-2 text-[11px] text-ink-faint">
        <span>day</span>
        <input type="date" value={date} onChange={(e) => e.target.value && onDate(e.target.value)} className="num rounded bg-transparent px-1 ring-1 ring-line" />
        <button disabled={busy} onClick={run} className="ml-auto rounded px-1.5 ring-1 ring-line hover:text-live disabled:opacity-50">run now</button>
      </div>
      {error ? <Empty>{String((error as { message?: string }).message ?? error)}</Empty> : !data ? <Empty>reading…</Empty> : (
        <>
          <div className="num text-[11px] text-ink-faint">
            {data.date} ({data.trigger}) · labels: {data.labels.operator} operator, {data.labels.system} system, {data.labels.audit} audit
          </div>
          {!data.packs.length && <Empty>no judgments to report</Empty>}
          {data.packs.map((p) => <PackBlock key={p.pack} p={p} />)}
        </>
      )}
    </div>
  )
}

function PackBlock({ p }: { p: PackReport }) {
  const h = p.holdout
  return (
    <div className="border-t border-line pt-2">
      <div className="num flex flex-wrap gap-x-3 text-ink">
        <span className="font-medium">{p.pack}</span>
        <span>{p.calls} calls ({p.answered} answered)</span>
        <span>{p.labeled} labeled</span>
        <span title={`against the baseline, ${p.baseline}: ${p.disagreements} disagree`}>agrees {pct(p.agreement)}</span>
        <span>{usd(p.cost_usd, 6)}</span>
      </div>
      {p.latency.map((l) => (
        <div key={l.class} className="num text-[11px] text-ink-faint">latency {l.class} ({l.n}): p50 {ms(l.p50_ms)} · p95 {ms(l.p95_ms)} · p99 {ms(l.p99_ms)}</div>
      ))}
      {p.questions.map((q) => <QuestionRow key={q.question} q={q} />)}
      <div className={cn('num mt-1 text-[11px]', h.sufficient ? 'text-live' : 'text-wait')}>
        holdout (frozen, {Math.round((h.end_ms - h.start_ms) / 86_400_000)} days): {h.judgments.length} judgments, {h.labels.length} labels · {h.insufficient ?? 'sufficient'}
      </div>
    </div>
  )
}

function QuestionRow({ q }: { q: QuestionReport }) {
  const c = q.calibration
  return (
    <div className="mt-1">
      <div className="num flex flex-wrap gap-x-3 text-[11.5px]">
        <span className={q.decides ? 'text-ink' : 'text-ink-faint'}>{q.question}{q.decides ? ' *' : ''}</span>
        <span className="text-ink-faint">{q.kind} · {q.answered} answered · {q.labeled} labeled</span>
        <span className="text-ink-faint">{q.bands.filter((b) => b.n).map((b) => `${b.band} ${pct(b.share)}`).join(' · ')}</span>
        {c && <span>Brier {c.brier.toFixed(3)} · ECE {c.ece.toFixed(3)}</span>}
      </div>
      {q.classes.filter((k) => k.predicted + k.actual > 0).map((k) => (
        <div key={k.class} className="num pl-3 text-[11px] text-ink-faint">{k.class}: precision {pct(k.precision)} ({k.predicted}) · recall {pct(k.recall)} ({k.actual})</div>
      ))}
      {c && c.n > 0 && (
        <div className="flex h-5 items-end gap-px pl-3" title="reliability: each bin's outcome rate (bar) against its mean p (line)">
          {c.bins.map((b) => (
            <div key={b.lo} className="relative h-full w-4 bg-line/30" title={`${b.lo.toFixed(1)}–${b.hi.toFixed(1)}: ${b.n} · ${pct(b.frequency)} true · mean p ${b.mean_p.toFixed(2)}`}>
              {b.n > 0 && <div className="absolute bottom-0 w-full bg-live/70" style={{ height: `${Math.round(b.frequency * 100)}%` }} />}
              {b.n > 0 && <div className="absolute w-full border-t border-wait" style={{ bottom: `${Math.round(b.mean_p * 100)}%` }} />}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
