// The Policy view (M7 42b): why a call waits, on one screen. `policy.explain` for the CLI and every bound place (or one
// session's, `?session=`): each place with its class and ceiling, each tool's result and the layers that raised it, a
// tool opening its layers in the gate's order and the conditions that depend on the call. Beside them, the tightenings
// with their undo (the Boundaries board's list, shared), and a link to the approval channels in Systems.
//
// The reads are the push's: `policy.explain` is read again on a tightening, its undo, a trust, and a change of any
// execution (a hold), never on a timer. State is in the address: `?session=`, `?place=` (the place opened), `?tool=`
// (the tool opened), `?q=` and `?result=`. Under the time machine it shows the present, and its undo is off.
import { useMemo } from 'react'
import { Link, useNavigate, useSearchParams } from 'react-router'
import { ChevronDown, ChevronRight, Link2, Lock, Search, ShieldCheck } from 'lucide-react'
import type { ExplainCondition, ExplainLayer, Health, PlaceExplain, PolicyExplainResult, SessionInfo, ToolExplain } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { useTick } from '@/lib/hooks'
import { useHistoryRows } from '@/lib/history'
import { useAsOf } from '@/lib/timemachine'
import { ceilingWords } from '@/lib/ceiling'
import { RESULTS, filterTools, isResult, placeSummary, sortTools, toolLine, type Result } from '@/lib/policyview'
import { ago, cn, short } from '@/lib/format'
import { Empty, Panel, Pill } from '@/components/ui'
import { Tightenings } from '@/components/Tightenings'
import type { Tone } from '@/lib/taxonomy'

const TONE: Record<string, Tone> = { open: 'ok', notify: 'live', approve: 'wait', refused: 'fault' }

export default function Policy() {
  const nav = useNavigate()
  const now = useTick(1000)
  const asOf = useAsOf((s) => s.t)
  const [params, setParams] = useSearchParams()
  const put = (k: string, v: string | null) => setParams((p) => { if (v) p.set(k, v); else p.delete(k); return p }, { replace: true })
  const session = params.get('session')
  const q = params.get('q') ?? ''
  const result = isResult(params.get('result')) ? (params.get('result') as Result) : null
  const openTool = params.get('tool')
  // The places opened (?place=a,b): the first is open when none is named.
  const openPlaces = useMemo(() => new Set((params.get('place') ?? '').split(',').filter(Boolean)), [params])
  const { data, error, isFetching } = useRpc<PolicyExplainResult>('policy.explain', session ? { session_id: session } : {}, 0)
  const { data: h } = useRpc<Health>('health', undefined, 3000)
  const { data: sl } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 0)
  const { rows } = useHistoryRows()
  const titleOf = useMemo(() => new Map((sl?.sessions ?? []).map((s) => [s.session_id, s.title || s.label || short(s.session_id)])), [sl])
  const title = (sid?: string | null) => (sid ? titleOf.get(sid) ?? short(sid) : '—')
  const log = useMemo(() => rows.filter((r) => r.kind === 'policy.tightened' || r.kind === 'policy.untightened').slice(-6).reverse(), [rows])
  const places = data?.places ?? []
  const togglePlace = (key: string) => {
    const cur = openPlaces.size ? new Set(openPlaces) : new Set(places.slice(0, 1).map((p) => p.place))
    if (cur.has(key)) cur.delete(key); else cur.add(key)
    put('place', cur.size ? [...cur].join(',') : '-')
  }
  const isOpen = (key: string) => (openPlaces.size ? openPlaces.has(key) : places[0]?.place === key)

  return (
    <div className="flex flex-col gap-3">
      <div className="panel flex flex-wrap items-center gap-x-6 gap-y-3 px-4 py-3">
        <div className="mr-2">
          <h1 className="ship-title !text-[26px]">The policy</h1>
          <div className="text-[11.5px] text-ink-dim">why a call waits: every layer of the gate, in its order, for each tool in each place</div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <label className="text-[11px] text-ink-faint" htmlFor="policy-session">for</label>
          <select id="policy-session" value={session ?? ''} onChange={(e) => put('session', e.target.value || null)}
            className="num max-w-72 rounded-md bg-white/5 px-2 py-1 text-[12px] text-ink outline-none ring-1 ring-line focus:ring-live/40">
            <option value="">no session: the CLI and every bound place</option>
            {(sl?.sessions ?? []).map((s) => <option key={s.session_id} value={s.session_id}>{s.title || s.label || short(s.session_id)} · {short(s.session_id)}</option>)}
            {session && !titleOf.has(session) && <option value={session}>{session}</option>}
          </select>
          <div className="flex items-center gap-1.5 rounded-md bg-white/5 px-2.5 py-1 ring-1 ring-line focus-within:ring-live/40">
            <Search size={13} className="text-ink-faint" />
            <input value={q} onChange={(e) => put('q', e.target.value || null)} placeholder="a tool, or a setting…" aria-label="search the tools" className="w-44 bg-transparent text-[12.5px] text-ink outline-none placeholder:text-ink-faint" />
          </div>
          <div className="flex items-center gap-1">
            {RESULTS.map((r) => (
              <button key={r} type="button" onClick={() => put('result', result === r ? null : r)} aria-pressed={result === r}
                className={cn('num rounded-md px-1.5 py-0.5 text-[11px] ring-1 ring-inset', result === r ? 'bg-white/[0.06] text-ink ring-live/40' : 'text-ink-dim ring-line hover:text-ink')}>{r}</button>
            ))}
          </div>
        </div>
        <Link to="/systems" className="num ml-auto flex items-center gap-1.5 text-[12px] text-ink-dim hover:text-live" title="who may approve, through which channel, and the recent approval rows">
          <ShieldCheck size={13} /> the approval channels · Systems
        </Link>
        {asOf !== null && <Pill tone="wait" title="the gate as it is now: the time machine's moment does not move it">shows the present · its acts are off</Pill>}
      </div>

      <div className="grid grid-cols-1 gap-3 2xl:grid-cols-[1fr_420px]">
        <div className="flex min-w-0 flex-col gap-3">
          {error && <Panel bodyClassName="p-3"><div className="text-[12.5px] text-fault">policy.explain failed: {String((error as Error).message ?? error)}</div></Panel>}
          {!data && !error && <Empty>reading the gate…</Empty>}
          {data && !places.length && <Empty>the daemon explains no place</Empty>}
          {places.map((p) => (
            <PlaceCard key={p.place} p={p} open={isOpen(p.place)} onToggle={() => togglePlace(p.place)} q={q} result={result}
              openTool={openTool} onTool={(t) => put('tool', openTool === t ? null : t)} onSession={(sid) => nav(`/session/${sid}`)} now={now} title={title} />
          ))}
          {data && <div className="num px-1 text-[11px] text-ink-faint">each tool’s result is for a call inside {data.roots.length ? data.roots.join(', ') : 'the workspace roots'} that no condition matches{isFetching ? ' · reading…' : ''}</div>}
        </div>
        <div className="flex min-w-0 flex-col gap-3">
          <Panel title={<>Tightenings · “Make actions like this ask in the future” · {(h?.tightenings ?? []).length}</>} icon={<Lock size={13} />} bodyClassName="p-2.5">
            <Tightenings tight={h?.tightenings ?? []} title={title} now={now} log={log} disabled={asOf !== null}
              onOpen={(sid, cid) => nav(`/session/${sid}${cid ? `?call=${cid}` : ''}`)} />
          </Panel>
        </div>
      </div>
    </div>
  )
}

function PlaceCard({ p, open, onToggle, q, result, openTool, onTool, onSession, now, title }: {
  p: PlaceExplain; open: boolean; onToggle: () => void; q: string; result: Result | null; openTool: string | null; onTool: (t: string) => void
  onSession: (sid: string) => void; now: number; title: (s?: string | null) => string
}) {
  const sum = useMemo(() => placeSummary(p), [p])
  const tools = useMemo(() => sortTools(filterTools(p.tools, q, result)), [p.tools, q, result])
  return (
    <section className="panel" data-place={p.place}>
      <button type="button" onClick={onToggle} aria-expanded={open} className="flex w-full flex-wrap items-center gap-x-3 gap-y-1 px-3.5 py-2.5 text-left">
        {open ? <ChevronDown size={14} className="text-ink-faint" /> : <ChevronRight size={14} className="text-ink-faint" />}
        <span className="text-[14px] font-semibold text-ink">{p.name}</span>
        <Pill tone={p.class === 'shared' ? 'wait' : 'idle'} title={p.class === 'shared' ? 'a shared place: others read it, so it has the public tools alone' : 'a private place'}>{p.class}</Pill>
        {p.ceiling && ceilingWords(p.ceiling) && <span className="num text-[11.5px] text-wait" title="what the bindings file narrows here">ceiling: {ceilingWords(p.ceiling)}</span>}
        <span className="num text-[11px] text-ink-faint">{p.place}</span>
        <span className="num ml-auto flex items-center gap-2 text-[11px]">
          {RESULTS.map((r) => sum.counts[r] > 0 && <span key={r} className={cn(r === 'open' ? 'text-ok' : r === 'notify' ? 'text-live' : r === 'approve' ? 'text-wait' : 'text-fault')}>{sum.counts[r]} {r}</span>)}
          {sum.tightened.length > 0 && <span className="text-wait" title={sum.tightened.join(', ')}>{sum.tightened.length} tightened</span>}
        </span>
      </button>
      {p.session_id && (
        <div className="num flex flex-wrap items-center gap-x-3 px-3.5 pb-2 text-[11.5px] text-ink-dim">
          <button type="button" onClick={() => onSession(p.session_id!)} className="hover:text-live">session {title(p.session_id)}</button>
          {p.hold
            ? <span className="flex items-center gap-1 text-magenta"><Link2 size={12} /> holds outside text: read {p.hold.tool} {p.hold.query ? `“${p.hold.query}”` : p.hold.url} {ago(p.hold.since_ms, now)}, so its calls that act wait · <Link to="/boundaries" className="underline decoration-dotted hover:text-live">trust it on the boundaries board</Link></span>
            : <span className="text-ok">trusted: no outside text held</span>}
        </div>
      )}
      {open && (
        <div className="border-t border-line">
          {!tools.length && <div className="px-3.5 py-3 text-[12px] text-ink-faint">no tool matches</div>}
          {tools.map((t) => <ToolRow key={t.tool} t={t} open={openTool === t.tool} onOpen={() => onTool(t.tool)} />)}
        </div>
      )}
    </section>
  )
}

function ToolRow({ t, open, onOpen }: { t: ToolExplain; open: boolean; onOpen: () => void }) {
  const line = toolLine(t)
  return (
    <div className="border-b border-line/40 last:border-b-0" data-tool={t.tool}>
      <button type="button" onClick={onOpen} aria-expanded={open} className="flex w-full flex-wrap items-baseline gap-x-3 px-3.5 py-1.5 text-left text-[12px] hover:bg-white/[0.03]">
        {open ? <ChevronDown size={12} className="self-center text-ink-faint" /> : <ChevronRight size={12} className="self-center text-ink-faint" />}
        <span className="num w-56 shrink-0 truncate text-tool" title={t.tool}>{t.tool}</span>
        <Pill tone={TONE[t.result] ?? 'idle'}>{t.result}</Pill>
        <span className="num text-[10.5px] text-ink-faint">{t.class}</span>
        {!t.offered && <span className="text-[11px] text-fault">not offered{t.refused ? `: ${t.refused}` : ''}</span>}
        {line.raisedBy.length > 0 && <span className="num text-[11px] text-ink-dim">raised by {line.raisedBy.join(', ')}</span>}
        {line.conditions > 0 && <span className="num text-[11px] text-ink-faint">{line.conditions} condition{line.conditions === 1 ? '' : 's'}</span>}
      </button>
      {open && (
        <div className="flex flex-col gap-2 px-3.5 pb-3 pl-9">
          <div className="text-[11.5px] text-ink-dim">{t.reason}</div>
          <table className="w-full text-[11.5px]">
            <thead className="text-[10px] uppercase tracking-wider text-ink-faint"><tr><th className="py-0.5 pr-3 text-left">layer</th><th className="pr-3 text-left">says</th><th className="pr-3 text-left">setting</th><th className="text-left">posture after</th></tr></thead>
            <tbody>{t.layers.map((l, i) => <LayerRow key={i} l={l} />)}</tbody>
          </table>
          {!!t.conditions?.length && (
            <div>
              <div className="ship-engraved mb-0.5 text-[9.5px]">depends on the call</div>
              {t.conditions.map((c, i) => <ConditionRow key={i} c={c} />)}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function LayerRow({ l }: { l: ExplainLayer }) {
  return (
    <tr className={cn('border-t border-line/30', l.raised && 'bg-wait/[0.05]')}>
      <td className="num py-0.5 pr-3 text-ink">{l.layer}{l.raised && <span className="ml-1 text-wait" title="this layer raised the posture">▲</span>}</td>
      <td className="pr-3 text-ink-dim">{l.says}</td>
      <td className="num pr-3 text-ink-faint">{l.setting ?? ''}</td>
      <td><Pill tone={TONE[l.result] ?? 'idle'}>{l.result}</Pill></td>
    </tr>
  )
}

function ConditionRow({ c }: { c: ExplainCondition }) {
  return (
    <div className="flex flex-wrap items-baseline gap-x-2 border-t border-line/30 py-0.5 text-[11.5px]">
      <span className="num w-28 shrink-0 text-ink">{c.layer}</span>
      <span className="text-ink-dim">when {c.when}</span>
      <span className="num text-ink-faint">→ {c.then}</span>
      {!!c.entries?.length && (
        <span className="num flex w-full flex-wrap gap-1 pl-[7.5rem] text-[10.5px] text-ink-faint">
          {c.entries!.map((e) => <span key={e} className="rounded bg-white/5 px-1 ring-1 ring-line">{e}</span>)}
        </span>
      )}
    </div>
  )
}
