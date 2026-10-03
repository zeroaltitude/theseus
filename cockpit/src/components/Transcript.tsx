// A session's content graph as a readable transcript: turns as sections, model replies as markdown with their
// thinking, and each tool call as one card holding its input, the gate's decision, and its result.
import { useMemo, useState } from 'react'
import { useSearchParams } from 'react-router'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { Bot, Brain, ChevronRight, CircleCheck, KeyRound, OctagonX, ScanSearch, Shield, ShieldCheck, User, Wrench } from 'lucide-react'
import type { Health, NodeInfo, PublishResult, Tightening } from '@protocol'
import { useTick } from '@/lib/hooks'
import { call, useRpc } from '@/lib/rpc'
import { useWorld } from '@/lib/world'
import { cn, ms, stamp, tokens, usd } from '@/lib/format'
import type { TurnRow } from '@/lib/derive'
import { byteWords, callSummary, diffLines, l1Words, looksLikeDiff, resultWords } from '@/lib/toolwords'
import { JsonView } from './JsonView'
import { Pill } from './ui'
import { ShouldHaveAsked } from './ShouldHaveAsked'

type D = Record<string, any>

interface Item { kind: 'user' | 'assistant' | 'tool'; node: NodeInfo; result?: NodeInfo }

function items(nodes: NodeInfo[]): Item[] {
  const results = new Map<string, NodeInfo>()
  for (const n of nodes) if (n.kind === 'tool_result') { const id = (n.detail as D | null)?.tool_use_id; if (id) results.set(id, n) }
  const out: Item[] = []
  for (const n of nodes) {
    const d = (n.detail ?? {}) as D
    if (n.kind === 'user_message') out.push({ kind: 'user', node: n })
    else if (n.kind === 'assistant_message') out.push({ kind: 'assistant', node: n })
    else if (n.kind === 'tool_call') out.push({ kind: 'tool', node: n, result: d.tool_use_id ? results.get(d.tool_use_id) : undefined })
    else if (n.kind === 'tool_result' && !d.tool_use_id) out.push({ kind: 'tool', node: n })
  }
  return out
}

/** What a session's current turn is doing that no node holds yet: the streamed text and thinking, and the tools running. */
export interface LiveTurn { turn_id: string; text: string; thinking?: string; running?: { id: string; tool: string; startedAt: number }[] }

export function Transcript({ nodes, turns, live, asking, tightened }: {
  nodes: NodeInfo[]; turns: Map<string, TurnRow>; live?: LiveTurn | null
  /** The correlation ids of the calls waiting for the operator, and the tools "should have asked" tightened. */
  asking?: Set<string>; tightened?: Map<string, Tightening>
}) {
  const groups = useMemo(() => {
    const g: { turn_id: string | null; items: Item[] }[] = []
    for (const it of items(nodes)) {
      const t = it.node.turn_id ?? null
      if (!g.length || g[g.length - 1].turn_id !== t) g.push({ turn_id: t, items: [] })
      g[g.length - 1].items.push(it)
    }
    return g
  }, [nodes])

  return (
    <div className="flex flex-col gap-4 p-4">
      {groups.map((g, gi) => {
        const t = g.turn_id ? turns.get(g.turn_id) : undefined
        return (
          <section key={`${g.turn_id}-${gi}`} className="relative">
            <div className="sticky top-0 z-10 -mx-4 mb-2 flex items-center gap-2 border-y border-line bg-hull px-4 py-1 shadow-[0_6px_12px_-8px_rgba(0,0,0,0.8)]">
              <span className="panel-title">turn {gi + 1}</span>
              {t && <>
                <span className="num text-[11px] text-ink-faint">{stamp(t.start)}</span>
                <span className="num text-[11px] text-live">{ms(t.elapsed_ms)}</span>
                <span className="num text-[11px] text-model">{t.loops ?? '?'} loops</span>
                <span className="num text-[11px] text-tool">{t.tool_calls ?? 0} tools</span>
                <span className="num text-[11px] text-money">{usd(t.cost)}</span>
                {t.first_token_ms != null && <span className="num text-[11px] text-ink-faint" title="time to the first token">first token {ms(t.first_token_ms)}</span>}
                {t.model && <span className="num text-[11px] text-ink-faint">{t.model}</span>}
                {t.stop && <span className="num text-[11px] text-ink-faint">{t.stop}</span>}
                {t.failed && <Pill tone="fault">failed</Pill>}
              </>}
            </div>
            <div className="flex flex-col gap-2">
              {g.items.map((it) => it.kind === 'user' ? <UserItem key={it.node.node_id} n={it.node} />
                : it.kind === 'assistant' ? <AssistantItem key={it.node.node_id} n={it.node} />
                : <ToolItem key={it.node.node_id} call={it.node} result={it.result} asking={asking} tightened={tightened} />)}
              {live && live.turn_id === g.turn_id && <LiveItem live={live} />}
              {t?.failed && <TurnFailed t={t} />}
            </div>
          </section>
        )
      })}
      {live && !groups.some((g) => g.turn_id === live.turn_id) && <LiveItem live={live} />}
    </div>
  )
}

function Gutter({ icon, at, tone }: { icon: React.ReactNode; at?: number; tone: string }) {
  return (
    <div className="flex w-16 shrink-0 flex-col items-end gap-1 pt-1">
      <span className={cn('grid h-6 w-6 place-items-center rounded-md ring-1 ring-inset', tone)}>{icon}</span>
      <span className="num text-[10px] text-ink-faint">{at ? new Date(at).toLocaleTimeString([], { hour12: false }) : 'live'}</span>
    </div>
  )
}

function UserItem({ n }: { n: NodeInfo }) {
  return (
    <div className="flex gap-3">
      <Gutter icon={<User size={13} />} at={n.at_unix_ms} tone="bg-white/5 text-ink ring-line-strong" />
      <div className="min-w-0 flex-1 rounded-lg bg-white/[0.04] px-3 py-2 ring-1 ring-line">
        <div className="mb-0.5 text-[10px] font-semibold uppercase tracking-wider text-ink-faint">{n.author ?? 'user'}</div>
        <div className="whitespace-pre-wrap text-[13px] text-ink">{n.text}</div>
      </div>
    </div>
  )
}

/** Publish (the place rule): put this item into a place's conversation, as your message there, where everyone who
 *  can read the place will. Confirmed first; the core judges it (only the owner, from a private place). Off while the
 *  time machine shows the past. */
function PublishControl({ nodeId }: { nodeId: string }) {
  const world = useWorld()
  const { data: h } = useRpc<Health>('health', undefined, 10000)
  const [said, setSaid] = useState<string | null>(null)
  const places = (h?.places?.places ?? []).filter((p) => p.place.startsWith('discord:'))
  if (world || !places.length) return null
  if (said) return <span className="text-[11px] text-ink-faint">{said}</span>
  return (
    <select value="" title="publish this into a place: it becomes your message there"
      className="rounded bg-deck px-1 text-[11px] text-ink-faint ring-1 ring-line"
      onChange={async (e) => {
        const to = e.target.value
        const name = places.find((p) => p.place === to)?.name ?? to
        if (!to || !window.confirm(`Publish this into ${name}? Everyone who can read ${name} will.`)) return
        try { setSaid(`📎 published into ${(await call<PublishResult>('place.publish', { node_id: nodeId, to })).name}`) }
        catch (err: any) { setSaid(err?.message ?? String(err)) }
      }}>
      <option value="">📎 publish…</option>
      {places.map((p) => <option key={p.place} value={p.place}>{p.name} ({p.class})</option>)}
    </select>
  )
}

function AssistantItem({ n }: { n: NodeInfo }) {
  const d = (n.detail ?? {}) as D
  const [think, setThink] = useState(false)
  const [params, setParams] = useSearchParams()
  const inspecting = params.get('msg') === n.node_id
  const u = d.usage as D | undefined
  return (
    <div className="group flex gap-3">
      <Gutter icon={<Bot size={13} />} at={n.at_unix_ms} tone="bg-model/10 text-model ring-model/30" />
      <div className={cn('relative min-w-0 flex-1 rounded-lg bg-model/[0.05] px-3 py-2 ring-1', inspecting ? 'ring-model/60' : 'ring-model/15')}>
        <button
          onClick={() => setParams((p) => { if (inspecting) p.delete('msg'); else { p.set('msg', n.node_id); p.delete('call') } return p }, { replace: true })}
          title="inspect this model call: its time, tokens, cost, headroom, and context"
          className={cn('absolute -right-2 -top-2 z-10 rounded-md bg-deck p-1 ring-1 transition-opacity', inspecting ? 'text-model ring-model/60' : 'text-ink-faint opacity-0 ring-line group-hover:opacity-100 hover:text-model')}
        ><ScanSearch size={13} /></button>
        {n.thinking && (
          <button onClick={() => setThink((v) => !v)} className="mb-1 flex items-center gap-1 text-[11px] text-think">
            <ChevronRight size={12} className={cn('transition-transform', think && 'rotate-90')} /><Brain size={12} /> thinking · {n.thinking.length.toLocaleString()} chars
          </button>
        )}
        {think && n.thinking && <div className="mb-2 whitespace-pre-wrap rounded-md bg-think/5 px-2.5 py-1.5 text-[12px] italic text-ink-dim ring-1 ring-think/15">{n.thinking}</div>}
        {n.text ? <div className="md text-[13px] text-ink"><Markdown remarkPlugins={[remarkGfm]}>{n.text}</Markdown></div>
          : !n.thinking && <div className="text-[12px] text-ink-faint">(no text: tool calls only)</div>}
        <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[11px] text-ink-faint">
          {d.model && <span className="num text-model">{d.model}</span>}
          {d.cost_usd !== undefined && <span className="num text-money">{usd(d.cost_usd)}</span>}
          {u && <span className="num">{tokens((u.input_tokens ?? 0) + (u.cache_read_input_tokens ?? 0) + (u.cache_creation_input_tokens ?? 0))} in · {tokens(u.output_tokens)} out · {tokens(u.cache_read_input_tokens)} cached</span>}
          {d.stop_reason && <span className="num">{d.stop_reason}</span>}
          {Array.isArray(d.tool_calls) && d.tool_calls.length > 0 && <span className="num text-tool">→ {d.tool_calls.map((t: D) => t.name).join(', ')}</span>}
          {n.text && <PublishControl nodeId={n.node_id} />}
        </div>
      </div>
    </div>
  )
}

/** A failed turn, as the Observatory said it: its class, and the error (the daemon's words, not ours). */
function TurnFailed({ t }: { t: TurnRow }) {
  if (!t.error && !t.errorClass) return null
  return (
    <div className="ml-[76px] rounded-lg bg-fault/[0.06] px-3 py-2 text-[12px] ring-1 ring-fault/30">
      <span className="font-semibold text-fault">turn failed</span>
      {t.errorClass && <span className="text-ink-dim"> · class <span className="num">{t.errorClass}</span></span>}
      {t.error && <div className="mt-0.5 whitespace-pre-wrap text-ink-dim">{t.error}</div>}
    </div>
  )
}

/** A result's text: a diff draws with its additions and deletions marked, anything else as it is. */
function ResultText({ text }: { text: string }) {
  const box = 'max-h-72 overflow-auto whitespace-pre-wrap rounded-md bg-black/30 p-2.5 font-mono text-[11.5px] text-ink-dim ring-1 ring-line'
  if (!looksLikeDiff(text)) return <pre className={box}>{text}</pre>
  return (
    <pre className={box}>
      {diffLines(text).map((l, i) => (
        <span key={i} className={cn(l.kind === 'add' && 'text-ok', l.kind === 'del' && 'text-fault', l.kind === 'hunk' && 'text-live', l.kind === 'meta' && 'text-ink-faint')}>{l.line}{'\n'}</span>
      ))}
    </pre>
  )
}

function ToolItem({ call, result, asking, tightened }: { call: NodeInfo; result?: NodeInfo; asking?: Set<string>; tightened?: Map<string, Tightening> }) {
  const d = (call.detail ?? {}) as D
  const r = (result?.detail ?? (call.kind === 'tool_result' ? call.detail : null) ?? {}) as D
  const [open, setOpen] = useState(false)
  const [params, setParams] = useSearchParams()
  const callKey: string | undefined = d.tool_use_id ?? r.tool_use_id ?? d.correlation_id ?? r.correlation_id
  const inspecting = !!callKey && params.get('call') === callKey
  const tool = d.tool ?? r.tool ?? 'tool'
  const decision = d.decision as D | undefined
  // The gate's verdict is on the call's result (`{gate: allow|deny|confirm}`); older and denied calls carry
  // it as decision.mode. The posture (open, notify, confirm) is what the policy said about the tool.
  const gate: string | undefined = (d.result as D | undefined)?.gate ?? decision?.mode
  const posture: string | undefined = decision?.posture
  const words = resultWords(result?.detail ?? (call.kind === 'tool_result' ? call.detail : null))
  const corr: string | undefined = d.correlation_id ?? r.correlation_id
  const waiting = !result && !!corr && !!asking?.has(corr)
  const notice = decision?.notify as { setting?: string; rule?: string } | undefined
  const denied = gate === 'deny' || r.status === 'declined'
  const failed = r.is_error && !denied
  const tone = denied ? 'fault' : failed ? 'fault' : result ? 'ok' : 'wait'
  return (
    <div className="group flex gap-3">
      <Gutter icon={<Wrench size={13} />} at={call.at_unix_ms} tone="bg-tool/10 text-tool ring-tool/30" />
      <div className={cn('relative min-w-0 flex-1 rounded-lg bg-tool/[0.04] ring-1', inspecting ? 'ring-tool/60' : 'ring-tool/15')}>
        {callKey && (
          <button
            onClick={() => setParams((p) => { if (inspecting) p.delete('call'); else { p.set('call', callKey); p.delete('msg') } return p }, { replace: true })}
            title="inspect this call: its gate, its life in the ledger, its job, its result"
            className={cn('absolute -right-2 -top-2 z-10 rounded-md bg-deck p-1 ring-1 transition-opacity', inspecting ? 'text-tool ring-tool/60' : 'text-ink-faint opacity-0 ring-line group-hover:opacity-100 hover:text-tool')}
          ><ScanSearch size={13} /></button>
        )}
        <button onClick={() => setOpen((v) => !v)} className="flex w-full items-center gap-2 px-3 py-1.5 text-left">
          <ChevronRight size={13} className={cn('text-ink-faint transition-transform', open && 'rotate-90')} />
          <span className="num text-[12.5px] font-medium text-tool">{tool}</span>
          <span className="min-w-0 flex-1 truncate text-[12px] text-ink-dim">{d.plan?.summary ?? ((call.kind === 'tool_call' && callSummary(tool, d.input)) || summarizeInput(d.input))}</span>
          {gate && gate !== 'allow' && <Pill tone={gate === 'deny' ? 'fault' : 'wait'} title={decision?.reason}><ShieldCheck size={11} />{gate}</Pill>}
          {posture && gate === 'allow' && <span className="num text-[10.5px] text-ink-faint" title={decision?.reason}>{posture}</span>}
          {notice && gate !== 'notify' && <Pill tone="wait" title={`${notice.setting ?? ''}\n${notice.rule ?? ''}`}>notified</Pill>}
          {decision?.granted && <Pill tone="idle" title="the secret broker (names only)"><KeyRound size={10} />{String(decision.granted)}</Pill>}
          {decision?.class === 'l1' && <Pill tone="ok" title={l1Words(d.egress)}><Shield size={10} /> L1{Array.isArray(d.egress) && d.egress.length > 0 ? ' · egress' : ''}</Pill>}
          {words.exit != null && <span className={cn('num text-[11px]', words.exit === 0 ? 'text-ink-faint' : 'text-fault')}>exit {words.exit}</span>}
          {result && typeof (result as D).bytes === 'number' && <span className="num text-[11px] text-ink-faint">{byteWords((result as D).bytes)}</span>}
          {r.duration_ms !== undefined && r.duration_ms !== null && <span className="num text-[11px] text-ink-faint">{ms(r.duration_ms)}</span>}
          {words.stoppedBy != null
            ? <Pill tone="idle" title="a /stop ended this call">stopped by {words.stoppedBy}</Pill>
            : <Pill tone={waiting ? 'wait' : tone}>{denied ? <OctagonX size={11} /> : failed ? <OctagonX size={11} /> : result ? <CircleCheck size={11} /> : null}{waiting ? 'waits for you' : denied ? 'not run' : failed ? 'error' : result ? (r.status ?? 'ok') : 'pending'}</Pill>}
        </button>
        {notice && <div className="flex items-center gap-2 px-3 pb-1 pl-9"><ShouldHaveAsked tool={tool} corr={corr} tightened={tightened?.get(tool)} /></div>}
        {!open && result?.text && <div className="truncate border-t border-tool/10 px-3 py-1 font-mono text-[11.5px] text-ink-faint">{result.text.split('\n')[0]}</div>}
        {open && (
          <div className="flex flex-col gap-2 border-t border-tool/10 p-3">
            {(gate || decision) && <div className="text-[12px]"><span className="text-ink-faint">gate:</span> <span className={denied ? 'text-fault' : 'text-ok'}>{gate ?? '—'}</span>{posture && <span className="text-ink-faint"> · posture {posture}</span>}{decision?.reason && <span className="text-ink-dim"> · {decision.reason}</span>}</div>}
            {Array.isArray(d.plan?.resources) && d.plan.resources.length > 0 && (
              <div className="flex flex-wrap gap-1">{d.plan.resources.map((res: D, i: number) => <Pill key={i} tone="tool">{res.access} {res.path ?? res.host ?? JSON.stringify(res)}</Pill>)}</div>
            )}
            <div><div className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-ink-faint">input</div><JsonView value={d.input ?? {}} maxHeight="240px" /></div>
            {result && (
              <div>
                <div className="mb-1 flex items-center gap-2 text-[10px] font-semibold uppercase tracking-wider text-ink-faint">
                  result {r.truncated && <Pill tone="wait" title={r.full_ref ? `full output: ${r.full_ref}` : undefined}>truncated</Pill>} {r.late && <Pill tone="wait">late</Pill>} {r.external && <Pill tone="wait">external text</Pill>}
                  <span className="ml-auto normal-case tracking-normal"><PublishControl nodeId={result.node_id} /></span>
                </div>
                <ResultText text={result.text} />
                {r.meta && Object.keys(r.meta).length > 0 && <div className="mt-1"><JsonView value={r.meta} maxHeight="160px" /></div>}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

function LiveItem({ live }: { live: LiveTurn }) {
  const now = useTick(1000)
  const running = live.running ?? []
  if (!live.text && !live.thinking && !running.length) return null
  return (
    <div className="flex gap-3">
      <Gutter icon={<Bot size={13} />} tone="bg-live/10 text-live ring-live/40" />
      <div className="live-sweep min-w-0 flex-1 rounded-lg bg-live/[0.05] px-3 py-2 ring-1 ring-live/30">
        <div className="mb-0.5 text-[10px] font-semibold uppercase tracking-wider text-live">streaming</div>
        {live.thinking && (
          <div className="mb-2 max-h-40 overflow-auto whitespace-pre-wrap rounded-md bg-think/5 px-2.5 py-1.5 text-[12px] italic text-ink-dim ring-1 ring-think/15">
            <span className="mr-1 inline-flex items-center gap-1 not-italic text-think"><Brain size={12} /> thinking…</span>{live.thinking}
          </div>
        )}
        {live.text && <div className="md text-[13px] text-ink"><Markdown remarkPlugins={[remarkGfm]}>{live.text}</Markdown><span className="ml-0.5 inline-block h-3.5 w-1.5 animate-pulse bg-live align-middle" /></div>}
        {running.map((t) => (
          <div key={t.id} className="num mt-1 flex items-center gap-2 text-[11.5px] text-tool"><Wrench size={11} /> {t.tool} <span className="text-live">running {Math.max(0, Math.round((now - t.startedAt) / 1000))} s…</span></div>
        ))}
      </div>
    </div>
  )
}

function summarizeInput(input: unknown): string {
  if (!input || typeof input !== 'object') return ''
  const o = input as D
  return o.command ?? o.path ?? o.pattern ?? o.query ?? o.url ?? JSON.stringify(o).slice(0, 120)
}
