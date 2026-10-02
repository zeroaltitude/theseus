// The session deck: one session, down to its spans. The transcript streams live; the inspector shows each turn's
// flame chart, the context lineage, the spend, and the session's own ledger rows.
import { useEffect, useMemo, useRef, useState } from 'react'
import { useParams, useNavigate, useSearchParams } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { Group, Panel as RPanel, Separator } from 'react-resizable-panels'
import { Tabs } from 'radix-ui'
import { ArrowDown, ArrowLeft, Brain, Coins, Copy, GitBranch, Layers, OctagonX, Pause, Play, ScrollText, ShieldCheck, Timer } from 'lucide-react'
import type { CompilationInfo, ExecutionInfo, LedgerEntry, SessionHistory, Span } from '@protocol'
import { call, useRpc, usePush, useSessionWatch } from '@/lib/rpc'
import { useLedger, providerCalls, turnRows, type ProviderCall, type TurnRow } from '@/lib/derive'
import { summarize } from '@/lib/summary'
import { ago, cn, ms, short, stamp, tokens, usd, clock } from '@/lib/format'
import { ledgerKind, toneHex } from '@/lib/taxonomy'
import { axisStyle, type EChartsOption } from '@/lib/chart'
import { useTick } from '@/lib/hooks'
import { Echart } from '@/components/Echart'
import { Flame, flatten } from '@/components/Flame'
import { JsonView } from '@/components/JsonView'
import { Transcript } from '@/components/Transcript'
import { CallInspector } from '@/components/CallInspector'
import { ModelInspector } from '@/components/ModelInspector'
import { SessionGraph } from '@/components/SessionGraph'
import { Composer } from '@/components/Composer'
import { ContextGrowth, TokenMix } from '@/components/instruments'
import { AttentionPill, Btn, Empty, Field, LiveDot, Meter, Panel, Pill } from '@/components/ui'

type Rows = LedgerEntry[] | undefined

export default function SessionDeck() {
  const { id = '' } = useParams()
  const nav = useNavigate()
  const qc = useQueryClient()
  useSessionWatch(id)
  const { data: hist, isLoading } = useRpc<SessionHistory>('session.history', { session_id: id }, 3000)
  const { data: el } = useRpc<{ executions: ExecutionInfo[] }>('execution.list', undefined, 3000)
  const { data: comps } = useRpc<{ compilations: CompilationInfo[] }>('compilation.list', { session_id: id, n: 50 }, 10_000)
  const { data: ledger } = useLedger(3000, 4000, undefined, id)
  const rows = ledger?.rows

  const turns = useMemo(() => turnRows(rows), [rows])
  const turnMap = useMemo(() => new Map(turns.map((t) => [t.turn_id, t])), [turns])
  const traces = useMemo(() => {
    const m = new Map<string, Span>()
    for (const r of rows ?? []) if (r.kind === 'turn.trace' && r.turn_id) m.set(r.turn_id, r.data as Span)
    return m
  }, [rows])
  const calls = useMemo(() => providerCalls(rows), [rows])
  const live = useLive(id)

  // A written node or an ended turn means the transcript changed: read it again now, not at the next poll.
  const events = usePush((s) => s.events)
  const lastEvent = events[events.length - 1]
  useEffect(() => {
    if (!lastEvent) return
    if (['node.written', 'turn.ended', 'tool.ended', 'confirm.requested', 'confirm.resolved'].includes(lastEvent.method)) {
      qc.invalidateQueries({ queryKey: ['session.history'] })
    }
  }, [lastEvent, qc])

  const s = hist?.session
  const exec = el?.executions.find((e) => e.execution_id === s?.execution_id) ?? el?.executions.find((e) => e.session_id === id)

  if (!s || !hist) return <Panel title="session" bodyClassName="h-64"><Empty>{isLoading ? 'reading the session…' : `no session ${short(id)}`}</Empty></Panel>

  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      <Header s={s} exec={exec} onBack={() => nav('/fleet')} />
      <Group orientation="horizontal" className="min-h-0 flex-1">
        <RPanel defaultSize="58" minSize={420} className="min-h-0">
          <Panel title={<>transcript · {hist.nodes.length} nodes</>} icon={<ScrollText size={13} />} className="h-full"
            bodyClassName="min-h-0"
            actions={live ? <span className="flex items-center gap-1.5 text-[11px] text-live"><LiveDot size={5} /> {live.text ? 'streaming' : 'turn running'}</span> : null}>
            <div className="flex h-full min-h-0 flex-col">
              <div className="relative min-h-0 flex-1">
                <Follow deps={[hist.nodes.length, live?.text]}>
                  <Transcript nodes={hist.nodes} turns={turnMap} live={live} />
                </Follow>
              </div>
              <Composer sessionId={id} busy={!!live || exec?.state === 'running' || exec?.state === 'queued'} />
            </div>
          </Panel>
        </RPanel>
        <Separator className="mx-1.5 w-1 rounded-full bg-transparent transition-colors hover:bg-live/30" />
        <RPanel defaultSize="42" minSize={360} className="min-h-0">
          <Inspector turns={turns} traces={traces} comps={comps?.compilations ?? []} rows={rows} calls={calls} session={s} nodes={hist.nodes} />
        </RPanel>
      </Group>
      <CallInspector sessionId={id} />
      <ModelInspector sessionId={id} />
    </div>
  )
}

/** A scroller that opens at the newest content and follows it while you are near the bottom; scroll up to read,
 *  and a button brings you back. */
function Follow({ children, deps }: { children: React.ReactNode; deps: unknown[] }) {
  const el = useRef<HTMLDivElement>(null)
  const [away, setAway] = useState(false)
  const near = useRef(true)
  const onScroll = () => {
    const e = el.current
    if (!e) return
    near.current = e.scrollHeight - e.scrollTop - e.clientHeight < 260
    setAway(!near.current)
  }
  useEffect(() => {
    const e = el.current
    if (e && near.current) e.scrollTop = e.scrollHeight
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps)
  return (
    <>
      <div ref={el} onScroll={onScroll} className="h-full overflow-auto">{children}</div>
      {away && (
        <button onClick={() => { const e = el.current; if (e) { e.scrollTo({ top: e.scrollHeight, behavior: 'smooth' }); near.current = true; setAway(false) } }}
          className="absolute bottom-3 right-4 flex items-center gap-1 rounded-full bg-live/15 px-3 py-1.5 text-[11.5px] font-medium text-live shadow-lg ring-1 ring-live/40 backdrop-blur">
          <ArrowDown size={13} /> latest
        </button>
      )}
    </>
  )
}

/** The streaming text of this session's current turn, from model.delta pushes. */
function useLive(sessionId: string) {
  const events = usePush((s) => s.events)
  return useMemo(() => {
    let turn: string | null = null
    let text = ''
    for (const e of events) {
      const p = e.params as Record<string, any>
      if (e.method === 'turn.started' && p.session_id === sessionId) { turn = p.turn_id; text = '' }
      else if (turn && p.turn_id === turn) {
        if (e.method === 'model.delta') text += p.text ?? ''
        else if (e.method === 'loop.started') text = ''
        else if (e.method === 'turn.ended' || e.method === 'turn.failed') { turn = null; text = '' }
      }
    }
    return turn ? { turn_id: turn, text } : null
  }, [events, sessionId])
}

function Header({ s, exec, onBack }: { s: SessionHistory['session']; exec?: ExecutionInfo; onBack: () => void }) {
  const now = useTick(5000)
  const [busy, setBusy] = useState<string | null>(null)
  const act = async (label: string, method: string, params: unknown) => {
    if (!window.confirm(`${label} ${short(s.session_id)}?`)) return
    setBusy(label)
    try { await call(method, params) } catch (e: any) { window.alert(e?.message ?? String(e)) } finally { setBusy(null) }
  }
  const b = exec?.budget
  const u = s.usage
  return (
    <div className="panel relative z-20 flex flex-wrap items-center gap-x-6 gap-y-2 px-4 py-3">
      <button onClick={onBack} className="rounded-md p-1 text-ink-faint hover:bg-white/5 hover:text-ink"><ArrowLeft size={16} /></button>
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <h1 className="truncate text-[17px] font-semibold text-ink">{s.title || s.label || 'untitled session'}</h1>
          <AttentionPill a={s.attention} state={s.execution_state} />
          <Pill tone={s.kind === 'task' ? 'tool' : 'idle'}>{s.kind}</Pill>
          {s.external_text && <Pill tone="wait"><ShieldCheck size={11} /> holds external text</Pill>}
        </div>
        <div className="num mt-0.5 flex flex-wrap gap-x-3 text-[11px] text-ink-faint">
          <button onClick={() => navigator.clipboard?.writeText(s.session_id)} className="inline-flex items-center gap-1 hover:text-ink"><Copy size={10} />{s.session_id}</button>
          {s.label && <span>{s.label}</span>}
          <span>{s.profile ?? 'default'} · {s.model ?? '—'}</span>
          <span>opened {stamp(s.created_at_unix_ms)}</span>
          <span>active {ago(s.last_active_ms, now)}</span>
          {s.parent_session_id && <span>task of {short(s.parent_session_id)}</span>}
        </div>
      </div>
      <div className="flex gap-6">
        <Stat label="turns" value={String(s.turns)} />
        <Stat label="tool calls" value={String(s.tool_calls ?? 0)} />
        <Stat label="tokens in" value={tokens(u.input_tokens + u.cache_read_input_tokens + u.cache_creation_input_tokens)} />
        <Stat label="cache read" value={tokens(u.cache_read_input_tokens)} tone="think" />
        <Stat label="out" value={tokens(u.output_tokens)} />
        <Stat label="cost" value={usd(s.cost_usd)} tone="money" />
      </div>
      {b && (
        <div className="w-56">
          <div className="mb-1 flex justify-between text-[11px]"><span className="text-ink-faint">budget</span><span className="num text-ink">{usd(b.spent_usd)} / {usd(b.limit_usd)}</span></div>
          <Meter value={b.spent_usd + b.reserved_usd} max={b.limit_usd} tone={b.available_usd > b.limit_usd * 0.2 ? 'ok' : b.available_usd > 0 ? 'wait' : 'fault'} />
          <div className="num mt-1 text-[10px] text-ink-faint">reserved {usd(b.reserved_usd)} · held {usd(b.held_unknown_usd)} · {usd(b.available_usd)} left</div>
        </div>
      )}
      <div className="ml-auto flex gap-2">
        <Recompile busy={!!busy?.startsWith('Recompile')} onPick={(strategy) => act(`Recompile (${strategy})`, 'session.recompile', { session_id: s.session_id, strategy }).then(() => undefined)} />
        {s.external_text && <Btn tone="wait" onClick={() => act('Trust', 'policy.trust', { session_id: s.session_id })} busy={busy === 'Trust'}><ShieldCheck size={13} /> Trust</Btn>}
        {exec && ['running', 'queued'].includes(exec.state) &&
          <Btn tone="wait" title="Halt the running turn; the session stays" onClick={() => act('Stop', 'execution.stop', { execution_id: exec.execution_id })} busy={busy === 'Stop'}><Pause size={13} /> Stop</Btn>}
        {exec && ['running', 'queued', 'waiting'].includes(exec.state) &&
          <Btn tone="fault" title="End this execution" onClick={() => act('Cancel', 'execution.cancel', { execution_id: exec.execution_id })} busy={busy === 'Cancel'}><OctagonX size={13} /> Cancel</Btn>}
      </div>
    </div>
  )
}

/** session.recompile: the next turn's context is compiled again, from the whole transcript (thinking stripped) or
 *  fresh (starting over). A small menu, since the two do very different things. */
function Recompile({ onPick, busy }: { onPick: (strategy: 'transcript' | 'fresh') => void; busy: boolean }) {
  const [open, setOpen] = useState(false)
  return (
    <div className="relative">
      <Btn tone="live" title="Compile this session's context again for its next turn" onClick={() => setOpen((v) => !v)} busy={busy}><Layers size={13} /> Recompile</Btn>
      {open && (
        <div className="absolute right-0 top-full z-30 mt-1 w-72 rounded-md border border-line-strong bg-deck p-1 shadow-xl">
          {([['transcript', 'Keep everything, with the thinking stripped'], ['fresh', 'Start over: the next turn sees only what is new']] as const).map(([k, what]) => (
            <button key={k} onClick={() => { setOpen(false); onPick(k) }} className="flex w-full flex-col items-start rounded px-2 py-1.5 text-left hover:bg-white/5">
              <span className="num text-[12px] text-live">{k}</span>
              <span className="text-[11px] text-ink-faint">{what}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

function Stat({ label, value, tone }: { label: string; value: string; tone?: keyof typeof toneHex }) {
  return (
    <div>
      <div className="text-[10px] font-semibold uppercase tracking-wider text-ink-faint">{label}</div>
      <div className="num text-[15px] font-semibold" style={{ color: tone ? toneHex[tone] : undefined }}>{value}</div>
    </div>
  )
}

const TABS = [
  { v: 'timeline', label: 'Timeline', icon: Timer },
  { v: 'context', label: 'Context', icon: Brain },
  { v: 'spend', label: 'Spend', icon: Coins },
  { v: 'graph', label: 'Graph', icon: GitBranch },
  { v: 'ledger', label: 'Ledger', icon: ScrollText },
] as const

function Inspector({ turns, traces, comps, rows, calls, session, nodes }: {
  turns: TurnRow[]; traces: Map<string, Span>; comps: CompilationInfo[]; rows: Rows; calls: ProviderCall[]; session: SessionHistory['session']; nodes: SessionHistory['nodes']
}) {
  const [params, setParams] = useSearchParams()
  const tab = params.get('tab') ?? 'timeline'
  return (
    <Tabs.Root value={tab} onValueChange={(v) => setParams((p) => { p.set('tab', v); return p }, { replace: true })} className="panel flex h-full min-h-0 flex-col">
      <Tabs.List className="flex gap-1 border-b border-line px-2 pt-1.5">
        {TABS.map((t) => (
          <Tabs.Trigger key={t.v} value={t.v}
            className="flex items-center gap-1.5 rounded-t-md px-3 py-1.5 text-[12px] font-medium text-ink-faint transition-colors hover:text-ink data-[state=active]:bg-live/10 data-[state=active]:text-live">
            <t.icon size={13} /> {t.label}
          </Tabs.Trigger>
        ))}
      </Tabs.List>
      <Tabs.Content value="timeline" className="min-h-0 flex-1"><TimelineTab turns={turns} traces={traces} /></Tabs.Content>
      <Tabs.Content value="context" className="min-h-0 flex-1 overflow-auto"><ContextTab comps={comps} rows={rows} session={session} /></Tabs.Content>
      <Tabs.Content value="spend" className="min-h-0 flex-1 overflow-auto"><SpendTab turns={turns} calls={calls} /></Tabs.Content>
      <Tabs.Content value="graph" className="min-h-0 flex-1"><SessionGraph nodes={nodes} /></Tabs.Content>
      <Tabs.Content value="ledger" className="min-h-0 flex-1"><LedgerTab rows={rows} /></Tabs.Content>
    </Tabs.Root>
  )
}

interface PickedSpan { name: string; kind: string; start: number; end: number; attrs: unknown }

/** Replay a turn on its own clock: a cursor in µs that you scrub, or play at 1×, 4×, or 16×. */
function useReplay(trace: Span | null | undefined) {
  const end = trace?.end_us ?? 0
  const [t, setT] = useState<number | null>(null)
  const [playing, setPlaying] = useState(false)
  const [speed, setSpeed] = useState<1 | 4 | 16>(4)
  useEffect(() => {
    if (!playing) return
    let raf = 0
    let last = performance.now()
    const step = (now: number) => {
      const dt = (now - last) * 1000 * speed
      last = now
      setT((cur) => {
        const next = (cur ?? 0) + dt
        if (next >= end) { setPlaying(false); return end }
        return next
      })
      raf = requestAnimationFrame(step)
    }
    raf = requestAnimationFrame(step)
    return () => cancelAnimationFrame(raf)
  }, [playing, speed, end])
  return {
    t, end, playing, speed, setSpeed,
    play: () => { if (t == null || t >= end) setT(0); setPlaying(true) },
    pause: () => setPlaying(false),
    seek: (v: number) => { setPlaying(false); setT(v) },
    stop: () => { setPlaying(false); setT(null) },
  }
}

function ReplayBar({ replay }: { replay: ReturnType<typeof useReplay> }) {
  if (!replay.end) return null
  return (
    <div className="flex items-center gap-2 border-b border-line px-3 py-1.5">
      <button onClick={replay.playing ? replay.pause : replay.play} title={replay.playing ? 'pause' : 'replay this turn'}
        className="grid h-6 w-6 place-items-center rounded-md bg-live/10 text-live ring-1 ring-live/30 hover:bg-live/20">
        {replay.playing ? <Pause size={12} /> : <Play size={12} />}
      </button>
      <input type="range" min={0} max={replay.end} step={Math.max(1, Math.round(replay.end / 1000))} value={replay.t ?? 0}
        onChange={(e) => replay.seek(Number(e.target.value))} className="h-1 flex-1 cursor-pointer accent-[#22d3ee]" />
      <span className="num w-28 text-right text-[11px] text-ink-dim">{replay.t != null ? `${ms(replay.t / 1000)} / ${ms(replay.end / 1000)}` : `replay · ${ms(replay.end / 1000)}`}</span>
      {([1, 4, 16] as const).map((s) => (
        <button key={s} onClick={() => replay.setSpeed(s)} className={cn('num rounded px-1.5 py-0.5 text-[10px]', replay.speed === s ? 'bg-live/15 text-live' : 'text-ink-faint hover:text-ink')}>{s}×</button>
      ))}
      {replay.t != null && <button onClick={replay.stop} className="text-[10.5px] text-ink-faint hover:text-ink">done</button>}
    </div>
  )
}

/** What was happening at instant t of a turn: the spans open then (outermost first), and the marks already past. */
function AtInstant({ trace, t }: { trace: Span | null | undefined; t: number }) {
  const flat = useMemo(() => (trace ? flatten(trace) : []), [trace])
  const open = flat.filter((f) => f.kind !== 'mark' && f.start <= t && t < f.end).sort((a, b) => a.depth - b.depth)
  const marks = flat.filter((f) => f.kind === 'mark' && f.start <= t)
  const done = flat.filter((f) => f.kind !== 'mark' && f.end <= t && f.depth >= 2).length
  return (
    <div className="flex flex-col gap-1">
      <div className="panel-title">at {ms(t / 1000)}</div>
      {open.length === 0 && <div className="text-[12px] text-ink-faint">between spans</div>}
      {open.map((f, i) => (
        <div key={`${f.name}-${f.start}`} className="num flex items-baseline gap-2 text-[12px]" style={{ paddingLeft: i * 12 }}>
          <span className="text-ink">{f.name}</span><span className="text-ink-faint">{f.kind}</span>
          <span className="ml-auto text-live">{ms((t - f.start) / 1000)} in</span>
          <span className="text-ink-faint">of {ms((f.end - f.start) / 1000)}</span>
        </div>
      ))}
      <div className="num mt-1 text-[11px] text-ink-faint">
        {done} spans finished{marks.length ? ` · ${marks.map((m) => `${m.name} at ${ms(m.start / 1000)}`).join(' · ')}` : ''}
      </div>
    </div>
  )
}

function TimelineTab({ turns, traces }: { turns: TurnRow[]; traces: Map<string, Span> }) {
  const withTrace = turns.filter((t) => traces.has(t.turn_id))
  const [pick, setPick] = useState<string | null>(null)
  const [span, setSpan] = useState<PickedSpan | null>(null)
  const turnId = pick ?? withTrace[withTrace.length - 1]?.turn_id ?? null
  const trace = turnId ? traces.get(turnId) : null
  const replay = useReplay(trace)
  if (!withTrace.length) return <Empty>no turn traces in this session’s rows</Empty>
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex gap-1 overflow-x-auto border-b border-line px-2 py-1.5">
        {withTrace.map((t, i) => (
          <button key={t.turn_id} onClick={() => { setPick(t.turn_id); setSpan(null); replay.stop() }}
            className={cn('num shrink-0 rounded-md px-2 py-1 text-left text-[10.5px] ring-1 ring-inset transition-colors',
              t.turn_id === turnId ? 'bg-live/10 text-live ring-live/30' : 'text-ink-faint ring-line hover:text-ink')}>
            <div>turn {i + 1} · {clock(t.start)}</div>
            <div className={t.failed ? 'text-fault' : ''}>{ms(t.elapsed_ms)} · {usd(t.cost)}</div>
          </button>
        ))}
      </div>
      <ReplayBar replay={replay} />
      <div className="min-h-0 flex-1 p-2"><Flame trace={trace} onPick={setSpan} cursor={replay.t} /></div>
      <div className="h-44 shrink-0 overflow-auto border-t border-line p-2">
        {replay.t != null ? <AtInstant trace={trace} t={replay.t} /> : span ? (
          <>
            <div className="mb-1 flex items-baseline gap-2 text-[12px]">
              <span className="font-semibold text-ink">{span.name}</span><span className="text-ink-faint">{span.kind}</span>
              <span className="num ml-auto text-live">{ms((span.end - span.start) / 1000)}</span>
            </div>
            <JsonView value={span.attrs ?? {}} maxHeight="120px" />
          </>
        ) : <Empty>click a span for its attributes</Empty>}
      </div>
    </div>
  )
}

function ContextTab({ comps, rows, session }: { comps: CompilationInfo[]; rows: Rows; session: SessionHistory['session'] }) {
  const [open, setOpen] = useState<string | null>(null)
  return (
    <div className="flex flex-col gap-3 p-3">
      <div className="h-48"><ContextGrowth rows={rows} sessions={[session]} /></div>
      <div className="panel-title flex items-center gap-1.5"><GitBranch size={12} /> compilation lineage</div>
      {comps.map((c) => (
        <div key={c.compilation_id} className={cn('rounded-lg ring-1 ring-inset', c.current ? 'bg-think/5 ring-think/30' : 'ring-line')}>
          <button onClick={() => setOpen(open === c.compilation_id ? null : c.compilation_id)} className="w-full px-3 py-2 text-left">
            <div className="flex items-center gap-2 text-[12px]">
              <span className="num text-think">{short(c.compilation_id)}</span>
              {c.current && <Pill tone="think">current</Pill>}
              <span className="text-ink-dim">{c.trigger}</span>
              <span className="num ml-auto text-[11px] text-ink-faint">{stamp(c.created_at_ms)}</span>
            </div>
            <div className="num mt-0.5 text-[11px] text-ink-faint">
              {c.strategy} · includes {c.includes} · as of {c.as_of}{c.derived_from ? ` · from ${short(c.derived_from)}` : ''} · {String((c.manifest as Record<string, unknown>).model ?? '')}
            </div>
          </button>
          {open === c.compilation_id && <div className="px-3 pb-3"><JsonView value={c.manifest} maxHeight="280px" /></div>}
        </div>
      ))}
      {!comps.length && <Empty>no compilations</Empty>}
    </div>
  )
}

function SpendTab({ turns, calls }: { turns: TurnRow[]; calls: ProviderCall[] }) {
  const option = useMemo<EChartsOption>(() => ({
    grid: { left: 50, right: 12, top: 16, bottom: 24 },
    tooltip: { trigger: 'axis', axisPointer: { type: 'shadow' }, valueFormatter: (v: any) => usd(Number(v)) },
    xAxis: { type: 'category', data: turns.map((_, i) => `t${i + 1}`), ...axisStyle, splitLine: { show: false } },
    yAxis: { type: 'value', ...axisStyle, axisLabel: { ...axisStyle.axisLabel, formatter: (v: number) => usd(v, 3) } },
    series: [{ type: 'bar', data: turns.map((t) => ({ value: t.cost ?? 0, itemStyle: { color: t.failed ? toneHex.fault : toneHex.money, borderRadius: [3, 3, 0, 0] } })) }],
  }), [turns])
  const total = calls.reduce((a, c) => a + c.cost, 0)
  return (
    <div className="flex flex-col gap-3 p-3">
      <div className="grid grid-cols-3 gap-4">
        <Field label="model calls" mono>{calls.length}</Field>
        <Field label="spend in rows" mono>{usd(total)}</Field>
        <Field label="avg per call" mono>{usd(calls.length ? total / calls.length : 0)}</Field>
      </div>
      <div className="panel-title">cost per turn</div>
      <div className="h-48">{turns.length ? <Echart option={option} /> : <Empty>no turns</Empty>}</div>
      <div className="panel-title">token mix per model call</div>
      <div className="h-56"><TokenMix calls={calls} /></div>
    </div>
  )
}

function LedgerTab({ rows }: { rows: Rows }) {
  const [q, setQ] = useState('')
  const [open, setOpen] = useState<number | null>(null)
  const list = useMemo(() => {
    const all = [...(rows ?? [])].reverse()
    const needle = q.toLowerCase()
    return (q ? all.filter((r) => r.kind.includes(needle) || summarize(r).toLowerCase().includes(needle)) : all).slice(0, 500)
  }, [rows, q])
  return (
    <div className="flex h-full min-h-0 flex-col">
      <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="filter by kind or text…"
        className="m-2 rounded-md bg-white/5 px-2.5 py-1.5 text-[12px] text-ink outline-none ring-1 ring-line placeholder:text-ink-faint focus:ring-live/40" />
      <div className="min-h-0 flex-1 overflow-auto">
        {list.map((r) => (
          <div key={r.position} className="border-b border-line/50">
            <button onClick={() => setOpen(open === r.position ? null : r.position)} className="flex w-full items-baseline gap-2 px-3 py-1 text-left text-[11.5px] hover:bg-white/[0.03]">
              <span className="num w-10 shrink-0 text-right text-ink-faint">{r.position}</span>
              <span className="num shrink-0 text-ink-faint">{clock(r.at_unix_ms)}</span>
              <span className="num w-40 shrink-0 truncate" style={{ color: toneHex[ledgerKind(r.kind).tone] }}>{r.kind}</span>
              <span className="min-w-0 truncate text-ink-dim">{summarize(r)}</span>
            </button>
            {open === r.position && <div className="px-3 pb-2"><JsonView value={r.data} maxHeight="260px" /></div>}
          </div>
        ))}
        {!list.length && <Empty>no rows</Empty>}
      </div>
    </div>
  )
}
