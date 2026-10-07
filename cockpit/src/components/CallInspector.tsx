// The call inspector (theseus-45n5): one tool call's whole story in one place. What the model asked, what the gate
// said, every step of its action's life with the time between steps, the job it ran, and what came back, gathered
// from the transcript and the ledger. It lives in the address (`?call=<tool_use_id or correlation id>` on a
// session's page), so a call can be linked to.
import { useEffect, useMemo, useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { motion } from 'motion/react'
import { ChevronRight, Copy, ExternalLink, ScanSearch, X } from 'lucide-react'
import type { LedgerEntry, NodeInfo, SessionHistory } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { useLedger } from '@/lib/derive'
import { summarize } from '@/lib/summary'
import { cn, ms, stamp } from '@/lib/format'
import { ledgerKind, type Tone } from '@/lib/taxonomy'

/** A tone as its CSS token: the Ship's night island and the day's paper each read their own (theseus-hnof.5). */
const toneVar = (t: Tone) => `var(--color-${t})`
import { JsonView } from './JsonView'
import { Pill } from './ui'

type D = Record<string, any>

/** The kinds that settle an action, and how each reads. */
const SETTLES: Record<string, Tone> = {
  'action.succeeded': 'ok', 'action.failed': 'fault', 'action.cancelled': 'fault', 'action.declined': 'fault',
  'action.expired': 'fault', 'action.outcome_unknown': 'wait',
}

interface Phase { name: string; from: number; to: number; tone: Tone; note?: string }

/** The phases of an action's life, from its rows: waiting for approval, queued, starting, running. */
function phases(rows: LedgerEntry[]): Phase[] {
  const at = (k: string) => rows.find((r) => r.kind === k)?.at_unix_ms
  const planned = at('action.planned')
  const asked = at('tool.confirm_requested')
  const answered = at('action.confirmed') ?? at('action.confirm_answered')
  const authorized = at('action.authorized')
  const dispatched = at('action.dispatched')
  const started = at('tool.job_started')
  const settle = rows.find((r) => r.kind in SETTLES)
  const out: Phase[] = []
  if (planned !== undefined && asked !== undefined) {
    out.push({ name: 'planned', from: planned, to: asked, tone: 'idle' })
  }
  if (asked !== undefined && (answered ?? authorized ?? settle?.at_unix_ms) !== undefined) {
    // The answer's row names the person (discord:zeroaltitude, the CLI); the confirm's own row may say only "operator".
    const ans = (rows.find((r) => r.kind === 'action.confirm_answered') ?? rows.find((r) => r.kind === 'action.confirmed'))?.data as D | undefined
    const verb = ans?.approved === false ? 'declined' : 'answered'
    out.push({ name: 'awaiting approval', from: asked, to: (answered ?? authorized ?? settle!.at_unix_ms), tone: 'wait', note: ans?.by ? `${verb} by ${ans.by}` : undefined })
  }
  const authFrom = answered ?? asked ?? planned
  if (authFrom !== undefined && dispatched !== undefined) out.push({ name: 'authorized → dispatched', from: authFrom, to: dispatched, tone: 'live' })
  if (dispatched !== undefined && started !== undefined) out.push({ name: 'starting', from: dispatched, to: started, tone: 'live' })
  const runFrom = started ?? dispatched
  if (runFrom !== undefined && settle) {
    const tone: Tone = SETTLES[settle.kind] === 'ok' ? 'tool' : SETTLES[settle.kind]
    // The settle row says how long the call itself ran; the rest, to the settle, is its result waiting to be
    // recorded (a late result, absorbed by the next turn).
    const ran = Number((settle.data as D | null)?.duration_ms)
    const runEnd = Number.isFinite(ran) && ran > 0 ? Math.min(settle.at_unix_ms, runFrom + ran) : settle.at_unix_ms
    out.push({ name: 'running', from: runFrom, to: runEnd, tone })
    if (settle.at_unix_ms - runEnd > 50) out.push({ name: 'until recorded', from: runEnd, to: settle.at_unix_ms, tone: 'idle' })
  }
  return out.filter((p) => p.to >= p.from)
}

/** The gate's word, toned: allowed, waiting on someone, or refused. */
function gateTone(gate: string | undefined): Tone {
  if (!gate) return 'idle'
  if (gate === 'deny' || gate.includes('deny') || gate.includes('refus')) return 'fault'
  if (gate.includes('confirm') || gate.includes('ask') || gate.includes('wait')) return 'wait'
  return 'ok'
}

export function CallInspector({ sessionId }: { sessionId: string }) {
  const [params, setParams] = useSearchParams()
  const key = params.get('call')
  const close = () => setParams((p) => { p.delete('call'); return p }, { replace: true })
  const { data: hist } = useRpc<SessionHistory>('session.history', { session_id: sessionId }, 5000, { enabled: !!key })
  const { call, result } = useMemo(() => {
    const nodes = hist?.nodes ?? []
    const match = (n: NodeInfo) => { const d = (n.detail ?? {}) as D; return d.tool_use_id === key || d.correlation_id === key }
    const c = nodes.find((n) => n.kind === 'tool_call' && match(n))
    const id = (c?.detail as D | undefined)?.tool_use_id
    const r = nodes.find((n) => n.kind === 'tool_result' && (match(n) || (!!id && (n.detail as D | null)?.tool_use_id === id)))
    return { call: c, result: r }
  }, [hist, key])
  const cd = (call?.detail ?? {}) as D
  const rd = (result?.detail ?? {}) as D
  const cid: string | undefined = cd.correlation_id ?? rd.correlation_id ?? (key?.startsWith('act_') ? key : undefined)
  const settled = !!result
  // The whole tail, filtered here by the action's id: an action's rows carry it, not always the session's.
  const { data: tail } = useLedger(5000, settled ? 0 : 3000, undefined, undefined, !!key)
  const rows = useMemo(
    () => (tail?.rows ?? []).filter((r) => cid && (r.data as D | null)?.correlation_id === cid).sort((a, b) => a.at_unix_ms - b.at_unix_ms),
    [tail, cid],
  )
  if (!key) return null
  const tool: string = cd.tool ?? rd.tool ?? 'tool'
  const gate: string | undefined = (cd.result as D | undefined)?.gate ?? cd.decision?.mode
  const status: string = rd.status ?? (result ? 'ok' : 'pending')
  const stoppedBy: string | undefined = rd.meta?.stopped_by
  const statusTone: Tone = stoppedBy ? 'wait' : rd.is_error || gate === 'deny' ? 'fault' : result ? 'ok' : 'wait'
  const job = rows.find((r) => r.kind === 'tool.job_started')?.data as D | undefined
  const jobOut = (rd.meta?.detail ?? rd.meta ?? {}) as D

  return (
    <Drawer onClose={close} head={<>
      <ScanSearch size={15} className="text-tool" />
      <span className="num text-[14px] font-semibold text-tool">{tool}</span>
      <Pill tone={statusTone}>{stoppedBy ? `⏹️ stopped by ${stoppedBy}` : gate === 'deny' ? 'denied' : status}</Pill>
      {rd.duration_ms != null && <span className="num text-[12px] text-ink-dim">{ms(rd.duration_ms)}</span>}
    </>}>
      {!call && !result ? (
        <div className="p-6 text-[12.5px] text-ink-faint">{hist ? `No tool call ${key} in this session.` : 'reading the session…'}</div>
      ) : (
        <div className="min-h-0 flex-1 overflow-auto px-4 py-3">
          <div className="mb-3 text-[12.5px] text-ink">{cd.plan?.summary ?? ''}</div>
          <div className="mb-3 flex flex-wrap gap-x-4 gap-y-1 text-[11px] text-ink-faint">
            {cid && <Ident label="action" value={cid} />}
            {(cd.tool_use_id ?? rd.tool_use_id) && <Ident label="tool use" value={cd.tool_use_id ?? rd.tool_use_id} />}
            {call?.at_unix_ms && <span className="num">asked {stamp(call.at_unix_ms)}</span>}
            {call?.turn_id && <span className="num">turn …{call.turn_id.slice(-6)} · loop {call.loop_index ?? '?'}</span>}
            {cid && <Link to={`/ledger?q=${cid}`} className="flex items-center gap-1 text-live hover:underline"><ExternalLink size={11} />its ledger rows</Link>}
          </div>

          <Section title="What the model asked">
            {Array.isArray(cd.plan?.resources) && cd.plan.resources.length > 0 && (
              <div className="mb-2 flex flex-wrap gap-1">{cd.plan.resources.map((res: D, i: number) => <Pill key={i} tone="tool">{res.access} {res.path ?? res.host ?? JSON.stringify(res)}</Pill>)}</div>
            )}
            <JsonView value={cd.input ?? {}} maxHeight="220px" />
          </Section>

          <Section title="What the gate said">
            <div className="flex flex-wrap items-center gap-2 text-[12px]">
              <Pill tone={gateTone(gate)}>{gate ?? '—'}</Pill>
              {cd.decision?.posture && <span className="num text-ink-faint">posture {cd.decision.posture}</span>}
            </div>
            {(cd.decision?.reason ?? cd.result?.reason) && <div className="mt-1.5 text-[12px] text-ink-dim">{cd.decision?.reason ?? cd.result?.reason}</div>}
          </Section>

          <Section title={`Its life${rows.length ? ` · ${rows.length} ledger rows` : ''}`}>
            {rows.length ? <Life rows={rows} /> : (
              <div className="text-[12px] text-ink-faint">{cid ? 'No ledger rows for this action in the newest 5,000.' : 'No action was planned for it: the gate decided before the kernel was asked.'}</div>
            )}
          </Section>

          {(job || jobOut.exit_code !== undefined) && (
            <Section title="The job">
              <div className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-[12px]">
                {job?.pid && <><span className="text-ink-faint">pid</span><span className="num text-ink">{job.pid}</span></>}
                {job?.argv && <><span className="text-ink-faint">argv</span><span className="num break-all text-ink">{(job.argv as string[]).join(' ')}</span></>}
                {job?.cwd && <><span className="text-ink-faint">cwd</span><span className="num break-all text-ink-dim">{job.cwd}</span></>}
                {job?.timeout_secs && <><span className="text-ink-faint">timeout</span><span className="num text-ink-dim">{job.timeout_secs} s</span></>}
                {jobOut.exit_code !== undefined && <><span className="text-ink-faint">exit</span><span className={cn('num', jobOut.exit_code === 0 ? 'text-ok' : 'text-fault')}>{String(jobOut.exit_code)}{jobOut.signal ? ` · signal ${jobOut.signal}` : ''}</span></>}
                {jobOut.bytes !== undefined && <><span className="text-ink-faint">output</span><span className="num text-ink-dim">{Number(jobOut.bytes).toLocaleString()} bytes</span></>}
              </div>
            </Section>
          )}

          <Section title="What came back">
            {result ? <>
              <div className="mb-1.5 flex flex-wrap gap-1">
                <Pill tone={statusTone}>{status}</Pill>
                {rd.truncated && <Pill tone="wait">truncated</Pill>}
                {rd.late && <Pill tone="wait">late</Pill>}
                {rd.external && <Pill tone="wait">external text</Pill>}
                <span className="num ml-auto text-[11px] text-ink-faint">{(result.bytes ?? result.text?.length ?? 0).toLocaleString()} bytes</span>
              </div>
              <Copyable text={result.text ?? ''} />
              {rd.meta && Object.keys(rd.meta).length > 0 && <div className="mt-2"><JsonView value={rd.meta} maxHeight="200px" /></div>}
            </> : <div className="text-[12px] text-ink-faint">nothing yet</div>}
          </Section>

          <Raw values={[call, result]} />
        </div>
      )}
    </Drawer>
  )
}

/** The inspectors' shell: a drawer on the right, over the deck, closed with its button or Esc. */
export function Drawer({ head, onClose, children }: { head: React.ReactNode; onClose: () => void; children: React.ReactNode }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose() }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })
  return (
    <motion.aside
      initial={{ x: 40, opacity: 0 }} animate={{ x: 0, opacity: 1 }} transition={{ duration: 0.18 }}
      className="fixed bottom-0 right-0 top-12 z-40 flex w-[min(640px,48vw)] flex-col border-l border-line-strong bg-deck/95 shadow-[-24px_0_48px_-24px_rgba(0,0,0,0.9)] backdrop-blur"
    >
      <header className="flex items-center gap-2 border-b border-line px-4 py-2.5">
        {head}
        <button onClick={onClose} title="close (Esc)" className="ml-auto rounded p-1 text-ink-faint hover:bg-white/5 hover:text-ink"><X size={15} /></button>
      </header>
      {children}
    </motion.aside>
  )
}

export function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="mb-4">
      <div className="panel-title mb-1.5">{title}</div>
      {children}
    </section>
  )
}

export function Ident({ label, value }: { label: string; value: string }) {
  return (
    <button onClick={() => navigator.clipboard?.writeText(value)} title="copy" className="flex items-center gap-1 hover:text-ink">
      <span>{label}</span><span className="num text-ink-dim">…{value.slice(-8)}</span><Copy size={10} />
    </button>
  )
}

/** The phases as one bar, then every row with its time from the first. */
export function Life({ rows }: { rows: LedgerEntry[] }) {
  const ps = phases(rows)
  const t0 = rows[0].at_unix_ms
  const span = Math.max(1, rows[rows.length - 1].at_unix_ms - t0)
  const [open, setOpen] = useState<number | null>(null)
  return (
    <div>
      {ps.length > 0 && (
        <div className="mb-2">
          <div className="flex h-3 w-full overflow-hidden rounded-full bg-white/[0.04] ring-1 ring-line">
            {ps.map((p, i) => (
              <div key={i} title={`${p.name}: ${ms(p.to - p.from)}${p.note ? ` · ${p.note}` : ''}`}
                style={{ width: `${Math.max(1.5, ((p.to - p.from) / span) * 100)}%`, background: toneVar(p.tone), opacity: 0.85 }} />
            ))}
          </div>
          <div className="mt-1 flex flex-wrap gap-x-3 gap-y-0.5 text-[11px]">
            {ps.map((p, i) => (
              <span key={i} className="flex items-center gap-1">
                <span className="inline-block h-2 w-2 rounded-sm" style={{ background: toneVar(p.tone) }} />
                <span className="text-ink-dim">{p.name}</span>
                <span className="num text-ink">{ms(p.to - p.from)}</span>
                {p.note && <span className="text-ink-faint">· {p.note}</span>}
              </span>
            ))}
          </div>
        </div>
      )}
      <div className="flex flex-col">
        {rows.map((r, i) => {
          const k = ledgerKind(r.kind)
          return (
            <div key={r.position} className="border-b border-line/40">
              <button onClick={() => setOpen(open === i ? null : i)} className="flex w-full items-baseline gap-2 py-1 text-left hover:bg-white/[0.02]">
                <ChevronRight size={11} className={cn('shrink-0 self-center text-ink-faint transition-transform', open === i && 'rotate-90')} />
                <span className="num w-16 shrink-0 text-right text-[11px] text-ink-faint">+{ms(r.at_unix_ms - t0)}</span>
                <span className="num w-44 shrink-0 truncate text-[11.5px]" style={{ color: toneVar(k.tone) }}>{r.kind}</span>
                <span className="min-w-0 flex-1 truncate text-[11.5px] text-ink-dim">{summarize(r)}</span>
              </button>
              {open === i && <div className="pb-2 pl-6"><JsonView value={r.data} maxHeight="200px" /></div>}
            </div>
          )
        })}
      </div>
    </div>
  )
}

export function Copyable({ text }: { text: string }) {
  const [done, setDone] = useState(false)
  return (
    <div className="relative">
      <pre className="max-h-80 overflow-auto whitespace-pre-wrap rounded-md bg-black/30 p-2.5 pr-9 font-mono text-[11.5px] text-ink-dim ring-1 ring-line">{text || '(empty)'}</pre>
      <button onClick={() => { navigator.clipboard?.writeText(text); setDone(true); setTimeout(() => setDone(false), 1200) }}
        title="copy" className="absolute right-2 top-2 rounded p-1 text-ink-faint hover:bg-white/5 hover:text-ink">
        {done ? <span className="text-[10px] text-ok">copied</span> : <Copy size={12} />}
      </button>
    </div>
  )
}

export function Raw({ values, label = 'the nodes, raw' }: { values: unknown[]; label?: string }) {
  const [open, setOpen] = useState(false)
  return (
    <section className="mb-2">
      <button onClick={() => setOpen((v) => !v)} className="panel-title flex items-center gap-1">
        <ChevronRight size={11} className={cn('transition-transform', open && 'rotate-90')} />{label}
      </button>
      {open && <div className="mt-1.5 flex flex-col gap-2">
        {values.filter((v) => v !== undefined && v !== null).map((v, i) => <JsonView key={i} value={v} maxHeight="260px" />)}
      </div>}
    </section>
  )
}
