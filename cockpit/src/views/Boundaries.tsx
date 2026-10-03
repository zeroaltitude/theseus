// The boundaries board (theseus-logs, round two): every line Theseus draws around its own work, as it stands, live.
//
// - The latch (T1): sessions holding text from outside, what they read and when, and the trust that lets them go.
// - The gate: approvals waiting, and the tightenings ("should have asked"), each with its undo.
// - The sandbox (17b): each L1 job live, with its command and how long it has run (`sandbox.usage`; an L1 job has no
//   cgroup to gauge, theseus-gyin); each finished one with what it left in scratch; each cancelled one with 18a's verdict.
// - The broker: which program or tool is handed which secret, by name only; and what it withheld.
// - Places (the place rule): each place and its class, a private channel's start-time read, and the public trees.
// - Egress (18c, a seam): the hosts sandboxed jobs reach, and the refusals, once 18c records them.
import { useDeferredValue, useMemo, type ReactNode } from 'react'
import { useNavigate } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { Anchor, CircleCheck, Eye, KeyRound, Link2, Lock, OctagonX, Radar, ShieldCheck, ShieldHalf, Siren, Undo2 } from 'lucide-react'
import type { ActionInfo, ConfirmRequest, Health, LedgerEntry, NodeInfo, SandboxUsage, SessionInfo } from '@protocol'
import { call, useRpc } from '@/lib/rpc'
import { useTick } from '@/lib/hooks'
import { useHistoryRows } from '@/lib/history'
import { useWorld } from '@/lib/world'
import { verdictWords } from '@/lib/verdict'
import { ago, bytes, clock, cn, ms, short, stamp } from '@/lib/format'
import { Btn, Empty, Panel, Pill } from '@/components/ui'

type D = Record<string, any>

const KINDS = new Set([
  'session.external_read', 'session.trusted', 'policy.tightened', 'policy.untightened', 'secret.granted', 'secret.withheld',
  'place.viewed', 'place.published', 'action.cancel_verified', 'action.cancel_uncertain', 'action.cancel_unsupported',
  'tool.job_started', 'sandbox.started', 'action.succeeded', 'action.failed', 'action.cancelled',
  // 18c's egress.
  'sandbox.egress', 'sandbox.egress_refused',
])

export default function Boundaries() {
  const nav = useNavigate()
  const tick = useTick(1000)
  const { data: h } = useRpc<Health>('health', undefined, 2000)
  const { data: cl } = useRpc<{ confirms: ConfirmRequest[] }>('confirm.list', undefined, 2000)
  const { data: sl } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 5000)
  const { rows } = useHistoryRows()
  // The time machine's moment (null while live): the holds, questions, tightenings, and verdicts are the fold's, and
  // the log's rows stop at the moment. The live L1 jobs, the broker's grants, and the labels' and egress lists are
  // read from the daemon as it is now, and say so.
  const world = useDeferredValue(useWorld())
  const asOf = world?.t ?? null
  const now = asOf ?? tick
  const mine = useMemo(() => rows.filter((r) => KINDS.has(r.kind) && (asOf === null || r.at_unix_ms <= asOf)), [rows, asOf])
  // L1 jobs live, read each second while any runs, else every five.
  const { data: usage } = useRpc<SandboxUsage>('sandbox.usage', undefined, 5000, {
    refetchInterval: (q) => (q.state.data?.running?.length ? 1000 : 5000),
  })
  const { data: resultList } = useRpc<{ nodes: NodeInfo[] }>('node.list', { session_id: null, kind: 'tool_result', n: 2000 }, 10_000)
  const results = useMemo(() => (resultList?.nodes ?? []).filter((n) => asOf === null || n.at_unix_ms <= asOf), [resultList, asOf])
  // A job's command rides its turn's next frame (`tool.job_started`): while it runs, `sandbox.usage`'s `running` names
  // it (theseus-kpz1), and its action its tool and when it was dispatched.
  const { data: al } = useRpc<{ actions: ActionInfo[] }>('action.list', { n: 200 }, 3000)
  const title = useMemo(() => {
    const m = new Map((sl?.sessions ?? []).map((s) => [s.session_id, s.title || s.label || short(s.session_id)]))
    return (sid: string | null | undefined) => (sid ? m.get(sid) ?? short(sid) : '—')
  }, [sl])

  const holds = world ? world.holds : h?.external_text ?? []
  const confirms = world ? world.confirms : cl?.confirms ?? []
  const tight = world ? world.tightenings : h?.tightenings ?? []
  // The live list can't be read back: in the past, the L1 jobs running then are counted from the fold.
  const live = useMemo(() => (world ? [] : usage?.running ?? []), [world, usage])
  const l1Then = useMemo(() => (world ? [...world.jobsRunning].filter((id) => world.l1.has(id)).length : 0), [world])
  const placeRows = mine.filter((r) => r.kind === 'place.viewed' || r.kind === 'place.published')
  const shared = (h?.places?.places ?? []).filter((p) => p.class === 'shared').length
  const secretsWithheld = mine.filter((r) => r.kind === 'secret.withheld').length
  const egress = mine.filter((r) => r.kind === 'sandbox.egress' || r.kind === 'sandbox.egress_refused')
  const reachedOut = new Set(egress.filter((r) => r.kind === 'sandbox.egress').map((r) => String((r.data as D)?.correlation_id))).size

  return (
    <div className="flex flex-col gap-3">
      <div className="panel flex flex-wrap items-center gap-x-6 gap-y-3 px-4 py-3">
        <div className="mr-2">
          <h1 className="ship-title !text-[26px]">The boundaries</h1>
          <div className="text-[11.5px] text-ink-dim">
            {world ? `as it stood at ${clock(world.t)} · the live jobs, grants, and host lists are the daemon's now` : 'every line Theseus draws around its own work, as it stands'}
          </div>
        </div>
        <Seal icon={<Link2 size={15} />} n={holds.length} word="chained" tone="#f472b6" hint="sessions holding outside text: their calls that act wait for you" />
        <Seal icon={<ShieldCheck size={15} />} n={confirms.length} word="asking" tone="#fbbf24" hint="approvals waiting for you" />
        <Seal icon={<Lock size={15} />} n={tight.length} word="tightened" tone="#fbbf24" hint="tools that ask first because someone pressed “Make actions like this ask in the future”" />
        <Seal icon={<ShieldHalf size={15} />} n={world ? l1Then : live.length} word={world ? 'in L1 then' : 'in L1 now'} tone="#5eead4" hint="sandboxed jobs running" />
        <Seal icon={<KeyRound size={15} />} n={h?.broker.length ?? 0} word="grants" tone="#d6a548" hint="the broker's grants: who is handed which secret (names only)" />
        <Seal icon={<Eye size={15} />} n={shared} word="shared places" tone="#22d3ee" hint="guild places others read: the public tools alone, and only the context files marked public" />
        <Seal icon={<Radar size={15} />} n={reachedOut} word="reached out" tone="#5eead4" hint="sandboxed jobs that connected out through their egress list (18c)" />
      </div>

      <div className="grid grid-cols-1 gap-3 2xl:grid-cols-2">
        <Panel title={<>The latch · sessions holding outside text · {holds.length}</>} icon={<Siren size={13} />} bodyClassName="p-2.5">
          <Latch holds={holds} rows={mine} title={title} now={now} onOpen={(sid) => nav(`/session/${sid}`)} />
        </Panel>
        <Panel title={<>The gate · approvals and tightenings</>} icon={<ShieldCheck size={13} />} bodyClassName="p-2.5">
          <Gate confirms={confirms} tight={tight} rows={mine} title={title} now={now} />
        </Panel>
      </div>

      <Panel title={<>The sandbox · L1 jobs, live and finished</>} icon={<ShieldHalf size={13} />} bodyClassName="p-2.5"
        actions={<span className="num text-[11px] text-ink-faint">{sandboxLine(h)}</span>}>
        <Sandbox usage={usage} rows={mine} results={results} actions={world ? world.actions : al?.actions ?? []} asOf={asOf} title={title} now={now} onOpen={(sid, cid) => nav(`/session/${sid}${cid ? `?call=${cid}` : ''}`)} />
      </Panel>

      <div className="grid grid-cols-1 gap-3 xl:grid-cols-3">
        <Panel title={<>The broker · grants and withholdings</>} icon={<KeyRound size={13} />} bodyClassName="p-2.5">
          <Broker health={h} rows={mine} title={title} withheld={secretsWithheld} />
        </Panel>
        <Panel title={<>Places · who reads where</>} icon={<Eye size={13} />} bodyClassName="p-2.5">
          <Places health={h} log={placeRows} />
        </Panel>
        <Panel title={<>Egress · where sandboxed jobs reach</>} icon={<Radar size={13} />} bodyClassName="p-2.5">
          <Egress rows={egress} health={h} title={title} now={now} />
        </Panel>
      </div>
    </div>
  )
}

/** A brass seal lamp: a count lit in its tone, dark at zero. */
function Seal({ icon, n, word, tone, hint }: { icon: ReactNode; n: number; word: string; tone: string; hint: string }) {
  const lit = n > 0
  return (
    <div className="flex items-center gap-2.5" title={hint}>
      <span className="grid h-9 w-9 place-items-center rounded-full"
        style={{
          background: lit ? `radial-gradient(circle at 40% 35%, ${tone}55, #071322 70%)` : 'radial-gradient(circle at 40% 35%, #13243a, #050c16 70%)',
          boxShadow: `0 0 0 2px #8c6a3c, 0 0 0 3px #2c2010, inset 0 1px 0 rgba(243,217,164,0.25)${lit ? `, 0 0 14px ${tone}66` : ''}`,
          color: lit ? tone : '#6e6656',
        }}>{icon}</span>
      <div className="leading-tight">
        <div className="num text-[19px] font-semibold" style={{ color: lit ? tone : '#9c907a', textShadow: lit ? `0 0 10px ${tone}88` : undefined }}>{n}</div>
        <div className="ship-engraved text-[9.5px]">{word}</div>
      </div>
    </div>
  )
}

function useAct() {
  const qc = useQueryClient()
  return async (method: string, params: unknown, ask: string) => {
    if (!window.confirm(ask)) return
    try { await call(method, params); await qc.invalidateQueries() } catch (e: any) { window.alert(e?.message ?? String(e)) }
  }
}

// ---------------------------------------------------------------- the latch

function Latch({ holds, rows, title, now, onOpen }: { holds: Health['external_text']; rows: LedgerEntry[]; title: (s?: string | null) => string; now: number; onOpen: (sid: string) => void }) {
  const act = useAct()
  const log = rows.filter((r) => r.kind === 'session.external_read' || r.kind === 'session.trusted').slice(-8).reverse()
  return (
    <div className="flex flex-col gap-2">
      {!holds.length && <Empty><span className="flex items-center gap-2"><ShieldCheck size={15} className="text-ok" /> every session is trusted: nothing holds outside text</span></Empty>}
      {holds.map((x) => (
        <div key={x.session_id} className="flex items-start gap-3 rounded-lg bg-[#f472b6]/[0.05] px-3 py-2.5 ring-1 ring-[#f472b6]/25">
          <Link2 size={16} className="mt-0.5 shrink-0 text-[#f472b6]" />
          <div className="min-w-0 flex-1">
            <button type="button" onClick={() => onOpen(x.session_id)} className="truncate text-left text-[13px] font-medium text-ink hover:text-live">{x.title || title(x.session_id)}{x.task ? ` · task ${x.task}` : ''}</button>
            <div className="num mt-0.5 truncate text-[11.5px] text-ink-dim" title={x.held.url}>
              read <span className="text-tool">{x.held.tool}</span> {x.held.query ? `“${x.held.query}”` : x.held.url}
            </div>
            <div className="num text-[11px] text-ink-faint">
              since {x.since_local || clock(x.held.since_ms)} · {ago(x.held.since_ms, now)}
              {x.held.from_session && <> · from {title(x.held.from_session)}{x.held.via ? ` (${x.held.via})` : ''}</>}
            </div>
          </div>
          <Btn tone="wait" onClick={() => act('policy.trust', { session_id: x.session_id }, `Trust ${x.title || title(x.session_id)} again? Its calls that act stop waiting.`)}><ShieldCheck size={13} /> Trust</Btn>
        </div>
      ))}
      {!!log.length && (
        <div className="mt-1">
          <div className="ship-engraved mb-1 text-[9.5px]">The latch&rsquo;s log</div>
          {log.map((r) => {
            const d = (r.data ?? {}) as D
            const held = r.kind === 'session.external_read'
            return (
              <div key={r.position} className="num flex items-baseline gap-2 py-[1px] text-[11px]">
                <span className="w-28 shrink-0 text-ink-faint">{stamp(r.at_unix_ms)}</span>
                <span className={held ? 'text-[#f472b6]' : 'text-ok'}>{held ? 'held' : 'trusted'}</span>
                <span className="min-w-0 truncate text-ink-dim">{title(r.session_id)}{held ? ` · ${d.tool ?? ''} ${d.query ? `“${d.query}”` : d.url ?? ''}` : d.by ? ` · by ${d.by}` : ''}</span>
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}

// ---------------------------------------------------------------- the gate

function Gate({ confirms, tight, rows, title, now }: { confirms: ConfirmRequest[]; tight: Health['tightenings']; rows: LedgerEntry[]; title: (s?: string | null) => string; now: number }) {
  const act = useAct()
  const log = rows.filter((r) => r.kind === 'policy.tightened' || r.kind === 'policy.untightened').slice(-6).reverse()
  return (
    <div className="flex flex-col gap-2">
      {!confirms.length && <div className="flex items-center gap-2 px-1 py-1 text-[12.5px] text-ink-dim"><CircleCheck size={14} className="text-ok" /> nothing waits for you</div>}
      {confirms.map((c) => (
        <div key={c.correlation_id} className="flex items-start gap-3 rounded-lg bg-wait/[0.05] px-3 py-2 ring-1 ring-wait/25">
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <span className="num text-[13px] font-semibold text-tool">{c.tool}</span>
              {c.floor && <Pill tone="fault">floor</Pill>}
              {c.external_text && <Pill tone="wait">after outside text</Pill>}
              <span className="num ml-auto text-[11px] text-ink-faint">{title(c.session_id)} · {ago(c.requested_at_ms, now)}</span>
            </div>
            <div className="mt-0.5 line-clamp-2 text-[12px] text-ink-dim">{c.reason}</div>
          </div>
          <div className="flex shrink-0 flex-col gap-1">
            <Btn tone="ok" onClick={() => act('action.confirm', { correlation_id: c.correlation_id, approve: true }, `Approve ${c.tool}?\n\n${c.reason}`)}><CircleCheck size={12} /> Approve</Btn>
            <Btn tone="fault" onClick={() => act('action.confirm', { correlation_id: c.correlation_id, approve: false }, `Decline ${c.tool}?`)}><OctagonX size={12} /> Decline</Btn>
          </div>
        </div>
      ))}
      <div className="ship-engraved mt-1 text-[9.5px]">Tightenings · “Make actions like this ask in the future”</div>
      {!tight.length && <div className="px-1 text-[12px] text-ink-faint">no tool asks first beyond its configured posture</div>}
      {tight.map((t) => (
        <div key={t.tool} className="flex items-center gap-3 rounded-lg px-3 py-1.5 ring-1 ring-line">
          <Lock size={14} className="text-wait" />
          <div className="min-w-0 flex-1">
            <div className="text-[12.5px]"><span className="num text-tool">{t.tool}</span> <span className="text-ink-dim">asks first ({t.posture})</span></div>
            <div className="num text-[11px] text-ink-faint">pressed by {t.by} via {t.via} · {ago(t.at_ms, now)}{t.session_id ? ` · in ${title(t.session_id)}` : ''}</div>
          </div>
          <Btn tone="ok" onClick={() => act('policy.untighten', { tool: t.tool }, `Let ${t.tool} go back to its configured posture?`)}><Undo2 size={12} /> Undo</Btn>
        </div>
      ))}
      {!!log.length && (
        <div className="mt-0.5">
          {log.map((r) => {
            const d = (r.data ?? {}) as D
            return (
              <div key={r.position} className="num flex items-baseline gap-2 py-[1px] text-[11px]">
                <span className="w-28 shrink-0 text-ink-faint">{stamp(r.at_unix_ms)}</span>
                <span className={r.kind === 'policy.tightened' ? 'text-wait' : 'text-ok'}>{r.kind === 'policy.tightened' ? 'tightened' : 'undone'}</span>
                <span className="truncate text-ink-dim">{d.tool} · by {d.by}</span>
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}

// ---------------------------------------------------------------- the sandbox

function sandboxLine(h?: Health): string {
  const s = h?.sandbox
  if (!s) return 'no sandbox in this daemon (tools are off)'
  const l = s.last_launch
  const launch = s.refuses ? `L1 unavailable: ${s.refuses}`
    : !l ? 'no L1 job yet since start'
    : l.ok ? `last launch ok${l.start_ms != null ? `, start ${l.start_ms.toFixed(1)} ms` : ''}`
    : `last launch failed: ${l.why ?? 'no reason given'}`
  return `limits ${s.pids} processes · ${s.scratch_mb} MB scratch · ${launch} · ${s.jobs_l1} L1 jobs since the start`
}

interface Finished { node: NodeInfo; cid: string; exit?: number; ms?: number; scratch?: string; failed: boolean }

function Sandbox({ usage, rows, results, actions, asOf, title, now, onOpen }: {
  usage?: SandboxUsage; rows: LedgerEntry[]; results: NodeInfo[]; actions: ActionInfo[]; asOf: number | null; title: (s?: string | null) => string; now: number
  onOpen: (sid: string, cid?: string) => void
}) {
  const action = useMemo(() => new Map(actions.map((a) => [a.correlation_id, a])), [actions])
  // What each job ran (`tool.job_started`), and the verdicts of the ones a cancel or a stop reached (18a).
  const started = useMemo(() => {
    const m = new Map<string, { argv: string[]; at: number; session: string | null; l1: boolean }>()
    for (const r of rows) {
      if (r.kind !== 'tool.job_started') continue
      const d = (r.data ?? {}) as D
      m.set(String(d.correlation_id), { argv: Array.isArray(d.argv) ? d.argv.map(String) : [], at: r.at_unix_ms, session: r.session_id, l1: d.class === 'l1' })
    }
    return m
  }, [rows])
  const verdicts = rows.filter((r) => r.kind.startsWith('action.cancel_')).slice(-8).reverse()
  const finished = useMemo<Finished[]>(() => {
    const out: Finished[] = []
    for (const n of results) {
      const d = (n.detail ?? {}) as D
      const meta = (d.meta?.detail ?? {}) as D
      if (meta.sandbox?.class !== 'l1') continue
      out.push({
        node: n, cid: String(d.correlation_id ?? ''), exit: typeof meta.exit_code === 'number' ? meta.exit_code : undefined,
        ms: typeof meta.duration_ms === 'number' ? meta.duration_ms : undefined, scratch: meta.scratch?.summary, failed: !!d.is_error,
      })
    }
    return out.sort((a, b) => b.node.at_unix_ms - a.node.at_unix_ms).slice(0, 10)
  }, [results])
  const live = asOf === null ? usage?.running ?? [] : []
  return (
    <div className="grid grid-cols-1 gap-3 xl:grid-cols-[1.25fr_1fr_1fr]">
      <div>
        <div className="ship-engraved mb-1.5 text-[9.5px]">Live · the L1 jobs running now</div>
        {!live.length && (
          <div className="rounded-lg px-3 py-3 text-[12px] text-ink-faint ring-1 ring-line">
            {asOf !== null ? 'the live list can’t be read back: the jobs then are counted in the seal above' : usage ? 'no sandboxed job runs now' : 'reading…'}
          </div>
        )}
        <div className="flex flex-col gap-2">
          {live.map((run) => {
            const cid = run.correlation_id
            const s = started.get(cid)
            const a = action.get(cid)
            const argv = s?.argv ?? run.argv
            const since = s?.at ?? run.started_at_ms ?? a?.dispatched_at_ms
            const sid = s?.session ?? run.session_id ?? a?.session_id
            return (
              <button type="button" key={cid} onClick={() => sid && onOpen(sid, cid)}
                className="flex items-center gap-3 rounded-lg bg-[#5eead4]/[0.05] px-3 py-2 text-left ring-1 ring-[#5eead4]/30 hover:bg-[#5eead4]/[0.08]">
                <div className="min-w-0 flex-1">
                  <div className="num truncate text-[12px] text-ink" title={argv.join(' ')}>{argv.join(' ')}</div>
                  <div className="num text-[11px] text-ink-faint">{sid ? title(sid) : short(cid)}{since ? ` · running ${ms(now - since)}` : ''} · {short(cid)}</div>
                </div>
              </button>
            )
          })}
        </div>
      </div>
      <div>
        <div className="ship-engraved mb-1.5 text-[9.5px]">Finished · what each left in scratch</div>
        {!finished.length && <div className="px-1 text-[12px] text-ink-faint">no L1 job has finished yet</div>}
        {finished.map((f) => (
          <button type="button" key={f.node.node_id} onClick={() => onOpen(f.node.session_id, f.cid)} className="flex w-full items-baseline gap-2 rounded px-1 py-[3px] text-left text-[11.5px] hover:bg-white/[0.03]">
            <span className="num w-16 shrink-0 text-ink-faint">{clock(f.node.at_unix_ms)}</span>
            <span className={cn('num w-12 shrink-0', f.failed || (f.exit ?? 0) !== 0 ? 'text-fault' : 'text-ok')}>{f.exit !== undefined ? `exit ${f.exit}` : f.failed ? 'failed' : 'ok'}</span>
            <span className="min-w-0 truncate text-ink-dim" title={started.get(f.cid)?.argv.join(' ')}>{f.scratch ?? 'no scratch summary'}{f.ms !== undefined ? ` · ${ms(f.ms)}` : ''}</span>
          </button>
        ))}
      </div>
      <div>
        <div className="ship-engraved mb-1.5 text-[9.5px]">Cancelled · 18a&rsquo;s verdicts</div>
        {!verdicts.length && <div className="px-1 text-[12px] text-ink-faint">no cancel or stop has reached a job yet</div>}
        {verdicts.map((r) => {
          const d = (r.data ?? {}) as D
          const state = r.kind === 'action.cancel_verified' ? 'termination_verified' : r.kind === 'action.cancel_uncertain' ? 'outcome_uncertain' : 'unsupported'
          const words = verdictWords({ correlation_id: String(d.correlation_id), tool: String(d.tool ?? ''), state, verified_by: String(d.verified_by ?? 'none'), ms: Number(d.ms ?? 0), ...(typeof d.killed === 'number' ? { killed: d.killed } : {}), ...(typeof d.why === 'string' ? { why: d.why } : {}) })
          return (
            <button type="button" key={r.position} onClick={() => r.session_id && onOpen(r.session_id, String(d.correlation_id))} className="flex w-full items-baseline gap-2 rounded px-1 py-[3px] text-left text-[11.5px] hover:bg-white/[0.03]">
              <span className="num w-16 shrink-0 text-ink-faint">{clock(r.at_unix_ms)}</span>
              <span className={cn('min-w-0 truncate', state === 'termination_verified' ? 'text-ok' : 'text-wait')} title={d.why}>{words}</span>
              <span className="num ml-auto shrink-0 text-ink-faint">{d.tool} · {ms(Number(d.ms ?? 0))}</span>
            </button>
          )
        })}
      </div>
    </div>
  )
}

// ---------------------------------------------------------------- the broker

function Broker({ health, rows, title, withheld }: { health?: Health; rows: LedgerEntry[]; title: (s?: string | null) => string; withheld: number }) {
  const grants = health?.broker ?? []
  const log = rows.filter((r) => r.kind === 'secret.granted' || r.kind === 'secret.withheld').slice(-8).reverse()
  return (
    <div className="flex flex-col gap-1.5">
      {!grants.length && <div className="text-[12px] text-ink-faint">the broker grants nothing in this config</div>}
      {grants.map((g) => (
        <div key={`${g.kind}:${g.to}:${g.secret}`} className="flex items-center gap-2 rounded-md px-2 py-1.5 ring-1 ring-line">
          <KeyRound size={13} className="text-gold" />
          <div className="min-w-0 flex-1 text-[12px]">
            <span className="num text-tool">{g.to}</span> <span className="text-ink-dim">gets</span> <span className="num text-ink">{g.secret}</span>
            {g.variable && <span className="text-ink-faint"> as {g.variable}</span>}
          </div>
          <Pill tone={g.posture === 'approve' ? 'wait' : 'live'}>{g.posture}</Pill>
          <span className="num w-14 text-right text-[11px] text-ink-faint">{g.uses} use{g.uses === 1 ? '' : 's'}</span>
        </div>
      ))}
      <div className="num mt-1 text-[11px] text-ink-faint">names only, never a value · {withheld} withheld in the record</div>
      {log.map((r) => {
        const d = (r.data ?? {}) as D
        const ok = r.kind === 'secret.granted'
        return (
          <div key={r.position} className="num flex items-baseline gap-2 text-[11px]">
            <span className="w-16 shrink-0 text-ink-faint">{clock(r.at_unix_ms)}</span>
            <span className={ok ? 'text-gold' : 'text-wait'}>{ok ? 'handed' : 'withheld'}</span>
            <span className="min-w-0 truncate text-ink-dim">{d.secret} → {d.program ?? d.tool}{d.why ? ` · ${d.why}` : ''} · {title(r.session_id)}</span>
          </div>
        )
      })}
    </div>
  )
}

// ---------------------------------------------------------------- labels (19a)

function Places({ health, log }: { health?: Health; log: LedgerEntry[] }) {
  const pl = health?.places
  if (!pl) return <Empty>this daemon predates the place rule</Empty>
  return (
    <div className="flex flex-col gap-1.5 text-[12px]">
      {pl.places.map((p) => (
        <div key={p.place} className="flex items-center gap-2 rounded-md px-2 py-1.5 ring-1 ring-line">
          {p.class === 'private' ? <Anchor size={13} className="text-gold" /> : <Eye size={13} className="text-live" />}
          <span className="num min-w-0 flex-1 truncate text-tool" title={p.place}>{p.name}</span>
          {p.class === 'private' ? <Pill tone="ok">private: everything</Pill> : <Pill tone="wait">shared: public tools</Pill>}
          {(p.others?.length ?? 0) > 0 && <Pill tone="fault">{`⚠ ${p.others?.join(', ')} can view it`}</Pill>}
          {p.unchecked && <span className="text-[11px] text-ink-faint" title={p.unchecked}>unchecked</span>}
        </div>
      ))}
      <div className="num px-1 text-[11.5px] text-ink-dim">
        {pl.public_paths?.length ? <>a shared place&rsquo;s file tools reach {pl.public_paths.join(', ')}</> : 'no public tree: a shared place gets no file tools'}
      </div>
      {log.slice(-5).reverse().map((r) => {
        const d = (r.data ?? {}) as D
        const what = r.kind === 'place.published'
          ? `${d.who ?? 'the owner'} published ${d.what ?? 'something'} into ${d.name ?? d.place} (${d.bytes ?? '?'} bytes)`
          : Array.isArray(d.others)
          ? (d.others.length ? `${d.name} is bound private, but ${d.others.join(', ')} can view it` : `only the owner can view ${d.name}`)
          : `who can view ${d.name} is unchecked: ${d.unread ?? ''}`
        return (
          <div key={r.position} className="num flex items-baseline gap-2 text-[11px]">
            <span className="w-16 shrink-0 text-ink-faint">{clock(r.at_unix_ms)}</span>
            <span className="min-w-0 truncate text-ink-dim">{what}</span>
          </div>
        )
      })}
    </div>
  )
}

// ---------------------------------------------------------------- egress (18c)

interface Reach { job: string; tool: string; session: string | null; host: string; at: number; connections?: number; up?: number; down?: number; why?: string; count?: number }

/** 18c's egress as a harbour chart: each L1 job that reached out on the left, the hosts on the right, a cyan line
 *  for each host it reached (thicker with more connections), and a rose dash for each refusal, which flashes for a
 *  minute after it (not in calm). Unreached hosts on the daemon's list ride dim at anchor. */
function Egress({ rows, health, title, now }: { rows: LedgerEntry[]; health?: Health; title: (s?: string | null) => string; now: number }) {
  const s = health?.sandbox
  const list = s?.egress ?? []
  const reach = useMemo<Reach[]>(() => rows.map((r) => {
    const d = (r.data ?? {}) as D
    const host = `${d.host}:${d.port}`
    return r.kind === 'sandbox.egress'
      ? { job: String(d.correlation_id), tool: String(d.tool ?? 'proc.run'), session: r.session_id, host, at: r.at_unix_ms, connections: Number(d.connections ?? 0), up: Number(d.up ?? 0), down: Number(d.down ?? 0) }
      : { job: String(d.correlation_id), tool: String(d.tool ?? 'proc.run'), session: r.session_id, host, at: r.at_unix_ms, why: String(d.why ?? 'refused'), count: Number(d.count ?? 1) }
  }), [rows])
  const counters = s?.egress_connections !== undefined
    ? `${s.egress_connections} connection${s.egress_connections === 1 ? '' : 's'} · ${bytes(s.egress_up ?? 0)} up · ${bytes(s.egress_down ?? 0)} down · ${s.egress_refused ?? 0} refused`
    : null
  if (!reach.length) {
    return (
      <div className="flex h-full min-h-28 flex-col items-center justify-center gap-2 text-center">
        <svg width="150" height="44" viewBox="0 0 150 44" aria-hidden>
          <polygon points="20,13 28,17.5 28,26.5 20,31 12,26.5 12,17.5" fill="none" stroke="#5eead4" strokeWidth="1.1" />
          {[0, 1, 2].map((i) => <line key={i} x1="32" y1="22" x2={128} y2={8 + i * 14} stroke="#9c907a" strokeWidth="0.8" strokeDasharray="2 4" />)}
          {[0, 1, 2].map((i) => <circle key={i} cx={134} cy={8 + i * 14} r="3.2" fill="none" stroke="#9c907a" strokeWidth="0.9" />)}
        </svg>
        <div className="text-[12px] text-ink-dim">No sandboxed job has reached out.</div>
        <div className="max-w-[320px] text-[11px] leading-snug text-ink-faint">
          {list.length ? `The daemon's list: ${list.join(', ')}.` : 'The config lists no host, so an L1 job reaches out only when its call names hosts and you approve them.'} Each connection is drawn here to the host it reached, and each refusal flashes.
        </div>
        {counters && <div className="num text-[10.5px] text-ink-faint">{counters}</div>}
      </div>
    )
  }
  const jobs = [...new Map(reach.map((r) => [r.job, r])).values()].sort((a, b) => b.at - a.at).slice(0, 6)
  const keep = new Set(jobs.map((j) => j.job))
  const shown = reach.filter((r) => keep.has(r.job))
  const hosts = [...new Set([...shown.map((r) => r.host), ...list])].slice(0, 8)
  const W = 420
  const H = Math.max(110, Math.max(jobs.length, hosts.length) * 26 + 16)
  const jy = (i: number) => (jobs.length <= 1 ? H / 2 : 18 + (i * (H - 36)) / (jobs.length - 1))
  const hy = (i: number) => (hosts.length <= 1 ? H / 2 : 18 + (i * (H - 36)) / (hosts.length - 1))
  return (
    <div className="flex flex-col gap-1.5">
      <svg viewBox={`0 0 ${W} ${H}`} className="w-full" role="img" aria-label="Sandboxed jobs and the hosts they reached">
        {shown.map((r, k) => {
          const a = jobs.findIndex((j) => j.job === r.job)
          const b = hosts.indexOf(r.host)
          if (a < 0 || b < 0) return null
          const [x1, y1, x2, y2] = [30, jy(a), W - 150, hy(b)]
          const path = `M ${x1} ${y1} C ${(x1 + x2) / 2} ${y1}, ${(x1 + x2) / 2} ${y2}, ${x2} ${y2}`
          if (r.why !== undefined) {
            const fresh = now - r.at < 60_000
            return (
              <g key={k}>
                <path d={path} fill="none" stroke="#fb7185" strokeWidth="1.3" strokeDasharray="4 3" className={fresh ? 'egress-refused' : undefined}><title>{r.why}</title></path>
                <text x={x2 - 10} y={y2 + 4} fill="#fb7185" fontSize="11" textAnchor="middle">✕</text>
              </g>
            )
          }
          return (
            <path key={k} d={path} fill="none" stroke="#22d3ee" strokeOpacity="0.8" strokeWidth={1 + Math.log2(1 + (r.connections ?? 1))} style={{ filter: 'drop-shadow(0 0 3px #22d3ee)' }}>
              <title>{`${r.host}: ${r.connections} connection${r.connections === 1 ? '' : 's'}, ${bytes(r.up ?? 0)} up, ${bytes(r.down ?? 0)} down`}</title>
            </path>
          )
        })}
        {jobs.map((j, i) => (
          <g key={j.job}>
            <polygon points={hexAt(18, jy(i), 8)} fill="#071322" stroke="#5eead4" strokeWidth="1.2" />
            <text x="30" y={jy(i) - 10} fill="#c8bb9b" fontSize="9.5" fontFamily="'JetBrains Mono Variable', monospace">{`${j.tool} · ${title(j.session).slice(0, 26)}`}</text>
          </g>
        ))}
        {hosts.map((h, i) => {
          const used = shown.some((r) => r.host === h && r.why === undefined)
          const refused = shown.some((r) => r.host === h && r.why !== undefined)
          return (
            <g key={h} opacity={used || refused ? 1 : 0.45}>
              <circle cx={W - 144} cy={hy(i)} r="5" fill="#071322" stroke={refused && !used ? '#fb7185' : '#d6a548'} strokeWidth="1.4" />
              <text x={W - 134} y={hy(i) + 3.5} fill={used ? '#efe3c8' : '#9c907a'} fontSize="10.5" fontFamily="'JetBrains Mono Variable', monospace">{h.length > 22 ? `${h.slice(0, 21)}…` : h}</text>
            </g>
          )
        })}
      </svg>
      <div className="num text-[10.5px] text-ink-faint">{list.length ? `the daemon's list: ${list.join(', ')}` : 'hosts named by each call, approved by you'}{counters ? ` · ${counters}` : ''}</div>
      {s?.egress_last_refused && <div className="num truncate text-[10.5px] text-fault" title={s.egress_last_refused}>last refused: {s.egress_last_refused}</div>}
    </div>
  )
}

const hexAt = (x: number, y: number, r: number) =>
  Array.from({ length: 6 }, (_, k) => {
    const a = (Math.PI / 3) * k + Math.PI / 6
    return `${(x + r * Math.cos(a)).toFixed(1)},${(y + r * Math.sin(a)).toFixed(1)}`
  }).join(' ')
