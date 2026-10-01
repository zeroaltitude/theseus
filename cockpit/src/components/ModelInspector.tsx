// The model-call inspector (theseus-45n5): one model call's whole story. Where its time went (the first byte, the
// first token, the stream), what its tokens were and what the cache saved, what it cost by the catalog's prices,
// the provider's rate-limit headroom at that moment, the context it saw, what it said and asked for, and its life
// as a kernel action. Gathered from the assistant node and its `provider.call` row. It lives in the address
// (`?msg=<node_id>` on a session's page).
import { useMemo, useState } from 'react'
import { useSearchParams } from 'react-router'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { Bot, Brain, ChevronRight, Wrench } from 'lucide-react'
import type { CatalogList, CompilationInfo, LedgerEntry, SessionHistory } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { useLedger } from '@/lib/derive'
import { cn, ms, stamp, tokens, usd } from '@/lib/format'
import { toneHex } from '@/lib/taxonomy'
import { Pill } from './ui'
import { Drawer, Ident, Life, Raw, Section } from './CallInspector'

type D = Record<string, any>

export function ModelInspector({ sessionId }: { sessionId: string }) {
  const [params, setParams] = useSearchParams()
  const key = params.get('msg')
  const close = () => setParams((p) => { p.delete('msg'); return p }, { replace: true })
  const openCall = (id: string) => setParams((p) => { p.delete('msg'); p.set('call', id); return p }, { replace: true })
  const { data: hist } = useRpc<SessionHistory>('session.history', { session_id: sessionId }, 5000, { enabled: !!key })
  const { data: tail } = useLedger(5000, 0, undefined, undefined, !!key)
  const { data: cat } = useRpc<CatalogList>('catalog.list', undefined, 60_000, { enabled: !!key })
  const { data: comps } = useRpc<{ compilations: CompilationInfo[] }>('compilation.list', { session_id: sessionId, n: 50 }, 10_000, { enabled: !!key })
  const node = useMemo(() => hist?.nodes.find((n) => n.node_id === key), [hist, key])
  const d = (node?.detail ?? {}) as D
  const pc = useMemo(() => (tail?.rows ?? []).find((r) => r.kind === 'provider.call' && (r.data as D | null)?.node_id === key), [tail, key])
  const p = (pc?.data ?? {}) as D
  const cid: string | undefined = d.correlation_id
  const life = useMemo(
    () => (tail?.rows ?? []).filter((r) => cid && (r.data as D | null)?.correlation_id === cid).sort((a, b) => a.at_unix_ms - b.at_unix_ms),
    [tail, cid],
  )
  if (!key) return null
  const model: string = p.model ?? d.model ?? 'model'
  const provider: string = p.provider ?? d.provider ?? ''
  const usage = (p.usage ?? d.usage ?? {}) as D
  const timing = (p.timing ?? {}) as D
  const price = cat?.models.find((m) => m.model === model)?.entry as D | undefined
  const comp = comps?.compilations.find((c) => c.compilation_id === d.compilation_id)

  return (
    <Drawer onClose={close} head={<>
      <Bot size={15} className="text-model" />
      <span className="num text-[14px] font-semibold text-model">{model}</span>
      <span className="num text-[12px] text-ink-faint">{provider}</span>
      {(p.stop_reason ?? d.stop_reason) && <Pill tone={(p.stop_reason ?? d.stop_reason) === 'end_turn' ? 'ok' : 'live'}>{p.stop_reason ?? d.stop_reason}</Pill>}
      {(p.cost_usd ?? d.cost_usd) !== undefined && <span className="num text-[12px] text-money">{usd(p.cost_usd ?? d.cost_usd)}</span>}
      {timing.total_ms !== undefined && <span className="num text-[12px] text-ink-dim">{ms(timing.total_ms)}</span>}
    </>}>
      {!node ? (
        <div className="p-6 text-[12.5px] text-ink-faint">{hist ? `No model reply ${key} in this session.` : 'reading the session…'}</div>
      ) : (
        <div className="min-h-0 flex-1 overflow-auto px-4 py-3">
          <div className="mb-3 flex flex-wrap gap-x-4 gap-y-1 text-[11px] text-ink-faint">
            {p.request_id && <Ident label="request" value={p.request_id} />}
            {cid && <Ident label="action" value={cid} />}
            <span className="num">{stamp(node.at_unix_ms)}</span>
            {node.turn_id && <span className="num">turn …{node.turn_id.slice(-6)} · loop {node.loop_index ?? '?'}</span>}
            {p.catalog_version && <span className="num">prices of {p.catalog_version}</span>}
          </div>

          <Section title="Where its time went">
            {timing.total_ms !== undefined ? <Timing t={timing} /> : <div className="text-[12px] text-ink-faint">No timing on record for this call.</div>}
          </Section>

          <Section title="Its tokens">
            <TokenBar u={usage} price={price} />
          </Section>

          {price && (
            <Section title="What it cost, at today's catalog prices">
              <Cost u={usage} price={price} charged={p.cost_usd ?? d.cost_usd} />
            </Section>
          )}

          {p.rate_limit && (
            <Section title="The provider's headroom at that moment">
              <Headroom rl={p.rate_limit as D} />
            </Section>
          )}

          <Section title="The context it saw">
            {comp ? (
              <div className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-[12px]">
                <span className="text-ink-faint">compilation</span><span className="num text-think">…{comp.compilation_id.slice(-10)}{comp.current ? ' · current' : ''}</span>
                <span className="text-ink-faint">trigger · strategy</span><span className="num text-ink-dim">{comp.trigger} · {comp.strategy}</span>
                <span className="text-ink-faint">nodes included</span><span className="num text-ink">{comp.includes.toLocaleString()}</span>
                <span className="text-ink-faint">as of</span><span className="num text-ink-dim">position {comp.as_of.toLocaleString()}</span>
                <span />
                <button onClick={() => setParams((q) => { q.set('tab', 'context'); return q }, { replace: true })} className="justify-self-start text-[11px] text-live hover:underline">its manifest, in the Context tab →</button>
              </div>
            ) : <div className="text-[12px] text-ink-faint">{d.compilation_id ? `compilation …${String(d.compilation_id).slice(-10)} (not among the session's newest 50)` : 'no compilation recorded'}</div>}
          </Section>

          <Section title="What it said">
            <Said text={node.text} thinking={node.thinking} />
            {Array.isArray(d.tool_calls) && d.tool_calls.length > 0 && (
              <div className="mt-2 flex flex-col gap-1">
                <div className="text-[11px] text-ink-faint">and asked for</div>
                {d.tool_calls.map((t: D) => (
                  <button key={t.id} onClick={() => openCall(t.id)} className="flex items-center gap-2 rounded-md bg-tool/[0.05] px-2 py-1 text-left ring-1 ring-tool/15 hover:ring-tool/40">
                    <Wrench size={12} className="text-tool" />
                    <span className="num text-[12px] text-tool">{t.name}</span>
                    <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-ink-dim">{JSON.stringify(t.input)}</span>
                    <span className="text-[10.5px] text-live">inspect →</span>
                  </button>
                ))}
              </div>
            )}
          </Section>

          <Section title={`Its life as a kernel action${life.length ? ` · ${life.length} ledger rows` : ''}`}>
            {life.length ? <Life rows={life} /> : <div className="text-[12px] text-ink-faint">No ledger rows for its action in the newest 5,000.</div>}
          </Section>

          <Raw values={[node, pc as LedgerEntry | undefined]} label="the node and its provider.call row, raw" />
        </div>
      )}
    </Drawer>
  )
}

/** The call's clock: the request to the first byte, the first byte to the first token, and the stream. */
function Timing({ t }: { t: D }) {
  const total = Math.max(1, Number(t.total_ms))
  const fb = Math.min(total, Number(t.first_byte_ms ?? 0))
  const ft = Math.min(total, Math.max(fb, Number(t.first_token_ms ?? fb)))
  const parts = [
    { name: 'to the first byte', v: fb, tone: toneHex.idle },
    { name: 'first byte → first token', v: ft - fb, tone: toneHex.think },
    { name: 'streaming', v: total - ft, tone: toneHex.model },
  ]
  return (
    <div>
      <div className="flex h-3 w-full overflow-hidden rounded-full bg-white/[0.04] ring-1 ring-line">
        {parts.map((x) => <div key={x.name} title={`${x.name}: ${ms(x.v)}`} style={{ width: `${Math.max(0.8, (x.v / total) * 100)}%`, background: x.tone, opacity: 0.85 }} />)}
      </div>
      <div className="mt-1 flex flex-wrap gap-x-3 text-[11px]">
        {parts.map((x) => (
          <span key={x.name} className="flex items-center gap-1">
            <span className="inline-block h-2 w-2 rounded-sm" style={{ background: x.tone }} />
            <span className="text-ink-dim">{x.name}</span><span className="num text-ink">{ms(x.v)}</span>
          </span>
        ))}
      </div>
    </div>
  )
}

/** Input as it was billed: new, read from the cache, written to it; then output. And what the cache saved. */
function TokenBar({ u, price }: { u: D; price?: D }) {
  const parts = [
    { name: 'input, new', v: Number(u.input_tokens ?? 0), tone: toneHex.live },
    { name: 'cache read', v: Number(u.cache_read_input_tokens ?? 0), tone: toneHex.think },
    { name: 'cache write', v: Number(u.cache_creation_input_tokens ?? 0), tone: toneHex.wait },
    { name: 'output', v: Number(u.output_tokens ?? 0), tone: toneHex.model },
  ]
  const all = Math.max(1, parts.reduce((a, x) => a + x.v, 0))
  const input = parts[0].v + parts[1].v + parts[2].v
  const hit = input ? parts[1].v / input : 0
  const saved = price ? (parts[1].v * (Number(price.input_per_mtok) - Number(price.cache_read_per_mtok))) / 1e6 : undefined
  return (
    <div>
      <div className="flex h-3 w-full overflow-hidden rounded-full bg-white/[0.04] ring-1 ring-line">
        {parts.map((x) => x.v > 0 && <div key={x.name} title={`${x.name}: ${x.v.toLocaleString()}`} style={{ width: `${Math.max(0.8, (x.v / all) * 100)}%`, background: x.tone, opacity: 0.85 }} />)}
      </div>
      <div className="mt-1 flex flex-wrap gap-x-3 text-[11px]">
        {parts.map((x) => (
          <span key={x.name} className="flex items-center gap-1">
            <span className="inline-block h-2 w-2 rounded-sm" style={{ background: x.tone }} />
            <span className="text-ink-dim">{x.name}</span><span className="num text-ink">{x.v.toLocaleString()}</span>
          </span>
        ))}
      </div>
      <div className="mt-1.5 text-[12px] text-ink-dim">
        {input ? <><span className="num text-think">{(hit * 100).toFixed(1)}%</span> of its {tokens(input)} input tokens came from the cache</> : 'no input tokens on record'}
        {saved !== undefined && saved > 0 && <>, which saved <span className="num text-money">{usd(saved)}</span> against reading them new</>}.
      </div>
    </div>
  )
}

/** The charge, recomputed from the catalog's prices beside what was recorded. */
function Cost({ u, price, charged }: { u: D; price: D; charged?: number }) {
  const lines = [
    { name: 'input, new', n: Number(u.input_tokens ?? 0), per: Number(price.input_per_mtok) },
    { name: 'cache read', n: Number(u.cache_read_input_tokens ?? 0), per: Number(price.cache_read_per_mtok) },
    { name: 'cache write', n: Number(u.cache_creation_input_tokens ?? 0), per: Number(price.cache_write_per_mtok) },
    { name: 'output', n: Number(u.output_tokens ?? 0), per: Number(price.output_per_mtok) },
  ]
  const sum = lines.reduce((a, l) => a + (l.n * l.per) / 1e6, 0)
  const off = charged !== undefined && Math.abs(sum - charged) > 1e-6
  return (
    <table className="w-full text-[12px]">
      <tbody>
        {lines.map((l) => (
          <tr key={l.name} className="border-b border-line/40">
            <td className="py-0.5 text-ink-faint">{l.name}</td>
            <td className="num py-0.5 text-right text-ink-dim">{l.n.toLocaleString()}</td>
            <td className="num py-0.5 text-right text-ink-faint">× ${l.per}/M</td>
            <td className="num py-0.5 text-right text-money">{usd((l.n * l.per) / 1e6)}</td>
          </tr>
        ))}
        <tr>
          <td className="pt-1 text-ink-faint">total</td><td /><td />
          <td className={cn('num pt-1 text-right', off ? 'text-wait' : 'text-money')} title={off ? `recorded ${usd(charged!)}` : 'matches what was recorded'}>{usd(sum)}{off && ` (recorded ${usd(charged!)})`}</td>
        </tr>
      </tbody>
    </table>
  )
}

function Headroom({ rl }: { rl: D }) {
  const row = (name: string, left: unknown, limit: unknown, reset?: unknown) => {
    const l = Number(left), m = Number(limit)
    if (!Number.isFinite(l)) return null
    const frac = Number.isFinite(m) && m > 0 ? l / m : undefined
    return (
      <div key={name} className="grid grid-cols-[9rem_1fr_auto] items-center gap-2 text-[12px]">
        <span className="text-ink-faint">{name}</span>
        <div className="h-1.5 overflow-hidden rounded-full bg-white/[0.05]">
          {frac !== undefined && <div style={{ width: `${frac * 100}%`, background: frac > 0.2 ? toneHex.ok : toneHex.fault }} className="h-full" />}
        </div>
        <span className="num text-ink-dim">{tokens(l)}{Number.isFinite(m) ? ` / ${tokens(m)}` : ''}{reset ? ` · resets ${new Date(String(reset)).toLocaleTimeString([], { hour12: false })}` : ''}</span>
      </div>
    )
  }
  return (
    <div className="flex flex-col gap-1">
      {row('requests', rl.requests_remaining, rl.requests_limit, rl.requests_reset)}
      {row('tokens', rl.tokens_remaining, rl.tokens_limit, rl.tokens_reset)}
      {row('input tokens', rl.input_tokens_remaining, undefined)}
      {row('output tokens', rl.output_tokens_remaining, undefined)}
      {rl.retry_after_secs != null && <div className="text-[12px] text-wait">retry after {rl.retry_after_secs} s</div>}
    </div>
  )
}

function Said({ text, thinking }: { text: string; thinking?: string }) {
  const [think, setThink] = useState(false)
  return (
    <div>
      {thinking && (
        <button onClick={() => setThink((v) => !v)} className="mb-1 flex items-center gap-1 text-[11px] text-think">
          <ChevronRight size={12} className={cn('transition-transform', think && 'rotate-90')} /><Brain size={12} /> thinking · {thinking.length.toLocaleString()} chars
        </button>
      )}
      {think && thinking && <div className="mb-2 whitespace-pre-wrap rounded-md bg-think/5 px-2.5 py-1.5 text-[12px] italic text-ink-dim ring-1 ring-think/15">{thinking}</div>}
      {text ? <div className="md max-h-96 overflow-auto rounded-md bg-model/[0.04] px-3 py-2 text-[13px] text-ink ring-1 ring-model/15"><Markdown remarkPlugins={[remarkGfm]}>{text}</Markdown></div>
        : <div className="text-[12px] text-ink-faint">(no text: tool calls only)</div>}
    </div>
  )
}
