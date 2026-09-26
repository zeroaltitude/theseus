import { useCallback, useEffect, useMemo, useState } from 'react'
import type { ProtocolClient } from './protocol'
import type { ActionInfo, ExecutionInfo, Health, LedgerEntry, SessionInfo } from './protocol'

// The Observatory: every durable thing the kernel wrote, as live windows onto
// the store. Nothing here is computed in the browser from events; every panel
// is a protocol query (health, execution.list, action.list, ledger.tail,
// session.list) re-run on a short timer and after every turn, so what you see
// is what a restarted daemon would also see.

const fmt = (n: number) => n.toLocaleString()
const fmtUs = (us: number) =>
  us >= 1_000_000 ? `${(us / 1e6).toFixed(2)} s` : us >= 1000 ? `${(us / 1e3).toFixed(1)} ms` : `${us} µs`
const clock = (ms: number) => {
  const d = new Date(ms)
  const p = (n: number, w = 2) => String(n).padStart(w, '0')
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`
}
const short = (id: string) => id.length > 14 ? `${id.slice(0, 4)}…${id.slice(-6)}` : id
const ago = (ms: number, now: number) => {
  const s = Math.max(0, Math.round((now - ms) / 1000))
  return s < 60 ? `${s}s ago` : s < 3600 ? `${Math.round(s / 60)}m ago` : `${Math.round(s / 3600)}h ago`
}

const STATE_CLASS: Record<string, string> = {
  running: 'ok', queued: 'accent', waiting: 'muted', blocked: 'warn',
  cancelled: 'bad', failed: 'bad', budget_exhausted: 'bad', complete: 'ok',
  planned: 'muted', authorized: 'muted', dispatched: 'accent', succeeded: 'ok', outcome_unknown: 'warn',
}
function State({ s }: { s: string }) {
  return <span className={`pill ${STATE_CLASS[s] ?? ''}`}>{s.replace('_', ' ')}</span>
}

const LEDGER_FAMILIES = ['all', 'execution.', 'action.', 'completion.', 'budget.', 'turn.', 'loop.', 'provider.', 'hook.', 'startup.', 'reconcile', 'session.'] as const

export interface ObservatoryProps {
  client: ProtocolClient
  health: Health | null
  /// Bumps after every turn so the panels refresh at once.
  tick: number
  currentSession: string | null
  /// Called on every refresh so the header's health stays in step with the panels.
  onRefresh?: () => Promise<void> | void
  onCancelled?: () => void
}

export default function Observatory({ client, health, tick, currentSession, onRefresh, onCancelled }: ObservatoryProps) {
  const [execs, setExecs] = useState<ExecutionInfo[]>([])
  const [actions, setActions] = useState<ActionInfo[]>([])
  const [actionsTotal, setActionsTotal] = useState(0)
  const [ledger, setLedger] = useState<LedgerEntry[]>([])
  const [ledgerTotal, setLedgerTotal] = useState(0)
  const [sessions, setSessions] = useState<SessionInfo[]>([])
  const [family, setFamily] = useState<(typeof LEDGER_FAMILIES)[number]>('all')
  const [onlyMine, setOnlyMine] = useState(false)
  const [pickedExec, setPickedExec] = useState<string | null>(null)
  const [openRow, setOpenRow] = useState<number | null>(null)
  const [live, setLive] = useState(true)
  const [now, setNow] = useState(Date.now())
  const [error, setError] = useState<string | null>(null)
  const [open, setOpen] = useState<Record<string, boolean>>({ kernel: true, executions: true, actions: true, ledger: true, sessions: false })

  const refresh = useCallback(async () => {
    try {
      const [e, a, l, s] = await Promise.all([
        client.call<{ executions: ExecutionInfo[] }>('execution.list'),
        client.call<{ actions: ActionInfo[]; total: number }>('action.list', { n: 200 }),
        client.call<{ rows: LedgerEntry[]; total: number }>('ledger.tail', { n: 400 }),
        client.call<{ sessions: SessionInfo[] }>('session.list'),
      ])
      setExecs(e.executions.slice().sort((x, y) => y.updated_at_ms - x.updated_at_ms))
      setActions(a.actions); setActionsTotal(a.total)
      setLedger(l.rows.slice().reverse()); setLedgerTotal(l.total)
      setSessions(s.sessions.slice().sort((x, y) => y.created_at_unix_ms - x.created_at_unix_ms))
      setNow(Date.now())
      setError(null)
      await onRefresh?.()
    } catch (err) {
      setError((err as { message?: string }).message ?? String(err))
    }
  }, [client, onRefresh])

  useEffect(() => { void refresh() }, [refresh, tick])
  useEffect(() => {
    if (!live) return
    const id = setInterval(() => void refresh(), 2500)
    return () => clearInterval(id)
  }, [live, refresh])

  const cancel = useCallback(async (id: string) => {
    if (!confirm(`Cancel execution ${id}? This is the deterministic control path: it never queues behind admission and terminates the execution's jobs.`)) return
    try {
      await client.call('execution.cancel', { execution_id: id })
      await refresh()
      onCancelled?.()
    } catch (err) { setError((err as { message?: string }).message ?? String(err)) }
  }, [client, refresh, onCancelled])

  const sessionOf = useMemo(() => {
    const m = new Map<string, string>()
    for (const e of execs) m.set(e.execution_id, e.session_id)
    return m
  }, [execs])

  const filterSession = pickedExec ? sessionOf.get(pickedExec) ?? null : onlyMine ? currentSession : null
  const rows = ledger.filter((r) =>
    (family === 'all' || r.kind.startsWith(family)) &&
    (!filterSession || r.session_id === filterSession))
  const shownActions = actions.filter((a) => !pickedExec || a.execution_id === pickedExec)

  const k = health?.kernel
  const startup = (k?.startup ?? null) as null | {
    steps?: { step: number; name: string; elapsed_us: number }[]
    requeued_interrupted?: string[]; spool_drained?: number; spool_quarantined?: number; elapsed_us?: number
    reconcile?: { woke_due?: string[]; marked_unknown?: string[]; settled_from_evidence?: string[] }
  }

  const toggle = (id: string) => setOpen((o) => ({ ...o, [id]: !o[id] }))

  return (
    <aside className="observatory">
      <div className="obs-head">
        <strong>Observatory</strong>
        <span className="muted">what the store holds, {live ? 'live' : 'paused'}</span>
        <label className="muted"><input type="checkbox" checked={live} onChange={(e) => setLive(e.target.checked)} /> live</label>
        <button type="button" className="link" onClick={() => void refresh()}>refresh</button>
        {error && <span className="warn">{error}</span>}
      </div>

      <ObsSection id="kernel" title="Kernel" open={!!open.kernel} onToggle={() => toggle('kernel')}>
        {k ? (
          <div className="kv">
            <div><span className="muted">state</span> <b className={k.accepting ? 'ok' : 'warn'}>{k.accepting ? 'accepting' : 'starting'}</b></div>
            <div><span className="muted">turns held</span> <b>{k.turns_held}</b> / {k.admission_ceiling} <span className="muted">admission ceiling</span></div>
            <div><span className="muted">executions</span> {Object.entries(k.executions_by_state).length === 0 ? <span className="muted">none</span> :
              Object.entries(k.executions_by_state).map(([s, n]) => <span key={s} className="count"><b>{n}</b> <State s={s} /></span>)}</div>
            <div><span className="muted">actions</span> {Object.entries(k.actions_by_state).length === 0 ? <span className="muted">none</span> :
              Object.entries(k.actions_by_state).map(([s, n]) => <span key={s} className="count"><b>{n}</b> <State s={s} /></span>)}</div>
            <div><span className="muted">quarantined completions</span> <b className={k.quarantined_completions ? 'warn' : ''}>{k.quarantined_completions}</b>
              <span className="muted"> (results that matched no action; never inferred into anything)</span></div>
            <div><span className="muted">ledger rows</span> <b>{fmt(health!.ledger_rows)}</b> <span className="muted">· uptime</span> <b>{fmt(health!.uptime_secs)}</b><span className="muted"> s</span></div>
            {startup?.steps && (
              <div className="startup">
                <span className="muted">last startup ({fmtUs(startup.elapsed_us ?? 0)})</span>
                <ol>
                  {startup.steps.map((s) => (
                    <li key={s.step}><span className="muted">{s.step}</span> {s.name} <span className="muted">{fmtUs(s.elapsed_us)}</span>
                      {s.name === 'load' && (startup.requeued_interrupted?.length ?? 0) > 0 && <span className="warn"> · requeued {startup.requeued_interrupted!.length} interrupted</span>}
                      {s.name === 'spool' && ((startup.spool_drained ?? 0) > 0 || (startup.spool_quarantined ?? 0) > 0) && <span> · drained {startup.spool_drained}{(startup.spool_quarantined ?? 0) > 0 && <span className="warn">, {startup.spool_quarantined} malformed</span>}</span>}
                      {s.name === 'reconcile' && startup.reconcile && ((startup.reconcile.marked_unknown?.length ?? 0) > 0 || (startup.reconcile.settled_from_evidence?.length ?? 0) > 0 || (startup.reconcile.woke_due?.length ?? 0) > 0) &&
                        <span> · woke {startup.reconcile.woke_due?.length ?? 0}, settled {startup.reconcile.settled_from_evidence?.length ?? 0}, <span className="warn">unknown {startup.reconcile.marked_unknown?.length ?? 0}</span></span>}
                    </li>
                  ))}
                </ol>
              </div>
            )}
          </div>
        ) : <div className="muted">no health yet</div>}
      </ObsSection>

      <ObsSection id="executions" title="Executions" open={!!open.executions} onToggle={() => toggle('executions')} count={`${execs.length}`}>
        {execs.length === 0 ? <div className="muted pad">none yet: the first prompt opens a session and its execution</div> : (
          <table className="obs-table">
            <thead><tr><th>execution</th><th>kind</th><th>state</th><th>turns</th><th>outstanding</th><th>queued</th><th>budget</th><th>updated</th><th></th></tr></thead>
            <tbody>
              {execs.map((e) => {
                const b = e.budget
                const pct = (n: number) => `${Math.min(100, (n / Math.max(1, b.limit)) * 100)}%`
                const mine = e.session_id === currentSession
                return (
                  <tr key={e.execution_id} className={`${pickedExec === e.execution_id ? 'picked' : ''} ${mine ? 'mine' : ''}`}
                    onClick={() => setPickedExec((p) => p === e.execution_id ? null : e.execution_id)}
                    title={`${e.execution_id}\nsession ${e.session_id}${e.ended_reason ? `\n${e.ended_reason}` : ''}\nclick to filter actions and ledger to this execution`}>
                    <td><code>{short(e.execution_id)}</code>{mine && <span className="muted"> (this tab)</span>}</td>
                    <td className="muted">{e.kind}</td>
                    <td><State s={e.state} />{e.interrupted > 0 && <span className="warn" title="times a crash interrupted a running turn; requeued at startup"> ↻{e.interrupted}</span>}</td>
                    <td>{e.turns}</td>
                    <td className={e.outstanding ? 'accent' : 'muted'}>{e.outstanding}</td>
                    <td className={e.queued_results ? 'accent' : 'muted'}>{e.queued_results}</td>
                    <td>
                      <div className="budget" title={`spent ${fmt(b.spent)} · reserved ${fmt(b.reserved)} · held unknown ${fmt(b.held_unknown)} · available ${fmt(b.available)} · limit ${fmt(b.limit)}`}>
                        <div className="bar spent" style={{ width: pct(b.spent) }} />
                        <div className="bar reserved" style={{ left: pct(b.spent), width: pct(b.reserved) }} />
                        <div className="bar held" style={{ left: pct(b.spent + b.reserved), width: pct(b.held_unknown) }} />
                      </div>
                      <span className="muted small">{fmt(b.spent)}{b.reserved > 0 && <> +{fmt(b.reserved)} rsv</>}{b.held_unknown > 0 && <span className="warn"> +{fmt(b.held_unknown)} held</span>} / {fmt(b.limit)}</span>
                    </td>
                    <td className="muted small" title={clock(e.updated_at_ms)}>{ago(e.updated_at_ms, now)}</td>
                    <td>{!['cancelled', 'failed', 'budget_exhausted', 'complete'].includes(e.state) && (
                      <button type="button" className="link danger" onClick={(ev) => { ev.stopPropagation(); void cancel(e.execution_id) }}>cancel</button>
                    )}</td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        )}
        {pickedExec && <div className="muted pad small">filtering actions and ledger to <code>{short(pickedExec)}</code> · <button type="button" className="link" onClick={() => setPickedExec(null)}>clear</button></div>}
      </ObsSection>

      <ObsSection id="actions" title="Actions" open={!!open.actions} onToggle={() => toggle('actions')} count={`${shownActions.length}${pickedExec ? '' : ` of ${actionsTotal}`}`}>
        {shownActions.length === 0 ? <div className="muted pad">none yet: the provider call inside a turn is the first action</div> : (
          <table className="obs-table">
            <thead><tr><th>correlation</th><th>tool</th><th>state</th><th>retry class</th><th>planned → dispatched → settled</th><th>reserved</th><th>seen</th><th>note</th></tr></thead>
            <tbody>
              {shownActions.map((a) => {
                const d = a.dispatched_at_ms != null ? a.dispatched_at_ms - a.planned_at_ms : null
                const s = a.settled_at_ms != null && a.dispatched_at_ms != null ? a.settled_at_ms - a.dispatched_at_ms : null
                const overdue = !['succeeded', 'failed', 'cancelled'].includes(a.state) && a.deadline_at_ms < now
                return (
                  <tr key={a.correlation_id} title={`${a.correlation_id}\nexecution ${a.execution_id}\nplanned ${clock(a.planned_at_ms)}${a.authorized_at_ms ? `\nauthorized ${clock(a.authorized_at_ms)}` : ''}${a.dispatched_at_ms ? `\ndispatched ${clock(a.dispatched_at_ms)}` : ''}${a.settled_at_ms ? `\nsettled ${clock(a.settled_at_ms)}` : ''}\ndeadline ${clock(a.deadline_at_ms)}${a.external_op_id ? `\nexternal ${a.external_op_id}` : ''}${a.result_ref ? `\nresult ${a.result_ref}` : ''}`}>
                    <td><code>{short(a.correlation_id)}</code></td>
                    <td>{a.tool}</td>
                    <td><State s={a.state} />{a.cancel && <span className="muted small"> cancel: {a.cancel}</span>}{overdue && <span className="warn small"> overdue</span>}</td>
                    <td className="muted small">{a.retry_class}{a.confirmed && ' · confirmed'}</td>
                    <td className="small">
                      <span className="muted">{clock(a.planned_at_ms)}</span>
                      {d != null && <> → <b>{fmt(d)} ms</b></>}
                      {s != null && <> → <b>{fmt(s)} ms</b></>}
                      {a.state === 'dispatched' && <span className="muted"> → …</span>}
                    </td>
                    <td className="muted small">{a.reserved_units ? fmt(a.reserved_units) : ''}</td>
                    <td className={a.completions_seen > 1 ? 'warn' : 'muted'} title="completions received; more than one means duplicates were ignored">{a.completions_seen}</td>
                    <td className="muted small">{a.resolution ?? (a.external_op_id && a.tool === 'provider.messages' ? `req ${short(a.external_op_id)}` : '')}</td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        )}
      </ObsSection>

      <ObsSection id="ledger" title="Ledger" open={!!open.ledger} onToggle={() => toggle('ledger')} count={`${rows.length} shown of ${fmt(ledgerTotal)}`}>
        <div className="chips">
          {LEDGER_FAMILIES.map((f) => (
            <button key={f} type="button" className={`chip ${family === f ? 'on' : ''}`} onClick={() => setFamily(f)}>{f === 'all' ? 'all' : f.replace(/\.$/, '.*')}</button>
          ))}
          <label className="chip-label muted"><input type="checkbox" checked={onlyMine} disabled={!!pickedExec} onChange={(e) => setOnlyMine(e.target.checked)} /> this tab's session</label>
        </div>
        <div className="ledger">
          {rows.length === 0 && <div className="muted pad">no rows match</div>}
          {rows.map((r) => (
            <div key={r.position} className={`lrow ${openRow === r.position ? 'open' : ''}`} onClick={() => setOpenRow((o) => o === r.position ? null : r.position)}>
              <span className="muted pos">{r.position}</span>
              <span className="muted">{clock(r.at_unix_ms)}</span>
              <code className={`kind ${r.kind.split('.')[0]}`}>{r.kind}</code>
              <span className="muted small ids">{r.session_id ? short(r.session_id) : ''}{r.turn_id ? ` · ${short(r.turn_id)}` : ''}</span>
              <span className="summary muted">{summarize(r)}</span>
              {openRow === r.position && <pre className="ldata">{JSON.stringify(r.data, null, 2)}</pre>}
            </div>
          ))}
        </div>
      </ObsSection>

      <ObsSection id="sessions" title="Sessions" open={!!open.sessions} onToggle={() => toggle('sessions')} count={`${sessions.length}`}>
        {sessions.length === 0 ? <div className="muted pad">none</div> : (
          <table className="obs-table">
            <thead><tr><th>session</th><th>kind</th><th>turns</th><th>tokens in / out</th><th>execution</th><th>label</th></tr></thead>
            <tbody>
              {sessions.map((s) => (
                <tr key={s.session_id} className={s.session_id === currentSession ? 'mine' : ''} title={s.session_id}>
                  <td><code>{short(s.session_id)}</code>{s.session_id === currentSession && <span className="muted"> (this tab)</span>}</td>
                  <td className="muted">{s.kind}</td>
                  <td>{s.turns}</td>
                  <td className="muted">{fmt(s.usage.input_tokens)} / {fmt(s.usage.output_tokens)}</td>
                  <td>{s.execution_state ? <State s={s.execution_state} /> : <span className="muted">none (pre-M2 session)</span>}</td>
                  <td className="muted">{s.label ?? ''}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </ObsSection>
    </aside>
  )
}

function ObsSection({ title, count, open, onToggle, children }: {
  id: string; title: string; count?: string; open: boolean; onToggle: () => void; children: React.ReactNode
}) {
  return (
    <section className="obs-section">
      <h3 onClick={onToggle}>
        <span className="chev">{open ? '▾' : '▸'}</span> {title} {count && <span className="muted">· {count}</span>}
      </h3>
      {open && children}
    </section>
  )
}

/// One line per ledger row: the fields a human wants without opening the JSON.
function summarize(r: LedgerEntry): string {
  const d = (r.data ?? {}) as Record<string, unknown>
  const g = (k: string) => d[k]
  const s = (k: string) => { const v = g(k); return v == null ? '' : typeof v === 'string' ? v : JSON.stringify(v) }
  switch (true) {
    case r.kind === 'action.planned': return `${s('tool')} · reserved ${s('reserved')} · ${s('retry_class').replace(/[{}"]/g, '')}`
    case r.kind === 'action.dispatched': return `${s('tool')}${g('external_op_id') ? ` · ext ${s('external_op_id')}` : ''}`
    case r.kind.startsWith('action.'): return `${s('outcome') || s('cancel') || ''}${g('duration_ms') != null ? ` · ${s('duration_ms')} ms` : ''}${g('usage_units') != null ? ` · ${s('usage_units')} units` : ''}${g('execution_state') ? ` · execution ${s('execution_state')}` : ''}`
    case r.kind === 'execution.running': return `turn ${s('turn')} · ${s('queued_results')} queued result(s)`
    case r.kind === 'execution.waiting': return `wake ${s('wake')} · turn ${s('turn')} took ${s('turn_ms')} ms`
    case r.kind === 'execution.queued': return `why ${s('why')}`
    case r.kind.startsWith('execution.'): return `${s('reason') || s('why') || s('by') || ''}`
    case r.kind.startsWith('budget.'): return `${s('units') ? `${s('units')} units · ` : ''}${s('purpose') || ''}${g('actual') != null ? `actual ${s('actual')}` : ''}${g('available_after') != null ? ` · ${s('available_after')} available` : ''}`
    case r.kind.startsWith('completion.'): return `${s('producer')} · ${s('outcome') || ''}${g('seen') ? ` · seen ${s('seen')}` : ''}`
    case r.kind === 'startup.step': return `${s('step')} ${s('name')}${g('requeued') ? ` · requeued ${s('requeued')}` : ''}${g('drained') != null ? ` · drained ${s('drained')}` : ''}`
    case r.kind === 'turn.started': return `${s('profile')} → ${s('provider')}/${s('model')} · ${s('input_chars')} chars`
    case r.kind === 'turn.ended': return `${s('loops')} loop(s) · ${s('stop_reason')} · ${s('elapsed_ms')} ms · first token ${s('first_token_ms')} ms`
    case r.kind === 'turn.failed': return s('reason')
    case r.kind === 'provider.call': return `${s('model')} · ${s('stop_reason')} · ${JSON.stringify(g('usage') ?? {})}`
    case r.kind === 'provider.error': return `${s('class')}${g('transient') ? ' transient' : ''}${g('usage_unknown') ? ' usage unknown' : ''} · ${s('message')}`
    case r.kind === 'hook.site': return `${s('event')} · ${s('handlers')} handler(s) · ${s('outcome')}`
    case r.kind === 'loop.ended': return `loop ${s('loop')} · ${s('decision').replace(/[{}"]/g, '')}`
    case r.kind === 'reconcile': return `woke ${(g('woke_due') as unknown[] | undefined)?.length ?? 0} · unknown ${(g('marked_unknown') as unknown[] | undefined)?.length ?? 0} · settled ${(g('settled_from_evidence') as unknown[] | undefined)?.length ?? 0} · ${s('elapsed_us')} µs`
    default: { const t = JSON.stringify(r.data); return t.length > 120 ? `${t.slice(0, 120)}…` : t }
  }
}
