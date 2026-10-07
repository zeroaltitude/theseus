// The Judgment section (M5 23b; design §2.13, "Web UI"): Jev's judgments as the daemon recorded them. Per pack, its
// mode and version, its calls and cost, and p50 and p95 of Jev's time per workload class; a log of judgments with
// filters; and one judgment whole, its state as fields and its answers as probability bars.
//
// - It reads only while it is open: `judge.list` (the newest 500 that match its filters, every few seconds) and, for
//   the judgment picked, `judge.get`, once. It keeps no copy of the ledger of its own. The daemon reads from the newest
//   back and stops one match past the limit, so "N of M+" says more match than it counted (`more`, theseus-wse2).
// - Its state is in the address: `?pack=`, `?session=`, `?since=` (1h, 24h, 7d), and `?id=` (the judgment open).
// - With the time machine set, the list stops at the moment, and the counts are the fold's (`World.judge`).
// - The learning report (25c; `?report=<date>`, the Learning panel) and a judgment's label buttons (its detail) are
//   `components/LearningReport.tsx` and `components/JudgmentLabels.tsx`; the ladder (26a: each pack's mode, rules
//   and rows, with promote and roll-back buttons) is `components/PackLadder.tsx`; the versions the learning loop
//   wrote (25f: each lineage, the diff between any two, `?va=` and `?vb=`, promote and reject) are
//   `components/PackVersions.tsx`. "disagrees"
//   here is the core's: an answered judgment whose pack, in its act band, would have done otherwise than the
//   baseline.
// - Jev's live notices (step 24's notices: `security.v3` sure an open call was risky) are the Notices panel: the
//   newest `tool.notified` rows `by: judge`, read while it is open, each with its label buttons, and health's word on
//   them (on, paused until a day and why, or off).
import { useMemo } from 'react'
import { useNavigate, useSearchParams } from 'react-router'
import { BellRing, GitCompare, GraduationCap, Gavel, ListFilter, Scale, TrendingUp } from 'lucide-react'
import type { Health, JudgeGetResult, JudgeListResult, LedgerEntry } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { useWorld } from '@/lib/world'
import { useTick } from '@/lib/hooks'
import { bars, judged, line, packStats, type Judged } from '@/lib/judgment'
import { cn, ms, short, stamp, usd } from '@/lib/format'
import { Empty, Field, Panel, Pill, Segmented } from '@/components/ui'
import { JsonView } from '@/components/JsonView'
import { JudgmentLabels } from '@/components/JudgmentLabels'
import { LearningReport } from '@/components/LearningReport'
import { PackLadder } from '@/components/PackLadder'
import { PackVersions } from '@/components/PackVersions'
import { packLine } from '@/lib/packs'
import { localDay } from '@/lib/learning'
import { noticeWords } from '@/lib/scores'

type D = Record<string, any>

const SINCE = ['all', '1h', '24h', '7d'] as const
type Since = (typeof SINCE)[number]
const SINCE_MS: Record<Since, number> = { all: 0, '1h': 3_600_000, '24h': 86_400_000, '7d': 604_800_000 }

const money = (micros: number | null | undefined) => (micros == null ? '—' : usd(micros / 1e6, 6))

export default function Judgment() {
  const [params, setParams] = useSearchParams()
  const nav = useNavigate()
  const pack = params.get('pack') ?? ''
  const session = params.get('session') ?? ''
  const since = (SINCE as readonly string[]).includes(params.get('since') ?? '') ? (params.get('since') as Since) : 'all'
  const id = params.get('id')
  const set = (k: string, v: string | null) => setParams((p) => { if (v) p.set(k, v); else p.delete(k); return p }, { replace: true })

  const world = useWorld()
  // The window's start moves on by the minute, not at every render.
  const now = useTick(60_000)
  const sinceMs = SINCE_MS[since] ? now - SINCE_MS[since] : undefined
  const { data: list, error } = useRpc<JudgeListResult>(
    'judge.list',
    { pack: pack || null, session_id: session || null, since: sinceMs ? Math.floor(sinceMs / 60_000) * 60_000 : null, limit: 500 },
    4000,
  )
  const { data: health } = useRpc<Health>('health', undefined, 5000)
  const all = useMemo(() => (list?.judgments ?? []).map(judged), [list])
  const shown = useMemo(() => (world ? all.filter((j) => j.at <= world.t) : all), [all, world])
  const stats = useMemo(() => packStats(shown), [shown])
  const modes = useMemo(() => {
    const m = new Map<string, string>()
    for (const p of health?.judge?.packs ?? []) { const { pack, mode } = packLine(p); m.set(pack, mode) }
    return m
  }, [health])
  const packs = useMemo(() => [...new Set([...modes.keys(), ...stats.map((s) => s.pack)])].sort(), [modes, stats])
  const h = health?.judge

  return (
    // From 1280 px wide, two columns, each scrolling on its own: the packs and the log on the left, the log taking the
    // height the packs leave; on the right the judgment picked, then the notices, the ladder, the versions and learning.
    // Every other panel keeps its own height: squeezed into one column, a panel's rows ran under the next one's title
    // at 1080 px tall (theseus-hnof). Narrower, one column, and the page scrolls.
    <div className="grid h-full min-h-0 grid-cols-1 content-start gap-3 overflow-auto p-3 xl:grid-cols-[minmax(0,1.25fr)_minmax(0,1fr)] xl:content-stretch xl:overflow-hidden">
      <div className="flex min-h-0 flex-col gap-3">
        <Panel className="shrink-0" title="Judgment" icon={<Scale size={14} />} actions={
          world
            ? <Pill tone="wait">as of {stamp(world.t)}: {world.judge.calls} calls · {money(world.judge.costMicros)}{world.judge.paused ? ' · paused' : ''}</Pill>
            : h && <Pill tone={h.enabled ? (h.paused ? 'wait' : 'ok') : 'idle'}>{h.enabled ? `breaker ${h.breaker}${(h.breakers ?? []).map((b) => ` · ${b.replace(': ', ' breaker ')}`).join('')} · key ${h.key || '?'}` : 'off'}</Pill>
        }>
          {!h?.enabled && !all.length ? (
            <Empty>the judge is off ([judge] enabled = false)</Empty>
          ) : (
            // The packs scroll under their own header past a third of the screen, so the log keeps its height.
            <div className="max-h-[34vh] overflow-auto">
            <table className="w-full text-[12px]">
              <thead className="sticky top-0 z-[1] bg-hull text-left text-ink-faint shadow-[inset_0_-1px_0_var(--color-line)]">
                <tr><th className="px-3 py-1.5">pack</th><th className="px-2">mode</th><th className="px-2">version</th><th className="px-2 text-right">calls</th><th className="px-2 text-right">cost</th><th className="px-3">Jev's time by class (p50 / p95)</th></tr>
              </thead>
              <tbody>
                {packs.map((p) => {
                  const s = stats.find((x) => x.pack === p)
                  return (
                    <tr key={p} className={cn('cursor-pointer border-t border-line hover:bg-gold/5', pack === p && 'bg-live/5')} onClick={() => set('pack', pack === p ? null : p)}>
                      <td className="num px-3 py-1.5 text-ink">{p}</td>
                      <td className="px-2"><Pill tone={modes.get(p) === 'off' ? 'idle' : modes.get(p) === 'shadow' ? 'think' : modes.get(p) === 'rolled back' ? 'fault' : 'live'}>{modes.get(p) ?? 'not wired'}</Pill></td>
                      <td className="num px-2">{s ? `v${s.version}` : '—'}</td>
                      <td className="num px-2 text-right" title={s ? `${s.answered} answered · ${s.failed} failed · ${s.skipped} skipped · ${s.disagrees} disagree` : ''}>{s?.calls ?? 0}</td>
                      <td className="num px-2 text-right">{money(s?.costMicros ?? 0)}</td>
                      <td className="num px-3 text-ink-faint">
                        {s?.classes.length ? s.classes.map((c) => `${c.cls} ${ms(c.p50)} / ${ms(c.p95)} (${c.calls})`).join(' · ') : '—'}
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
            </div>
          )}
          {h?.enabled && !world && (
            <div className="num border-t border-line px-3 py-1.5 text-[11px] text-ink-faint">
              today ({h.day}): {h.calls_today} calls · {h.failed_today} failed · {h.skipped_today} skipped · {h.shed} shed · {usd(h.spend_today_usd, 6)} of {usd(h.shadow_limit_usd)}{h.paused ? ' · shadow paused until midnight' : ''}
            </div>
          )}
        </Panel>

        <Panel className="h-[560px] shrink-0 xl:h-auto xl:min-h-[300px] xl:flex-1" bodyClassName="flex flex-col" title="Judgment log" icon={<ListFilter size={14} />} actions={
          <Segmented value={since} options={SINCE} onChange={(v) => set('since', v === 'all' ? null : v)} />
        }>
          <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-line px-3 py-1.5 text-[11px] text-ink-faint">
            <span>pack</span>
            <select value={pack} onChange={(e) => set('pack', e.target.value || null)} className="rounded bg-transparent ring-1 ring-line">
              <option value="">every pack</option>
              {packs.map((p) => <option key={p} value={p}>{p}</option>)}
            </select>
            <span>session</span>
            <input value={session} onChange={(e) => set('session', e.target.value.trim() || null)} placeholder="ses_…" className="num w-56 rounded bg-transparent px-1 ring-1 ring-line" />
            <span className="ml-auto num" title={list?.scopes.join(', ')}>
              {list ? `${shown.length} of ${list.matched}${list.more ? '+' : ''} in ${list.scopes.length} scope${list.scopes.length === 1 ? '' : 's'}` : ''}
            </span>
          </div>
          {error ? <Empty>{String((error as { message?: string }).message ?? error)}</Empty> : !shown.length ? <Empty>no judgments match</Empty> : (
            <div className="min-h-0 flex-1 overflow-auto">
              {[...shown].reverse().map((j) => (
                <LogRow key={j.id} j={j} open={j.id === id} onOpen={() => set('id', j.id === id ? null : j.id)} onSession={(s) => nav(`/session/${s}?tab=timeline`)} />
              ))}
            </div>
          )}
        </Panel>
      </div>

      <div className="flex min-h-0 flex-col gap-3 xl:overflow-auto xl:pr-0.5">
        <Panel className="shrink-0" title={id ? `Judgment ${short(id)}` : 'One judgment'} icon={<Gavel size={14} />} actions={
          id && <button className="text-[11px] text-ink-faint hover:text-live" onClick={() => set('id', null)}>close</button>
        }>
          {id ? <Detail id={id} /> : <Empty>pick a judgment in the log for its state and answers</Empty>}
        </Panel>

        {h?.enabled && !world && <Notices state={h.notices} onSession={(s) => nav(`/session/${s}?tab=timeline`)} />}
        <Panel className="shrink-0" title="Ladder" icon={<TrendingUp size={14} />}>
          <PackLadder readOnly={!!world} />
        </Panel>

        <Panel className="shrink-0" title="Versions" icon={<GitCompare size={14} />} bodyClassName="max-h-[360px] overflow-auto">
          <PackVersions readOnly={!!world} />
        </Panel>

        <Panel className="shrink-0" title="Learning" icon={<GraduationCap size={14} />} actions={
          <button className="text-[11px] text-ink-faint hover:text-live" onClick={() => set('report', params.get('report') ? null : localDay(new Date()))}>
            {params.get('report') ? 'hide' : 'show the report'}
          </button>
        }>
          {params.get('report') && <LearningReport date={params.get('report')!} pack={pack} onDate={(d) => set('report', d)} />}
        </Panel>
      </div>
    </div>
  )
}

/** Jev's live notices: the newest, each with what it said and its label buttons. */
function Notices({ state, onSession }: { state: string; onSession: (s: string) => void }) {
  const { data } = useRpc<{ rows: LedgerEntry[] }>('ledger.tail', { n: 200, kind: 'tool.notified', session_id: null }, 6000)
  const rows = useMemo(() => (data?.rows ?? []).filter((r) => (r.data as D)?.by === 'judge').reverse().slice(0, 20), [data])
  return (
    <Panel className="shrink-0" title="Notices" icon={<BellRing size={14} />} actions={
      <Pill tone={state === 'on' ? 'live' : state.startsWith('paused') ? 'wait' : 'idle'} title="security.v3's notices after an open call it is sure was risky">{state || 'not reported'}</Pill>
    }>
      {!rows.length ? <Empty>no notice yet</Empty> : rows.map((r) => {
        const d = r.data as D
        return (
          <div key={`${d.judgment}`} className="border-b border-line/60 px-3 py-1.5 text-[11.5px]">
            <div className="num flex items-baseline gap-3">
              <span className="shrink-0 text-ink-faint">{stamp(r.at_unix_ms)}</span>
              <span className="shrink-0 text-tool">{String(d.tool ?? '')}</span>
              <span className="min-w-0 flex-1 truncate text-wait">{noticeWords({ percent: Number(d.percent ?? 0), reasons: (d.reasons ?? []).map(String) })}</span>
              {r.session_id && <button className="shrink-0 text-ink-faint hover:text-live" onClick={() => onSession(r.session_id!)}>{short(r.session_id)}</button>}
            </div>
            <div className="truncate text-ink-dim">{String(d.summary ?? '')}</div>
            <JudgmentLabels id={String(d.judgment)} answers={[]} />
          </div>
        )
      })}
    </Panel>
  )
}

function LogRow({ j, open, onOpen, onSession }: { j: Judged; open: boolean; onOpen: () => void; onSession: (s: string) => void }) {
  const tone = j.outcome === 'failed' ? 'text-fault' : j.outcome === 'skipped' ? 'text-ink-faint' : j.disagrees ? 'text-wait' : 'text-ink'
  return (
    <div className={cn('num flex cursor-pointer items-baseline gap-3 border-b border-line/60 px-3 py-1 text-[11.5px] hover:bg-gold/5', open && 'bg-live/10')} onClick={onOpen}>
      <span className="shrink-0 text-ink-faint">{stamp(j.at)}</span>
      <span className="shrink-0">{j.pack}</span>
      <span className={cn('min-w-0 flex-1 truncate', tone)}>{line(j)}</span>
      <span className="shrink-0 text-ink-faint">{j.cls}</span>
      <span className="shrink-0">{ms(j.totalMs)}</span>
      <span className="shrink-0">{money(j.costMicros)}</span>
      {j.session && <button className="shrink-0 text-ink-faint hover:text-live" title={`open session ${j.session}`} onClick={(e) => { e.stopPropagation(); onSession(j.session!) }}>{short(j.session)}</button>}
    </div>
  )
}

/** One judgment: its row's fields, the state Jev was sent, and each answer as bars. */
function Detail({ id }: { id: string }) {
  const { data, error } = useRpc<JudgeGetResult>('judge.get', { id }, 0)
  if (error) return <Empty>{String((error as { message?: string }).message ?? error)}</Empty>
  if (!data) return <Empty>reading…</Empty>
  const d = (data.judgment.data ?? {}) as D
  const j = judged(data.judgment)
  const state = data.state as D | null | undefined
  return (
    <div className="flex h-full min-h-0 flex-col gap-3 overflow-auto p-3">
      <div>
        <Field label="pack">{j.pack} v{j.version} · {j.mode} · {String(d.point ?? '')}</Field>
        <Field label="outcome">{line(j)}</Field>
        <Field label="session · turn" mono>{short(j.session)} · {short(j.turn)}</Field>
        <Field label="class · baseline">{j.cls} · {String(d.context?.decision ?? '—')}</Field>
        <Field label="model" mono>{String(d.model ?? '')}{d.answered_by && d.answered_by !== d.model ? ` (answered by ${d.answered_by})` : ''}</Field>
        <Field label="time" mono>{ms(d.timing?.total_ms)} (queued {ms(d.timing?.queued_ms)}, http {ms(d.timing?.http_ms)}) · on path {ms(d.context?.on_path_ms ?? 0)}</Field>
        <Field label="cost" mono>{money(j.costMicros)} of {money(d.reserve_micros)} reserved · {String(d.budget ?? '')} budget</Field>
        <Field label="state" mono>{d.state?.bytes} bytes · ~{d.state?.tokens} of {d.state?.cap_tokens} tokens · {short(d.state?.sha256)}</Field>
      </div>
      {!!(d.answers ?? []).length && j.outcome === 'answered' && <JudgmentLabels id={id} answers={d.answers as D[]} />}
      {!!(d.answers ?? []).length && (
        <div>
          <div className="panel-title mb-1">answers</div>
          {(d.answers as D[]).map((a) => (
            <div key={`${a.question}:${a.about ?? ''}`} className="mb-2">
              <div className="flex items-baseline gap-2 text-[12px]">
                <span className="text-ink">{a.question}{a.about ? ` · ${a.about}` : ''}</span>
                <Pill tone={a.band?.band === 'act' ? 'ok' : a.band?.band === 'confirm' ? 'wait' : 'fault'}>{a.band?.band} {Number(a.band?.value ?? 0).toFixed(2)}</Pill>
              </div>
              {bars(a).map((b) => (
                <div key={b.label} className="num flex items-center gap-2 text-[11px]">
                  <span className={cn('w-40 shrink-0 truncate', b.chosen ? 'text-live' : 'text-ink-faint')}>{b.label}</span>
                  <div className="h-2 flex-1 rounded bg-line/40"><div className={cn('h-2 rounded', b.chosen ? 'bg-live' : 'bg-ink-faint/50')} style={{ width: `${Math.round(b.p * 100)}%` }} /></div>
                  <span className="w-10 shrink-0 text-right">{b.p.toFixed(2)}</span>
                </div>
              ))}
            </div>
          ))}
        </div>
      )}
      <div>
        <div className="panel-title mb-1">state</div>
        {state && typeof state === 'object' ? (
          Object.entries(state).map(([k, v]) => (
            <div key={k} className="mb-1 text-[11.5px]">
              <div className="text-ink-faint">{k}</div>
              {v !== null && typeof v === 'object' ? <JsonView value={v} maxHeight="160px" /> : <div className="num whitespace-pre-wrap text-ink">{String(v)}</div>}
            </div>
          ))
        ) : <Empty>{data.state_missing ?? 'no state'}</Empty>}
      </div>
    </div>
  )
}
