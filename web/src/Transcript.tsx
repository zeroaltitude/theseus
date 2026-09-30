import { useState } from 'react'
import type { ConfirmRequest, NodeInfo, ProviderErrorData, Span, Tightening, TightenResult, TurnResult, Usage } from './protocol'
import TraceView from './TraceView'

// The transcript is rebuilt from the session's durable nodes (session.history):
// what you see is what a restarted daemon would render too. Live notifications
// only add what has not been written yet — streamed text and thinking for a
// loop whose assistant node does not exist, tools still running — and they
// disappear as soon as the node that holds them lands.

export interface LiveLoop { text: string; thinking: string }
export interface LiveTurn {
  turnId: string
  continuation: boolean
  startedAt: number
  loops: Record<number, LiveLoop>
  running: Record<string, { tool: string; startedAt: number; argv?: string[]; backend?: string }>
  compiles: Record<string, unknown>[]
}
export interface TurnError { message: string; data?: ProviderErrorData; class?: string | null }

export interface TranscriptProps {
  nodes: NodeInfo[]
  pending: ConfirmRequest[]
  live: Record<string, LiveTurn>
  results: Record<string, TurnResult>
  errors: Record<string, TurnError>
  traces: Record<string, Span | null>
  /// `trust` (theseus-9bp): approve, and trust the session again, so it no longer holds
  /// external text.
  onConfirm: (correlationId: string, approve: boolean, note: string, trust?: boolean) => Promise<void>
  onLoadTrace: (turnId: string) => void
  now: number
  /// Tool → its tightening ("should have asked", theseus-sgh), from health.
  tightened: Record<string, Tightening>
  onTighten: (tool: string, correlationId: string) => Promise<TightenResult>
}

const fmt = (n: number) => n.toLocaleString()
const money = (n: number | null | undefined) => n == null ? null : n < 0.01 ? `$${n.toFixed(4)}` : `$${n.toFixed(3)}`
const bytes = (b: number) => b >= 1 << 20 ? `${(b / (1 << 20)).toFixed(1)} MB` : b >= 1024 ? `${(b / 1024).toFixed(1)} KB` : `${b} B`
const clip = (s: string, n: number) => s.length > n ? `${s.slice(0, n)}…` : s
const str = (v: unknown) => typeof v === 'string' ? v : v == null ? '' : JSON.stringify(v)

function UsageBits({ u }: { u: Usage }) {
  const cache = u.cache_read_input_tokens + u.cache_creation_input_tokens
  return <span>in <b>{fmt(u.input_tokens)}</b> · out <b>{fmt(u.output_tokens)}</b>{cache > 0 && <> · cache r{fmt(u.cache_read_input_tokens)} w{fmt(u.cache_creation_input_tokens)}</>}</span>
}

/// One line saying what a tool call does, in the terms of that tool.
function callSummary(tool: string, input: unknown): string {
  const i = (input ?? {}) as Record<string, unknown>
  const s = (k: string) => str(i[k])
  switch (tool) {
    case 'proc.run': return `${((i.argv as string[] | undefined) ?? []).join(' ')}${i.cwd ? `   (in ${s('cwd')})` : ''}`
    case 'fs.read': return `${s('path')}${i.offset ? ` from line ${s('offset')}` : ''}${i.limit ? ` (${s('limit')} lines)` : ''}`
    case 'fs.write': return `${s('path')} (${bytes(s('content').length)})`
    case 'fs.edit': return s('path')
    case 'fs.patch': return `patch of ${s('patch').split('\n').length} lines`
    case 'fs.glob': return `${s('pattern')}${i.path ? ` in ${s('path')}` : ''}`
    case 'fs.grep': return `/${s('pattern')}/${i.path ? ` in ${s('path')}` : ''}${i.glob ? ` (${s('glob')})` : ''}`
    case 'fs.list': return s('path') || '.'
    case 'git.diff': return `${s('repo') || s('path') || '.'}${i.rev ? ` ${s('rev')}` : ''}`
    case 'git.log': return `${s('repo') || s('path') || '.'}${i.n ? ` (${s('n')})` : ''}`
    case 'text.diff': return 'two texts'
    case 'http.fetch': return `${s('url')}${i.max_bytes ? ` (at most ${bytes(Number(i.max_bytes))})` : ''}`
    case 'web.search': return `"${s('query')}"${i.count ? ` (${s('count')} results)` : ''}`
    default: return clip(JSON.stringify(input), 120)
  }
}

/// `fs_read` → `fs.read` (the provider's tool-name alphabet has no dots).
const wireToName = (w: string) => w.replace('_', '.')

const looksLikeDiff = (t: string) => /(^|\n)@@ .* @@/.test(t) || /(^|\n)--- .*\n\+\+\+ /.test(t)

function Content({ text }: { text: string }) {
  if (!looksLikeDiff(text)) return <pre className="tool-out">{text}</pre>
  return (
    <pre className="tool-out diff">
      {text.split('\n').map((l, i) => {
        const c = l.startsWith('+++') || l.startsWith('---') ? 'meta' : l.startsWith('@@') ? 'hunk' : l.startsWith('+') ? 'add' : l.startsWith('-') ? 'del' : ''
        return <span key={i} className={c}>{l}{'\n'}</span>
      })}
    </pre>
  )
}

/// What an edit would do, as a diff, before the operator approves it.
function Preview({ tool, input }: { tool: string; input: unknown }) {
  const i = (input ?? {}) as Record<string, unknown>
  if (tool === 'fs.edit' && typeof i.old_string === 'string' && typeof i.new_string === 'string') {
    const d = [`--- ${str(i.path)}`, `+++ ${str(i.path)}`, '@@ edit @@',
      ...i.old_string.split('\n').map((l) => `-${l}`), ...i.new_string.split('\n').map((l) => `+${l}`)].join('\n')
    return <Content text={d} />
  }
  if (tool === 'fs.write' && typeof i.content === 'string') {
    return <><div className="muted small">{str(i.path)} · {bytes(i.content.length)}</div><pre className="tool-out">{clip(i.content, 4000)}</pre></>
  }
  if (tool === 'fs.patch' && typeof i.patch === 'string') return <Content text={i.patch} />
  if (tool === 'proc.run') {
    return <pre className="tool-out">$ {((i.argv as string[] | undefined) ?? []).map((a) => /\s/.test(a) ? `'${a}'` : a).join(' ')}{i.cwd ? `\n  (in ${str(i.cwd)})` : ''}{i.timeout_secs ? `\n  (timeout ${str(i.timeout_secs)} s)` : ''}</pre>
  }
  return <pre className="tool-out">{JSON.stringify(input, null, 2)}</pre>
}

function ConfirmCard({ c, onConfirm, now }: { c: ConfirmRequest; onConfirm: TranscriptProps['onConfirm']; now: number }) {
  const [note, setNote] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const left = Math.max(0, Math.round((c.expires_at_ms - now) / 1000))
  const answer = async (approve: boolean, trust = false) => {
    setBusy(true); setErr(null)
    try { await onConfirm(c.correlation_id, approve, note, trust) } catch (e) { setErr((e as { message?: string }).message ?? String(e)); setBusy(false) }
  }
  // It waits because its session read external text (theseus-9bp).
  const ext = c.external_text
  return (
    <div className={`confirm ${c.floor ? 'floor' : ''}`}>
      <div className="confirm-head"><b>{c.tool}</b> {c.floor
        ? <><span className="pill bad">floor</span> touches Theseus's own state or secrets; it always asks</>
        : ext ? <><span className="pill warn">external text</span> this session read {ext.tool} {clip(ext.url, 80)}, so a call that acts waits</>
        : 'needs your confirmation'}</div>
      <div className="muted small">{c.reason}</div>
      <Preview tool={c.tool} input={c.input} />
      <div className="confirm-actions">
        <input value={note} onChange={(e) => setNote(e.target.value)} placeholder="note (optional; the model sees it on a decline)" disabled={busy} />
        <button type="button" className="approve" disabled={busy} onClick={() => void answer(true)}>Approve</button>
        {ext && <button type="button" className="approve" disabled={busy} onClick={() => void answer(true, true)}
          title="Approve this call, and trust the session again: its later calls that act run at their postures, until it reads external text again">Approve + trust session</button>}
        <button type="button" className="decline" disabled={busy} onClick={() => void answer(false)}>Decline</button>
      </div>
      <div className="muted small">
        {busy ? 'answered; the turn resumes on its own…' : `expires in ${left >= 3600 ? `${Math.round(left / 3600)} h` : left >= 60 ? `${Math.round(left / 60)} min` : `${left} s`} · bound to exactly these arguments · ${c.correlation_id}`}
      </div>
      {err && <div className="warn small">{err}</div>}
    </div>
  )
}

/// A session at its spend limit asks whether its spend may go back to $0
/// (theseus-0sg). No tool call stands behind it: the turn stopped before its
/// model call, so the card sits after the last turn.
function BudgetCard({ c, onConfirm }: { c: ConfirmRequest; onConfirm: TranscriptProps['onConfirm'] }) {
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const b = c.budget
  const answer = async (approve: boolean) => {
    setBusy(true); setErr(null)
    try { await onConfirm(c.correlation_id, approve, '') } catch (e) { setErr((e as { message?: string }).message ?? String(e)); setBusy(false) }
  }
  return (
    <div className="confirm budget-ask">
      <div className="confirm-head"><b>Budget</b> this session reached its spend limit</div>
      <div>{c.reason}</div>
      {b && <div className="muted small">
        Approve resets its spend to $0, and the waiting call goes on (it reserves {money(b.needed_usd)}). The session's
        lifetime cost, {money(b.lifetime_usd)}, keeps counting. Decline, or send a new message, and it keeps waiting.
      </div>}
      <div className="confirm-actions">
        <button type="button" className="approve" disabled={busy} onClick={() => void answer(true)}>Reset to $0 and continue</button>
        <button type="button" className="decline" disabled={busy} onClick={() => void answer(false)}>Keep waiting</button>
      </div>
      <div className="muted small">{busy ? 'answered…' : c.correlation_id}</div>
      {err && <div className="warn small">{err}</div>}
    </div>
  )
}

// A call that never ran; rows from before theseus-8az say `denied`.
const NOT_RUN = ['declined', 'denied']
const STATUS_CLASS: Record<string, string> = { ok: 'ok', error: 'bad', declined: 'warn', denied: 'warn', background: 'accent', unknown: 'warn', cancelled: 'muted' }
const GATE_CLASS: Record<string, string> = { allow: 'ok', open: 'ok', notify: 'warn', confirm: 'accent', approve: 'accent', deny: 'bad' }

function ResultLine({ r, open }: { r: NodeInfo; open: boolean }) {
  const d = (r.detail ?? {}) as Record<string, unknown>
  const status = str(d.status)
  const exit = (d.meta as Record<string, unknown> | undefined)?.exit_code
  return (
    <>
      <div className="result-line">
        <span className={`pill ${STATUS_CLASS[status] ?? ''}`}>{NOT_RUN.includes(status) ? 'not run' : status}</span>
        {d.late === true && <span className="pill accent" title="arrived after the turn that asked for it">late</span>}
        {exit != null && <span className={exit === 0 ? 'muted' : 'bad'}>exit {str(exit)}</span>}
        {d.duration_ms != null && <span className="muted">{fmt(Number(d.duration_ms))} ms</span>}
        <span className="muted">{bytes(r.bytes)}</span>
        {d.truncated === true && <span className="warn" title={d.full_ref ? `full output: ${str(d.full_ref)}` : ''}>truncated</span>}
        {!open && <span className="muted preview">{clip(r.text.replace(/\s+/g, ' '), 140)}</span>}
      </div>
      {open && <Content text={r.text} />}
    </>
  )
}

/// "Should have asked" on a notice (theseus-sgh): one press makes the tool ask first from
/// now on, on every surface. Once it does, the notice says so instead.
function ShouldHaveAsked({ tool, corr, tightened, onTighten }: {
  tool: string; corr: string; tightened: Tightening | undefined; onTighten: TranscriptProps['onTighten']
}) {
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  if (tightened) {
    return <span className="muted small" title={`tightened by ${tightened.by} · undo it in the Observatory's Tools view`}>asks first now</span>
  }
  const press = (e: React.MouseEvent) => {
    e.stopPropagation()
    setBusy(true); setErr(null)
    onTighten(tool, corr).catch((x: { message?: string }) => setErr(x.message ?? String(x))).finally(() => setBusy(false))
  }
  return (
    <>
      <button type="button" className="link small" disabled={busy} onClick={press}
        title={`${tool} asks first from now on, on every surface. It only tightens; undo it in the Observatory's Tools view.`}>
        should have asked
      </button>
      {err && <span className="warn small">{err}</span>}
    </>
  )
}

function ToolCard({ call, use, results, confirm, running, onConfirm, now, tightened, onTighten }: {
  call: NodeInfo | null
  use: { id: string; name: string; input: unknown }
  results: NodeInfo[]
  confirm: ConfirmRequest | undefined
  running: LiveTurn['running'][string] | undefined
  onConfirm: TranscriptProps['onConfirm']
  now: number
  tightened: TranscriptProps['tightened']
  onTighten: TranscriptProps['onTighten']
}) {
  const [open, setOpen] = useState(false)
  const d = (call?.detail ?? {}) as Record<string, unknown>
  const tool = str(d.tool) || (results[0] ? str((results[0].detail ?? {}).tool) : wireToName(use.name))
  const decision = d.decision as { mode?: string; posture?: string; reason?: string; granted?: string; notify?: { kind: string; setting: string; rule: string } } | null | undefined
  const gate = decision?.posture ?? decision?.mode ?? (d.result as { gate?: string } | undefined)?.gate
  const notice = decision?.notify
  const input = call ? d.input : use.input
  return (
    <div className={`tool ${confirm ? 'awaiting' : ''}`}>
      <div className="tool-head" onClick={() => setOpen((o) => !o)} title="click for the full input and output">
        <span className="chev">{open ? '▾' : '▸'}</span>
        <code className="tool-name">{tool}</code>
        <span className="tool-sum">{callSummary(tool, input)}</span>
        {gate && <span className={`pill ${GATE_CLASS[gate] ?? ''}`} title={decision?.reason ?? ''}>{gate}</span>}
        {notice && gate !== 'notify' && <span className="pill warn" title={`${notice.setting}\n${notice.rule}`}>notified</span>}
        {decision?.granted && <span className="pill" title="the secret broker (names only)">🔑 {decision.granted}</span>}
        {notice && <ShouldHaveAsked tool={tool} corr={str(d.correlation_id)} tightened={tightened[tool]} onTighten={onTighten} />}
        {running && results.length === 0 && <span className="accent small">running {Math.max(0, Math.round((now - running.startedAt) / 1000))} s…</span>}
      </div>
      {open && (
        <div className="tool-body">
          <div className="muted small">input{decision?.reason ? ` · policy: ${decision.reason}` : ''}</div>
          <pre className="tool-out">{JSON.stringify(input, null, 2)}</pre>
        </div>
      )}
      {confirm && <ConfirmCard c={confirm} onConfirm={onConfirm} now={now} />}
      {results.map((r) => <ResultLine key={r.node_id} r={r} open={open} />)}
    </div>
  )
}

interface Turn { id: string; nodes: NodeInfo[] }

function groupTurns(nodes: NodeInfo[]): Turn[] {
  const out: Turn[] = []
  const at = new Map<string, Turn>()
  for (const n of nodes) {
    const id = n.turn_id ?? `n:${n.node_id}`
    let t = at.get(id)
    if (!t) { t = { id, nodes: [] }; at.set(id, t); out.push(t) }
    t.nodes.push(n)
  }
  return out
}

export default function Transcript(p: TranscriptProps) {
  const turns = groupTurns(p.nodes)
  // Results pair with their call by tool_use_id across turns (a late result
  // lands in a later turn but belongs on the card of the call that started it).
  const resultsByUse = new Map<string, NodeInfo[]>()
  const callsByUse = new Map<string, NodeInfo>()
  for (const n of p.nodes) {
    const d = (n.detail ?? {}) as Record<string, unknown>
    if (n.kind === 'tool_result') resultsByUse.set(str(d.tool_use_id), [...(resultsByUse.get(str(d.tool_use_id)) ?? []), n])
    if (n.kind === 'tool_call') callsByUse.set(str(d.tool_use_id), n)
  }
  const confirmByCorr = new Map(p.pending.map((c) => [c.correlation_id, c]))
  const liveIds = Object.keys(p.live).filter((id) => !turns.some((t) => t.id === id))
  const budgetAsks = p.pending.filter((c) => c.budget)

  return (
    <>
      {turns.map((t) => <TurnView key={t.id} t={t} p={p} resultsByUse={resultsByUse} callsByUse={callsByUse} confirmByCorr={confirmByCorr} />)}
      {liveIds.map((id) => <TurnView key={id} t={{ id, nodes: [] }} p={p} resultsByUse={resultsByUse} callsByUse={callsByUse} confirmByCorr={confirmByCorr} />)}
      {budgetAsks.map((c) => <BudgetCard key={c.correlation_id} c={c} onConfirm={p.onConfirm} />)}
    </>
  )
}

function TurnView({ t, p, resultsByUse, callsByUse, confirmByCorr }: {
  t: Turn; p: TranscriptProps
  resultsByUse: Map<string, NodeInfo[]>; callsByUse: Map<string, NodeInfo>; confirmByCorr: Map<string, ConfirmRequest>
}) {
  const [showTrace, setShowTrace] = useState(false)
  const user = t.nodes.find((n) => n.kind === 'user_message')
  const assistants = t.nodes.filter((n) => n.kind === 'assistant_message')
  const live = p.live[t.id]
  const result = p.results[t.id]
  const error = p.errors[t.id]
  const loopsWritten = new Set(assistants.map((a) => a.loop_index ?? 0))
  const liveLoops = live ? Object.entries(live.loops).filter(([l, b]) => !loopsWritten.has(Number(l)) && (b.text || b.thinking)) : []
  const lateResults = t.nodes.filter((n) => n.kind === 'tool_result' && (n.detail ?? {}).late === true)

  // Footer numbers: the turn result when this tab saw the turn end, else the nodes.
  const usage: Usage = { input_tokens: 0, output_tokens: 0, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 }
  let cost: number | null = 0
  for (const a of assistants) {
    const d = (a.detail ?? {}) as { usage?: Usage; cost_usd?: number | null }
    if (d.usage) for (const k of Object.keys(usage) as (keyof Usage)[]) usage[k] += d.usage[k] ?? 0
    cost = d.cost_usd == null || cost == null ? null : cost + d.cost_usd
  }
  const last = assistants[assistants.length - 1]
  const lastD = (last?.detail ?? {}) as Record<string, unknown>
  const trace = result?.trace ?? p.traces[t.id] ?? null
  const running = live != null && !result && !error

  return (
    <section className="exchange">
      {user ? (
        <div className="prompt" title={`${user.author ?? 'operator'} · ${new Date(user.at_unix_ms).toLocaleString()}`}><pre>{user.text}</pre></div>
      ) : (
        <div className="turn-divider muted small">continuation{lateResults.length > 0 ? ` · ${lateResults.length} background result(s) arrived` : ' · resumed after a confirmation or a restart'}</div>
      )}
      <div className={`reply ${error ? 'failed' : ''}`}>
        {lateResults.map((r) => {
          const d = (r.detail ?? {}) as Record<string, unknown>
          const call = callsByUse.get(str(d.tool_use_id))
          return (
            <div key={r.node_id} className="tool late">
              <div className="tool-head"><span className="chev">↩</span><code className="tool-name">{str(d.tool)}</code>
                <span className="tool-sum">{call ? callSummary(str(d.tool), (call.detail ?? {}).input) : str(d.tool_use_id)}</span>
                <span className="muted small">background result</span></div>
              <ResultLine r={r} open={false} />
            </div>
          )
        })}
        {t.nodes.filter((n) => n.kind === 'assistant_message').map((a) => {
          const d = (a.detail ?? {}) as { tool_calls?: { id: string; name: string; input: unknown }[] }
          return (
            <div key={a.node_id} className="loop">
              {a.thinking && <details className="thinking"><summary className="muted small">thinking · {fmt(a.thinking.length)} chars</summary><pre>{a.thinking}</pre></details>}
              {a.text && <pre className="text">{a.text}</pre>}
              {(d.tool_calls ?? []).map((u) => {
                const call = callsByUse.get(u.id) ?? null
                const corr = call ? str((call.detail ?? {}).correlation_id) : ''
                const res = (resultsByUse.get(u.id) ?? [])
                if (!call && res.length === 0) {
                  return <div key={u.id} className="tool queued"><div className="tool-head"><span className="chev">·</span><code className="tool-name">{wireToName(u.name)}</code><span className="tool-sum">{callSummary(wireToName(u.name), u.input)}</span><span className="muted small">waits for the call before it</span></div></div>
                }
                return <ToolCard key={u.id} call={call} use={u} results={res} confirm={corr ? confirmByCorr.get(corr) : undefined}
                  running={live?.running[u.id]} onConfirm={p.onConfirm} now={p.now} tightened={p.tightened} onTighten={p.onTighten} />
              })}
            </div>
          )
        })}
        {liveLoops.map(([l, b]) => (
          <div key={`live-${l}`} className="loop live">
            {b.thinking && <details className="thinking" open><summary className="muted small">thinking…</summary><pre>{b.thinking}</pre></details>}
            {b.text && <pre className="text">{b.text}</pre>}
          </div>
        ))}
        {running && <span className="cursor">▍</span>}
        {error && (
          <div className="error">
            <strong>turn failed</strong>
            {(error.data?.class ?? error.class) && <> · class <code>{error.data?.class ?? error.class}</code></>}
            {error.data && <>{error.data.transient ? ' · transient' : ' · permanent'}{error.data.usage_unknown && ' · usage unknown (reservation held)'}</>}
            <div>{error.message}</div>
          </div>
        )}
        {(assistants.length > 0 || result) && (
          <footer>
            {result ? <>
              <span title="profile → provider/model">{result.profile} → {result.provider}/{result.model}</span>
              <span>{result.loops} loop{result.loops === 1 ? '' : 's'}</span>
              {(result.tool_calls ?? 0) > 0 && <span>{result.tool_calls} tool call{result.tool_calls === 1 ? '' : 's'}</span>}
              <span>{result.stop_reason}</span>
              <UsageBits u={result.usage} />
              {result.cost_usd != null && <span><b>{money(result.cost_usd)}</b></span>}
            </> : <>
              <span>{str(lastD.model)}</span>
              <span>{assistants.length} loop{assistants.length === 1 ? '' : 's'}</span>
              <span>{str(lastD.stop_reason)}</span>
              <UsageBits u={usage} />
              {cost != null && <span><b>{money(cost)}</b></span>}
            </>}
            {(live?.compiles ?? []).map((c, i) => (
              <span key={i} className={c.decision === 'recompile' ? 'accent' : 'muted'} title={`compilation ${str(c.compilation_id)}\nprefix ${str(c.prefix_nodes)} + tail ${str(c.tail_nodes)} nodes\ndigest ${str(c.digest)}`}>
                context {str(c.decision)}{c.trigger ? ` (${str(c.trigger)})` : ''} · {str(c.messages)} msg · ~{fmt(Number(c.est_tokens ?? 0))} tok
              </span>
            ))}
            {!t.id.startsWith('n:') && (
              <button type="button" className="link" title="the turn's full timing tree" onClick={() => {
                if (!trace) p.onLoadTrace(t.id)
                setShowTrace((v) => !v)
              }}>
                {result ? <>{fmt(result.elapsed_ms)} ms{result.first_token_ms != null && <> · first token {fmt(result.first_token_ms)} ms</>} </> : 'timing '}▾
              </button>
            )}
          </footer>
        )}
        {showTrace && (trace ? <TraceView root={trace} /> : <div className="muted small">no trace recorded for this turn</div>)}
      </div>
    </section>
  )
}
