// The turn inspector (theseus-x9swu), at /session/<id>/turn/<turn>: one turn, whole. The prompt, the outcome and the
// totals; the flame graph of the turn on one axis; then every loop (an accordion) with the request as compiled, the
// answer in full (thinking included), and each tool call with its input, gate record and result; a loop's request
// against the one before. One read, `turn.inspect`; a loop's system parts come from `context.explain`, asked when a
// request is opened. Thinking, texts and tool inputs are private: the daemon gives them to this page alone.
import { useMemo, useState, type ReactNode } from 'react'
import { Link, useParams, useSearchParams } from 'react-router'
import { ArrowLeft, ArrowRight, Brain, ChevronDown, ChevronRight, ClipboardCopy, Layers, Wrench } from 'lucide-react'
import type { ContextExplainResult, InspectCall, InspectLoop, TurnInspectResult } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { cn, ms, short, stamp, tokens, us, usd } from '@/lib/format'
import { blockWords, breakpointParts, cacheShare, callTiming, layout, loopDiff, rowsOfCall, signed, where, type Bar } from '@/lib/inspect'
import { Empty, Panel, Pill } from '@/components/ui'
import { JsonView } from '@/components/JsonView'
import { TurnFlame } from '@/components/TurnFlame'
import { stateTone } from '@/lib/taxonomy'

export default function TurnInspect() {
  const { id = '', turn = '' } = useParams()
  const [params, setParams] = useSearchParams()
  const { data, error, isLoading } = useRpc<TurnInspectResult>('turn.inspect', { session_id: id, turn_id: turn }, 0, {
    refetchInterval: (q) => (q.state.data?.outcome === 'running' ? 1500 : false),
  })
  const [picked, setPicked] = useState<Bar | null>(null)
  const open = useMemo(() => new Set((params.get('loops') ?? '0').split(',').filter(Boolean).map(Number)), [params])
  const toggle = (i: number) => setParams((p) => {
    const s = new Set(open)
    if (s.has(i)) s.delete(i); else s.add(i)
    p.set('loops', [...s].sort((a, b) => a - b).join(','))
    return p
  }, { replace: true })

  if (isLoading && !data) return <div className="p-6 text-ink-faint">Reading the turn…</div>
  if (error || !data) return <Empty>{error ? String((error as Error).message ?? error) : 'no such turn'}</Empty>
  const loopOpen = (i: number) => open.has(i)
  return (
    <div className="flex flex-col gap-3 pb-8">
      <Header r={data} />
      {data.withheld && <div className="rounded-md bg-wait/10 px-3 py-1.5 text-[12px] text-wait ring-1 ring-inset ring-wait/30">{data.withheld}</div>}
      <Panel title="The turn on one clock" icon={<Layers size={13} />} help={false} bodyClassName="p-0">
        <TurnFlame trace={data.trace} rows={data.rows} endedMs={data.ended_ms} startedMs={data.started_ms} selected={picked?.id ?? null} onSelect={(b) => {
          setPicked(b)
          if (b?.loop != null && !open.has(b.loop)) toggle(b.loop)
        }} />
        {picked && (
          <div className="border-t border-line p-2">
            <div className="mb-1 flex items-baseline gap-2 text-[12px]">
              <span className="font-semibold text-ink">{picked.name}</span><span className="text-ink-faint">{picked.lane} · {picked.kind}{picked.loop != null ? ` · loop ${picked.loop}` : ''}</span>
              <span className="num ml-auto text-live">{ms((picked.end - picked.start) / 1000)}</span>
            </div>
            {picked.attrs != null && <JsonView value={picked.attrs} maxHeight="140px" />}
          </div>
        )}
      </Panel>
      <div className="flex flex-col gap-2">
        {data.loops.length === 0 && <Empty>this turn made no model call</Empty>}
        {data.loops.map((l, k) => (
          <LoopCard key={l.loop} l={l} prev={data.loops[k - 1]} r={data} open={loopOpen(l.loop)} onToggle={() => toggle(l.loop)} />
        ))}
      </div>
      <Rows r={data} />
    </div>
  )
}

/** A state as a pill: a turn's outcome, or a call's status. */
function statePill(state: string): ReactNode {
  const tone = state === 'ended' ? 'ok' : state === 'running' ? 'live' : state === 'failed' || state === 'refused' ? 'fault' : stateTone(state)
  return <Pill tone={tone}>{state}</Pill>
}

// ---------------------------------------------------------------- header

function Header({ r }: { r: TurnInspectResult }) {
  const t = r.totals
  const u = t.usage
  const route = r.route as Record<string, unknown> | null
  const copy = () => { void navigator.clipboard?.writeText(JSON.stringify(r, null, 2)) }
  const share = cacheShare(u)
  return (
    <Panel help={false} bodyClassName="flex flex-col gap-2 p-3">
      <div className="flex flex-wrap items-center gap-2">
        <Link to={`/session/${r.session_id}?turn=${r.turn_id}`} className="text-[12px] text-ink-faint hover:text-ink">← session {short(r.session_id)}</Link>
        <span className="panel-title">turn {short(r.turn_id)}</span>
        {statePill(r.outcome)}
        {r.stop_reason && <span className="num text-[11px] text-ink-faint">{r.stop_reason}</span>}
        <span className="num text-[11px] text-ink-faint">{stamp(r.started_ms)}</span>
        <div className="ml-auto flex items-center gap-1">
          {r.prev_turn ? <Link to={`/session/${r.session_id}/turn/${r.prev_turn}`} className="rounded px-2 py-1 text-[11.5px] ring-1 ring-inset ring-line hover:text-ink" aria-label="previous turn"><ArrowLeft size={12} className="inline" /> prev</Link> : null}
          {r.next_turn ? <Link to={`/session/${r.session_id}/turn/${r.next_turn}`} className="rounded px-2 py-1 text-[11.5px] ring-1 ring-inset ring-line hover:text-ink" aria-label="next turn">next <ArrowRight size={12} className="inline" /></Link> : null}
          <button type="button" onClick={copy} className="flex items-center gap-1 rounded px-2 py-1 text-[11.5px] ring-1 ring-inset ring-line hover:text-ink"><ClipboardCopy size={12} />copy as JSON</button>
        </div>
      </div>
      <div className="rounded-md bg-black/20 px-3 py-2 ring-1 ring-inset ring-line">
        <div className="mb-0.5 text-[11px] text-ink-faint">{r.prompt.author ?? 'the harness'} · {stamp(r.prompt.at_ms)} · {r.prompt.chars.toLocaleString('en-US')} chars</div>
        {r.prompt.text ? <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-words text-[12.5px] text-ink">{r.prompt.text}</pre> : <div className="text-[12px] text-ink-faint">{r.prompt.chars ? '(withheld)' : 'no input: a continuation'}</div>}
      </div>
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4 lg:grid-cols-8">
        <Tile label="wall time" value={ms(t.wall_ms)} />
        <Tile label="loops" value={String(t.loops)} />
        <Tile label="model calls" value={String(t.model_calls)} />
        <Tile label="tool calls" value={String(t.tool_calls)} />
        <Tile label="input" value={tokens(u.input_tokens)} />
        <Tile label="cache read" value={tokens(u.cache_read_input_tokens)} note={share != null ? `${Math.round(share * 100)}% of input` : undefined} />
        <Tile label="cache write" value={tokens(u.cache_creation_input_tokens)} />
        <Tile label="output" value={tokens(u.output_tokens)} />
        <Tile label="cost" value={usd(t.cost_usd)} />
        <Tile label="errors" value={String(t.errors)} bad={t.errors > 0} />
      </div>
      {route && Object.keys(route).length > 0 && <RouteLine route={route} />}
    </Panel>
  )
}

function RouteLine({ route }: { route: Record<string, unknown> }) {
  const bits = ['model', 'profile', 'mode', 'effort_applied', 'effort', 'reason'].filter((k) => route[k] != null && route[k] !== '').map((k) => `${k.replace('_applied', '')} ${String(route[k])}`)
  return <div className="num text-[11.5px] text-ink-faint" title={JSON.stringify(route)}>route · {bits.join(' · ') || 'decided'}</div>
}

function Tile({ label, value, note, bad }: { label: string; value: string; note?: string; bad?: boolean }) {
  return (
    <div className="rounded-md bg-black/20 px-2.5 py-1.5 ring-1 ring-inset ring-line">
      <div className="text-[10.5px] uppercase tracking-wide text-ink-faint">{label}</div>
      <div className={cn('num text-[15px]', bad ? 'text-fault' : 'text-ink')}>{value}</div>
      {note && <div className="num text-[10.5px] text-ink-faint">{note}</div>}
    </div>
  )
}

// ---------------------------------------------------------------- a loop

type Tab = 'request' | 'response' | 'calls' | 'diff'

function LoopCard({ l, prev, r, open, onToggle }: { l: InspectLoop; prev: InspectLoop | undefined; r: TurnInspectResult; open: boolean; onToggle: () => void }) {
  const [tab, setTab] = useState<Tab>('response')
  const a = l.answer
  const failed = l.calls.some((c) => c.is_error)
  return (
    <section className="panel" data-loop={l.loop}>
      <button type="button" onClick={onToggle} aria-expanded={open} className="flex w-full flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 text-left">
        {open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        <span className="panel-title">loop {l.loop}</span>
        {a && <span className="num text-[11px] text-model">{a.model}</span>}
        {a?.stop_reason && <span className="num text-[11px] text-ink-faint">{a.stop_reason}</span>}
        {a && <span className="num text-[11px] text-ink-faint">in {tokens(a.usage.input_tokens + a.usage.cache_read_input_tokens + a.usage.cache_creation_input_tokens)} · out {tokens(a.usage.output_tokens)}</span>}
        {a?.cost_usd != null && <span className="num text-[11px] text-money">{usd(a.cost_usd)}</span>}
        {a && a.thinking_chars > 0 && <span className="flex items-center gap-1 text-[11px] text-think"><Brain size={11} />{a.thinking_chars.toLocaleString('en-US')} chars of thinking</span>}
        {l.calls.length > 0 && <span className="flex items-center gap-1 text-[11px] text-tool"><Wrench size={11} />{l.calls.length}</span>}
        {failed && <Pill tone="fault">a call failed</Pill>}
      </button>
      {open && (
        <div className="border-t border-line">
          <nav className="flex gap-1 px-2 pt-1.5" aria-label={`loop ${l.loop}`}>
            {(['request', 'response', 'calls', 'diff'] as const).map((t) => (
              <button key={t} type="button" onClick={() => setTab(t)} aria-current={tab === t ? 'page' : undefined}
                className={cn('rounded-md px-2.5 py-1 text-[12px] ring-1 ring-inset', tab === t ? 'bg-live/10 text-live ring-live/30' : 'text-ink-dim ring-line hover:text-ink')}>
                {t === 'diff' ? 'changed since the last loop' : t}
              </button>
            ))}
          </nav>
          <div className="p-3">
            {tab === 'request' && <Request l={l} r={r} />}
            {tab === 'response' && <Response l={l} r={r} />}
            {tab === 'calls' && <Calls l={l} r={r} />}
            {tab === 'diff' && <Diff l={l} prev={prev} />}
          </div>
        </div>
      )}
    </section>
  )
}

function Kv({ k, children }: { k: string; children: ReactNode }) {
  return <div className="flex gap-2 text-[12px]"><span className="w-36 shrink-0 text-ink-faint">{k}</span><span className="num min-w-0 break-words text-ink">{children}</span></div>
}

/** The request as sent: how it was compiled, the cache's breakpoints, the system parts in order, then what it added. */
function Request({ l, r }: { l: InspectLoop; r: TurnInspectResult }) {
  const c = l.compiled
  const { data: x } = useRpc<ContextExplainResult>('context.explain', { session_id: r.session_id, turn_id: r.turn_id }, 0)
  if (!c) return <Empty>this loop has no compile row</Empty>
  const stops = x ? breakpointParts(x.parts, c.cache.breakpoints) : new Set<number>()
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-1">
        <Kv k="compile">{c.decision}{c.trigger ? ` (${c.trigger})` : ''} · {c.strategy}</Kv>
        <Kv k="size">≈ {tokens(c.est_tokens)} tokens · {c.messages} messages ({c.prefix_nodes} in the prefix, {c.tail_nodes} in the tail) · {c.tools} tools offered</Kv>
        <Kv k="digest">{c.digest}</Kv>
        <Kv k="cache">breakpoints {c.cache.breakpoints.join(' → ') || 'none'} · system ttl {c.cache.ttl} · conversation ttl {c.cache.conversation_ttl}</Kv>
        {l.answer && <Kv k="cache state">{tokens(l.answer.usage.cache_read_input_tokens)} read · {tokens(l.answer.usage.cache_creation_input_tokens)} written · {tokens(l.answer.usage.input_tokens)} uncached</Kv>}
        {Object.entries(c.cache.layers ?? {}).map(([k, v]) => <Kv key={k} k={`layer ${k}`}>{v}</Kv>)}
        {c.repairs.length > 0 && <Kv k="repairs">{c.repairs.join(', ')} had no result and were given a synthetic one</Kv>}
      </div>
      <div>
        <h3 className="panel-title mb-1">System blocks, in order</h3>
        {!x ? <div className="text-[12px] text-ink-faint">reading the parts…</div> : (
          <>
            <div className="mb-1 text-[11.5px] text-ink-faint">
              {x.unchanged === true ? 'These are the bytes this turn saw (the digests agree).' : x.unchanged === false ? 'The system blocks have changed since this turn: the parts below are as the next turn would build them.' : ''}
              {x.withheld ? ` ${x.withheld}` : ''}
            </div>
            <table className="w-full text-[12px]">
              <thead><tr className="text-left text-ink-faint"><th className="py-0.5 pr-2 font-normal">block</th><th className="pr-2 font-normal">part</th><th className="pr-2 text-right font-normal">tokens</th><th className="font-normal">vs. this turn</th></tr></thead>
              <tbody>
                {x.parts.map((p, i) => (
                  <PartRows key={`${p.block}-${p.name}-${i}`} p={p} stop={stops.has(i)} />
                ))}
              </tbody>
            </table>
          </>
        )}
      </div>
      <RequestMessages l={l} r={r} />
      {l.added.length > 0 && <div className="text-[12px] text-ink-dim">This request added {l.added.length} node{l.added.length === 1 ? '' : 's'} to the one before it: the last answer, its tool calls and their results ({l.added.map(short).join(', ')}).</div>}
    </div>
  )
}

/** The messages this request carried from the turn itself: the prompt, then each earlier loop's answer, its calls and
 *  their results, in the order they went. The conversation before the turn is the compile's prefix (counted above). */
function RequestMessages({ l, r }: { l: InspectLoop; r: TurnInspectResult }) {
  const items: { who: string; what: string; text: string }[] = []
  if (r.prompt.node_id) items.push({ who: 'user', what: `prompt · ${r.prompt.chars.toLocaleString('en-US')} chars`, text: r.prompt.text })
  for (const p of r.loops) {
    if (p.loop >= l.loop) break
    for (const b of (p.answer?.blocks ?? []) as Record<string, unknown>[]) {
      items.push({ who: 'assistant', what: blockWords(b), text: String(b.thinking ?? b.text ?? (b.type === 'tool_use' ? JSON.stringify(b.input) : '')) })
    }
    for (const c of p.calls) items.push({ who: 'tool result', what: `${c.tool}${c.is_error ? ' (error)' : ''} · ${c.bytes_total.toLocaleString('en-US')} bytes${c.truncated ? ' · cut' : ''}`, text: c.content })
  }
  if (!items.length) return null
  return (
    <div>
      <h3 className="panel-title mb-1">Messages from this turn, in order</h3>
      <div className="flex flex-col gap-1">
        {items.map((m, i) => (
          <details key={i} className="rounded-md bg-black/20 px-2.5 py-1 ring-1 ring-inset ring-line">
            <summary className="cursor-pointer text-[12px]"><span className="text-ink-faint">{m.who}</span> · <span className="text-ink">{m.what}</span></summary>
            {m.text && <pre className="mt-1 max-h-60 overflow-auto whitespace-pre-wrap break-words text-[11.5px] text-ink-dim">{m.text}</pre>}
          </details>
        ))}
      </div>
    </div>
  )
}

function PartRows({ p, stop }: { p: ContextExplainResult['parts'][number]; stop: boolean }) {
  const [more, setMore] = useState(false)
  return (
    <>
      <tr className="border-t border-line/50 align-top">
        <td className="py-0.5 pr-2 text-ink-dim">{p.block}</td>
        <td className="pr-2 text-ink">{p.name}{p.text != null && <button type="button" onClick={() => setMore(!more)} className="ml-2 text-[11px] text-live">{more ? 'hide' : 'show text'}</button>}</td>
        <td className="num pr-2 text-right">{p.tokens.toLocaleString('en-US')}</td>
        <td className="text-ink-faint">{p.then ?? ''}{p.note ? ` · ${p.note}` : ''}</td>
      </tr>
      {more && p.text != null && <tr><td colSpan={4}><pre className="max-h-72 overflow-auto whitespace-pre-wrap break-words rounded bg-black/30 p-2 text-[11.5px]">{p.text}</pre></td></tr>}
      {stop && <tr><td colSpan={4} className="py-0.5 text-[11px] text-money">◆ cache breakpoint: everything above is written to the cache here</td></tr>}
    </>
  )
}

/** The answer: thinking in full, text, tool_use blocks, the stop reason, the usage. */
function Response({ l, r }: { l: InspectLoop; r: TurnInspectResult }) {
  const a = l.answer
  const bars = useMemo(() => layout(r.trace, r.rows, r.ended_ms, r.started_ms).bars, [r])
  const t = callTiming(bars, l.loop)
  const row = r.rows.find((x) => x.kind === 'provider.call' && (x.data as { loop?: number } | null)?.loop === l.loop)
  const timing = (row?.data as { timing?: { first_byte_ms?: number | null; first_token_ms?: number | null; total_ms?: number | null } } | null)?.timing
  if (!a) return <Empty>no answer was recorded for this loop</Empty>
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-1">
        <Kv k="model">{a.provider}/{a.model}{a.effort ? ` · effort ${a.effort}` : ''}</Kv>
        <Kv k="stop reason">{a.stop_reason ?? '—'}</Kv>
        <Kv k="usage">in {a.usage.input_tokens.toLocaleString('en-US')} · cache read {a.usage.cache_read_input_tokens.toLocaleString('en-US')} · cache write {a.usage.cache_creation_input_tokens.toLocaleString('en-US')} · out {a.usage.output_tokens.toLocaleString('en-US')}</Kv>
        {a.cost_usd != null && <Kv k="cost">{usd(a.cost_usd)}</Kv>}
        {a.request_id && <Kv k="request id">{a.request_id}</Kv>}
        {a.request_digest && <Kv k="request digest">{a.request_digest}</Kv>}
        {a.redacted.map((x, i) => (
          <Kv key={i} k="vault secrets">{x.names.length ? x.names.join(', ') : `${x.count} value${x.count === 1 ? '' : 's'}`} · {x.substituted ? 'substituted before it left' : 'sent as written (redact_outbound is off)'}{where(x)}</Kv>
        ))}
        {t && <Kv k="latency">call {us(t.call)}{t.firstByte != null ? ` · first byte ${us(t.firstByte)}` : ''}{t.firstToken != null ? ` · first token ${us(t.firstToken)}` : ''}{t.streaming != null ? ` · streaming ${us(t.streaming)}` : ''}</Kv>}
        {!t && timing && <Kv k="latency">{timing.total_ms != null ? `call ${ms(timing.total_ms)}` : ''}{timing.first_byte_ms != null ? ` · first byte ${ms(timing.first_byte_ms)}` : ''}{timing.first_token_ms != null ? ` · first token ${ms(timing.first_token_ms)}` : ''}</Kv>}
        <Kv k="at">{stamp(a.at_ms)}</Kv>
      </div>
      {a.blocks.map((b, i) => <Block key={i} b={b as Record<string, unknown>} />)}
    </div>
  )
}

function Block({ b }: { b: Record<string, unknown> }) {
  const type = String(b.type ?? '')
  const text = type === 'thinking' ? (b.thinking as string | undefined) : type === 'text' ? (b.text as string | undefined) : undefined
  return (
    <div className={cn('rounded-md ring-1 ring-inset', type === 'thinking' ? 'bg-think/5 ring-think/25' : 'bg-black/20 ring-line')}>
      <div className={cn('px-2.5 pt-1.5 text-[11px]', type === 'thinking' ? 'text-think' : 'text-ink-faint')}>{blockWords(b)}</div>
      {text != null ? <pre className="max-h-[32rem] overflow-auto whitespace-pre-wrap break-words px-2.5 pb-2 pt-1 text-[12.5px] text-ink">{text}</pre>
        : type === 'tool_use' || type === 'redacted_thinking' ? <div className="p-2"><JsonView value={b} maxHeight="200px" /></div>
          : <div className="px-2.5 pb-2 text-[11.5px] text-ink-faint">{type === 'thinking' || type === 'text' ? 'withheld from this surface' : ''}</div>}
    </div>
  )
}

/** Each call of the loop: input, gate record, result as the model saw it, and the ledger rows that name it. */
function Calls({ l, r }: { l: InspectLoop; r: TurnInspectResult }) {
  if (!l.calls.length) return <Empty>this loop called no tool</Empty>
  return <div className="flex flex-col gap-2">{l.calls.map((c) => <CallCard key={c.tool_use_id} c={c} r={r} />)}</div>
}

function CallCard({ c, r }: { c: InspectCall; r: TurnInspectResult }) {
  const [gate, setGate] = useState(false)
  const rows = useMemo(() => rowsOfCall(r.rows, c.tool_use_id, c.correlation_id), [r.rows, c])
  return (
    <div className="rounded-md bg-black/20 ring-1 ring-inset ring-line" data-call={c.tool_use_id}>
      <div className="flex flex-wrap items-center gap-2 px-2.5 py-1.5">
        <Wrench size={12} className="text-tool" />
        <span className="font-semibold text-ink">{c.tool}</span>
        {c.status ? statePill(c.status) : <Pill tone="wait">no result</Pill>}
        {c.is_error && <Pill tone="fault">error</Pill>}
        {c.late && <Pill tone="wait">late result</Pill>}
        {c.duration_ms != null && <span className="num text-[11px] text-live">{ms(c.duration_ms)}</span>}
        <span className="num text-[11px] text-ink-faint">{c.bytes_total.toLocaleString('en-US')} bytes{c.truncated ? ' · cut' : ''}</span>
        <span className="num ml-auto text-[10.5px] text-ink-faint">{c.tool_use_id}</span>
      </div>
      <div className="grid gap-2 p-2.5 lg:grid-cols-2">
        <div><div className="mb-1 text-[11px] text-ink-faint">input</div>{c.input == null ? <div className="text-[11.5px] text-ink-faint">withheld</div> : <JsonView value={c.input} maxHeight="260px" />}</div>
        <div>
          <div className="mb-1 text-[11px] text-ink-faint">result as the model saw it{c.truncated ? ' (capped)' : ''}</div>
          {c.content ? <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-words rounded bg-black/30 p-2 text-[11.5px] text-ink">{c.content}</pre> : <div className="text-[11.5px] text-ink-faint">{c.result_node ? 'empty or withheld' : 'not answered yet'}</div>}
          {c.full_ref && <div className="num mt-1 text-[11px] text-ink-faint">the full output is kept: {c.full_ref}</div>}
        </div>
      </div>
      {c.gate && (
        <div className="border-t border-line px-2.5 py-1.5">
          <button type="button" onClick={() => setGate(!gate)} className="text-[11.5px] text-live">{gate ? 'hide' : 'show'} the gate's record (approval, floor question, plan)</button>
          {gate && <div className="mt-1"><JsonView value={c.gate} maxHeight="260px" /></div>}
        </div>
      )}
      {rows.length > 0 && (
        <div className="border-t border-line px-2.5 py-1.5 text-[11.5px] text-ink-dim">
          ledger: {rows.map((x) => `${x.kind} @${x.position}`).join(' · ')}
        </div>
      )}
    </div>
  )
}

/** This loop's request against the one before: size, layers, and the nodes it added. */
function Diff({ l, prev }: { l: InspectLoop; prev: InspectLoop | undefined }) {
  if (!prev) return <Empty>the first loop has nothing before it</Empty>
  const d = loopDiff(prev, l)
  const added: { what: string; text: string }[] = []
  if (prev.answer) for (const b of prev.answer.blocks as Record<string, unknown>[]) added.push({ what: blockWords(b), text: String(b.thinking ?? b.text ?? (b.type === 'tool_use' ? JSON.stringify(b.input) : '')) })
  for (const c of prev.calls) added.push({ what: `result of ${c.tool}${c.is_error ? ' (error)' : ''}`, text: c.content })
  return (
    <div className="flex flex-col gap-3">
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
        <Tile label="tokens" value={signed(d.tokens)} />
        <Tile label="messages" value={signed(d.messages)} />
        <Tile label="tools offered" value={signed(d.tools)} />
        <Tile label="nodes added" value={String(d.addedNodes)} />
      </div>
      <div className="text-[12px] text-ink-dim">
        {d.recompiled ? 'This loop recompiled the context. ' : 'This loop appended to the compilation. '}
        {d.digestChanged ? 'The request digest changed. ' : 'The request digest is the same. '}
        {d.layers.length ? `System layers that changed: ${d.layers.join(', ')}. ` : 'No system layer changed. '}
        {d.breakpointsChanged ? 'The cache breakpoints moved.' : ''}
      </div>
      <div>
        <h3 className="panel-title mb-1">What the last loop added to the context</h3>
        <div className="flex flex-col gap-1.5">
          {added.map((a, i) => (
            <div key={i} className="rounded-md border-l-2 border-ok/60 bg-ok/5 px-2.5 py-1.5">
              <div className="text-[11px] text-ok">+ {a.what}</div>
              {a.text && <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-words text-[11.5px] text-ink-dim">{a.text}</pre>}
            </div>
          ))}
          {added.length === 0 && <Empty>nothing was added</Empty>}
        </div>
      </div>
    </div>
  )
}

/** Every ledger row the turn wrote, folded. */
function Rows({ r }: { r: TurnInspectResult }) {
  const [open, setOpen] = useState(false)
  return (
    <Panel title={`Ledger rows of this turn (${r.rows.length})`} help={false} actions={<button type="button" className="text-[11.5px] text-live" onClick={() => setOpen(!open)}>{open ? 'hide' : 'show'}</button>}>
      {open && (
        <table className="w-full text-[11.5px]">
          <tbody>
            {r.rows.map((x) => (
              <tr key={x.position} className="border-t border-line/50 align-top">
                <td className="num px-2 py-0.5 text-ink-faint">{x.position}</td>
                <td className="num px-2 text-ink-faint">{stamp(x.at_ms)}</td>
                <td className="px-2 text-ink">{x.kind}</td>
                <td className="num max-w-[40rem] truncate px-2 text-ink-dim" title={JSON.stringify(x.data)}>{x.data == null ? '' : JSON.stringify(x.data)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </Panel>
  )
}
