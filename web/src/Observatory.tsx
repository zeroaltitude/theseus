import { useCallback, useEffect, useMemo, useState } from 'react'
import type { ProtocolClient } from './protocol'
import type { ActionInfo, CatalogList, CompilationInfo, ExecutionInfo, Health, LedgerEntry, NodeInfo, SessionInfo, ToolList } from './protocol'

// The Observatory: every durable thing the harness wrote, as live windows onto
// the store. Nothing here is computed in the browser from events; every panel
// is a protocol query (health, execution.list, action.list, ledger.tail,
// session.list, compilation.list, node.list, tool.list, catalog.list) re-run on
// a short timer and after every turn, so what you see is what a restarted
// daemon would also see.

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

const LEDGER_FAMILIES = ['all', 'tool.', 'context.', 'execution.', 'action.', 'completion.', 'budget.', 'turn.', 'loop.', 'provider.', 'hook.', 'startup.', 'reconcile', 'session.', 'discord.', 'store.'] as const
const BINDING_CLASS: Record<string, string> = {
  ready: 'ok', connecting: 'accent', starting: 'accent', resuming: 'warn',
  unconfigured: 'muted', disabled: 'muted', disconnected: 'bad', failed: 'bad',
}
const money = (n: number | null | undefined) => n == null ? '—' : n === 0 ? '$0' : n < 0.01 ? `$${n.toFixed(4)}` : `$${n.toFixed(3)}`
const price = (n: unknown) => typeof n === 'number' && n > 0 ? String(+n.toFixed(3)) : '—'
const tokens = (n: unknown) => typeof n !== 'number' ? '—' : n >= 1_000_000 ? `${+(n / 1_000_000).toFixed(2)}M` : n >= 1000 ? `${Math.round(n / 1000)}K` : String(n)

export interface ObservatoryProps {
  client: ProtocolClient
  health: Health | null
  /// Bumps after every turn so the panels refresh at once.
  tick: number
  currentSession: string | null
  /// Called on every refresh so the header's health stays in step with the panels.
  onRefresh?: () => Promise<void> | void
  onCancelled?: () => void
  onPickSession?: (id: string) => void
}

export default function Observatory({ client, health, tick, currentSession, onRefresh, onCancelled, onPickSession }: ObservatoryProps) {
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
  const [open, setOpen] = useState<Record<string, boolean>>(() => {
    try { return JSON.parse(localStorage.getItem('theseus.obs.open') ?? '') } catch { /* default */ }
    return { context: true, tools: true, kernel: true, executions: true, actions: true, ledger: true, nodes: false, catalog: false, sessions: false }
  })
  const [compilations, setCompilations] = useState<CompilationInfo[]>([])
  const [compiles, setCompiles] = useState<LedgerEntry[]>([])
  const [nodes, setNodes] = useState<NodeInfo[]>([])
  const [nodesTotal, setNodesTotal] = useState(0)
  const [nodeKind, setNodeKind] = useState<string>('all')
  const [openNode, setOpenNode] = useState<string | null>(null)
  const [tools, setTools] = useState<ToolList | null>(null)
  const [catalog, setCatalog] = useState<CatalogList | null>(null)
  const [recompileNote, setRecompileNote] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    try {
      const sid = currentSession
      const [e, a, l, s, t] = await Promise.all([
        client.call<{ executions: ExecutionInfo[] }>('execution.list'),
        client.call<{ actions: ActionInfo[]; total: number }>('action.list', { n: 200 }),
        client.call<{ rows: LedgerEntry[]; total: number }>('ledger.tail', { n: 400 }),
        client.call<{ sessions: SessionInfo[] }>('session.list'),
        client.call<ToolList>('tool.list'),
      ])
      setExecs(e.executions.slice().sort((x, y) => y.updated_at_ms - x.updated_at_ms))
      setActions(a.actions); setActionsTotal(a.total)
      setLedger(l.rows.slice().reverse()); setLedgerTotal(l.total)
      setSessions(s.sessions)
      setTools(t)
      if (open.context) {
        const [c, cc] = await Promise.all([
          client.call<{ compilations: CompilationInfo[] }>('compilation.list', sid ? { session_id: sid } : { n: 30 }),
          client.call<{ rows: LedgerEntry[] }>('ledger.tail', { n: 12, kind: 'context.compiled', ...(sid ? { session_id: sid } : {}) }),
        ])
        setCompilations(c.compilations.slice().sort((x, y) => y.created_at_ms - x.created_at_ms))
        setCompiles(cc.rows.slice().reverse())
      }
      if (open.nodes) {
        const n = await client.call<{ nodes: NodeInfo[]; total: number }>('node.list', { n: 120, ...(sid ? { session_id: sid } : {}), ...(nodeKind !== 'all' ? { kind: nodeKind } : {}) })
        setNodes(sid ? n.nodes.slice().sort((x, y) => y.position - x.position) : n.nodes); setNodesTotal(n.total)
      }
      if (open.catalog && !catalog) setCatalog(await client.call<CatalogList>('catalog.list'))
      setNow(Date.now())
      setError(null)
      await onRefresh?.()
    } catch (err) {
      setError((err as { message?: string }).message ?? String(err))
    }
  }, [client, onRefresh, currentSession, open.context, open.nodes, open.catalog, catalog, nodeKind])

  const recompile = useCallback(async (strategy: 'fresh' | 'transcript') => {
    if (!currentSession) return
    const what = strategy === 'fresh'
      ? 'Recompile FRESH on the next turn: the model keeps only the current exchange and forgets the rest of this session (the nodes stay in the store).'
      : 'Recompile as TRANSCRIPT on the next turn: the whole history stays, thinking blocks stripped, a new cache prefix is written.'
    if (!confirm(what)) return
    try {
      await client.call('session.recompile', { session_id: currentSession, strategy })
      setRecompileNote(`next turn recompiles (${strategy})`)
      await refresh()
    } catch (err) { setError((err as { message?: string }).message ?? String(err)) }
  }, [client, currentSession, refresh])

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

  const toggle = (id: string) => setOpen((o) => {
    const n = { ...o, [id]: !o[id] }
    localStorage.setItem('theseus.obs.open', JSON.stringify(n))
    return n
  })
  const sessionTitle = (id: string) => { const x = sessions.find((y) => y.session_id === id); return x?.title ?? x?.label ?? short(id) }

  return (
    <aside className="observatory">
      <div className="obs-head">
        <strong>Observatory</strong>
        <span className="muted">what the store holds, {live ? 'live' : 'paused'}</span>
        <label className="muted"><input type="checkbox" checked={live} onChange={(e) => setLive(e.target.checked)} /> live</label>
        <button type="button" className="link" onClick={() => void refresh()}>refresh</button>
        {error && <span className="warn">{error}</span>}
      </div>

      <ObsSection id="context" title="Context" open={!!open.context} onToggle={() => toggle('context')}
        count={currentSession ? `${compilations.length} compilation${compilations.length === 1 ? '' : 's'} · this session` : `${compilations.length} recent · all sessions`}>
        <div className="pad small muted">
          A session's context is a <b>compilation</b> (a frozen prefix of nodes plus a manifest) and the <b>tail</b> of nodes written after it.
          Every loop decides <i>append</i> or <i>recompile</i>; only a new session, a model/system/tool change, overflow, or you trigger a recompile.
        </div>
        {currentSession && (
          <div className="pad small">
            <button type="button" className="chip" onClick={() => void recompile('fresh')} title="the next turn starts from the current exchange only">recompile fresh</button>{' '}
            <button type="button" className="chip" onClick={() => void recompile('transcript')} title="the next turn re-renders the whole history, thinking stripped">recompile transcript</button>
            {recompileNote && <span className="accent"> {recompileNote}</span>}
          </div>
        )}
        {compiles.length > 0 && (
          <table className="obs-table">
            <thead><tr><th>when</th><th>loop</th><th>decision</th><th>prefix + tail</th><th>messages</th><th>~tokens</th><th>repairs</th><th>digest</th></tr></thead>
            <tbody>
              {compiles.map((r) => {
                const d = (r.data ?? {}) as Record<string, unknown>
                const rep = (d.repairs as unknown[] | undefined) ?? []
                return (
                  <tr key={r.position} title={`compilation ${String(d.compilation_id ?? '')}\nturn ${r.turn_id ?? ''}\nnodes scanned ${String(d.nodes_scanned ?? '')}\ntools offered ${String(d.tools ?? '')}`}>
                    <td className="muted small">{clock(r.at_unix_ms)}</td>
                    <td className="muted">{String(d.loop ?? '')}</td>
                    <td className={d.decision === 'recompile' ? 'accent' : ''}>{String(d.decision ?? '')}{d.trigger ? <span className="muted small"> ({String(d.trigger)}, {String(d.strategy ?? '')})</span> : null}</td>
                    <td>{String(d.prefix_nodes ?? 0)} + {String(d.tail_nodes ?? 0)}</td>
                    <td>{String(d.messages ?? '')}</td>
                    <td>{fmt(Number(d.est_tokens ?? 0))}</td>
                    <td className={rep.length ? 'warn' : 'muted'} title={rep.length ? `tool_use ids with no recorded result got a synthetic error result: ${rep.join(', ')}` : ''}>{rep.length}</td>
                    <td className="muted small"><code>{String(d.digest ?? '').slice(0, 10)}</code></td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        )}
        {compilations.length === 0 ? <div className="muted pad">no compilations yet: the first loop of a session makes one</div> : (
          <table className="obs-table">
            <thead><tr><th>compilation</th><th>trigger</th><th>strategy</th><th>as of</th><th>prefix nodes</th><th>model</th><th>thinking</th>{!currentSession && <th>session</th>}</tr></thead>
            <tbody>
              {compilations.map((c) => {
                const m = c.manifest as Record<string, unknown>
                return (
                  <tr key={c.compilation_id} className={c.current ? 'mine' : ''}
                    title={`${c.compilation_id}${c.derived_from ? `\nderived from ${c.derived_from}` : ''}\ncreated ${clock(c.created_at_ms)}\nsystem ${String(m.system_digest ?? '')} · tools ${String(m.tools_digest ?? '')}\ntools: ${((m.tools as string[] | undefined) ?? []).join(', ')}\ncatalog ${String(m.catalog_version ?? '')} · window ${String(m.context_window ?? '')}\ncompiler v${String(m.compiler_version ?? '')} renderer v${String(m.renderer_version ?? '')}`}>
                    <td><code>{short(c.compilation_id)}</code>{c.current && <span className="accent small"> current</span>}</td>
                    <td>{c.trigger}</td>
                    <td className="muted">{c.strategy}</td>
                    <td className="muted">@{c.as_of}</td>
                    <td>{c.includes}</td>
                    <td className="muted small">{String(m.provider ?? '')}/{String(m.model ?? '')}</td>
                    <td className={m.strip_thinking ? 'warn small' : 'muted small'}>{m.strip_thinking ? 'stripped' : 'kept'}</td>
                    {!currentSession && <td className="muted small">{sessionTitle(c.session_id)}</td>}
                  </tr>
                )
              })}
            </tbody>
          </table>
        )}
      </ObsSection>

      <ObsSection id="tools" title="Tools" open={!!open.tools} onToggle={() => toggle('tools')}
        count={tools ? `${tools.tools.length} toollets · ${tools.calls_total} call${tools.calls_total === 1 ? '' : 's'} · shell fallback ${(tools.shell_fallback_ratio * 100).toFixed(0)}%` : ''}>
        {tools && (
          <>
            <div className="pad small muted">roots: {tools.roots.map((r) => <code key={r}>{r} </code>)} · <b>proc.run</b> (typed argv) is the only shell path; the shell-fallback ratio is proc.run calls over all calls.</div>
            <table className="obs-table">
              <thead><tr><th>tool</th><th>class</th><th>backend</th><th>posture</th><th>calls</th><th>what it does</th></tr></thead>
              <tbody>
                {tools.tools.map((t) => (
                  <tr key={t.name} title={`${t.wire_name}\n${JSON.stringify(t.input_schema, null, 2)}`}>
                    <td><code>{t.name}</code></td>
                    <td className="muted">{t.class}</td>
                    <td className="muted">{t.backend}</td>
                    <td><span className={`pill ${t.policy === 'open' ? 'ok' : t.policy === 'notify' ? 'warn' : t.policy === 'approve' ? 'accent' : 'bad'}`}>{t.policy}</span></td>
                    <td>{t.calls || <span className="muted">0</span>}</td>
                    <td className="muted small desc" title={t.description}>{t.description}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </>
        )}
      </ObsSection>

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

      <ObsSection id="discord" title="Discord" open={open.discord ?? true} onToggle={() => toggle('discord')}
        count={(health?.bindings ?? []).map((b) => `${b.state} · ${b.places.length} place${b.places.length === 1 ? '' : 's'} · ${b.messages_in} in · ${b.messages_out} out`).join(' ') || 'no binding'}>
        {(health?.bindings ?? []).length === 0 && <div className="muted pad">no binding reported: a daemon older than M3c, or a <code>--stdio</code> server (only the socket daemon binds)</div>}
        {(health?.bindings ?? []).map((b) => {
          const traffic = ledger.filter((r) => r.kind.startsWith('discord.')).slice(0, 12)
          return (
            <div key={b.kind}>
              <div className="kv">
                <div><span className="muted">state</span> <span className={`pill ${BINDING_CLASS[b.state] ?? ''}`}>{b.state}</span>
                  {b.detail && <span className={b.state === 'ready' ? 'muted small' : 'warn small'}> {b.detail}</span>}</div>
                {b.bot_user && <div><span className="muted">bot</span> <b>{b.bot_user}</b>{b.guild_id && <span className="muted"> · guild <code>{b.guild_id}</code></span>}</div>}
                {b.bindings_file && <div><span className="muted">bindings</span> <code>{b.bindings_file}</code>{b.revision && <span className="muted"> · revision <code>{b.revision}</code></span>}</div>}
                {b.connected_at_ms > 0 && <div><span className="muted">connected</span> {ago(b.connected_at_ms, now)}{b.latency_ms != null && <span className="muted"> · heartbeat {b.latency_ms} ms</span>}</div>}
                <div><span className="muted">traffic</span> <b>{b.messages_in}</b> in · <b>{b.messages_out}</b> sent · <b>{b.edits}</b> edits · <b>{b.interactions}</b> button/command presses
                  · <b className={b.ignored ? 'warn' : ''}>{b.ignored}</b> ignored · <b className={b.errors ? 'bad' : ''}>{b.errors}</b> errors</div>
                {b.last_error && <div><span className="muted">last error</span> <span className="warn small">{b.last_error}</span></div>}
              </div>
              {b.places.length > 0 && (
                <table className="obs-table">
                  <thead><tr><th>place</th><th>kind</th><th>channel</th><th>session</th><th>who may drive it</th><th>last message</th></tr></thead>
                  <tbody>
                    {b.places.map((p) => (
                      <tr key={p.label} className={p.session_id && p.session_id === currentSession ? 'mine' : ''}>
                        <td><b>{p.label}</b>{p.mention_only && <span className="muted small" title="only messages that @mention Theseus or reply to it start a turn"> · @mention only</span>}</td>
                        <td className="muted">{p.kind}</td>
                        <td className="muted small">{p.channel_id ? <code>{p.channel_id}</code> : 'opens on first DM'}</td>
                        <td>{p.session_id ? <button type="button" className="link" onClick={() => onPickSession?.(p.session_id!)} title="open this session's transcript">{sessionTitle(p.session_id)}</button> : '—'}</td>
                        <td className="muted small">{p.users.map((u) => <code key={u}>{u} </code>)}</td>
                        <td className="muted small">{p.last_activity_ms ? ago(p.last_activity_ms, now) : '—'}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
              {traffic.length > 0 && (
                <table className="obs-table">
                  <thead><tr><th>when</th><th>kind</th><th>what</th></tr></thead>
                  <tbody>
                    {traffic.map((r) => (
                      <tr key={r.position} title={JSON.stringify(r.data, null, 2)}>
                        <td className="muted small">{clock(r.at_unix_ms)}</td>
                        <td className={r.kind === 'discord.error' ? 'bad' : r.kind === 'discord.ignored' ? 'warn' : ''}>{r.kind.replace('discord.', '')}</td>
                        <td className="small">{summarize(r)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
          )
        })}
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

      <ObsSection id="nodes" title="Nodes" open={!!open.nodes} onToggle={() => toggle('nodes')}
        count={`${nodes.length} shown · ${fmt(nodesTotal)} in the store${currentSession ? ' · this session' : ''}`}>
        <div className="chips">
          {['all', 'user_message', 'assistant_message', 'tool_call', 'tool_result'].map((k) => (
            <button key={k} type="button" className={`chip ${nodeKind === k ? 'on' : ''}`} onClick={() => setNodeKind(k)}>{k}</button>
          ))}
        </div>
        <div className="ledger">
          {nodes.filter((n) => nodeKind === 'all' || n.kind === nodeKind).map((n) => (
            <div key={n.node_id} className={`lrow ${openNode === n.node_id ? 'open' : ''}`} onClick={() => setOpenNode((o) => o === n.node_id ? null : n.node_id)}>
              <span className="muted pos">{n.position}</span>
              <span className="muted">{clock(n.at_unix_ms)}</span>
              <code className={`kind node-${n.kind}`}>{n.kind}</code>
              <span className="muted small ids">{short(n.node_id)}{n.loop_index != null ? ` · loop ${n.loop_index}` : ''}</span>
              <span className="summary muted">{nodeSummary(n)}</span>
              {openNode === n.node_id && <pre className="ldata">{JSON.stringify({ ...n, text: n.text.length > 4000 ? `${n.text.slice(0, 4000)}…` : n.text }, null, 2)}</pre>}
            </div>
          ))}
        </div>
      </ObsSection>

      <ObsSection id="catalog" title="Model catalog" open={!!open.catalog} onToggle={() => toggle('catalog')} count={catalog ? `${catalog.models.length} models · ${catalog.version}` : ''}>
        {catalog && (
          <table className="obs-table">
            <thead><tr><th>model</th><th>provider</th><th>window</th><th>max out</th><th title="USD per million input tokens">$in</th><th title="USD per million output tokens">$out</th><th title="cache read / write per million">$cache r/w</th><th>thinking</th><th>profiles</th></tr></thead>
            <tbody>
              {catalog.models.map((m) => {
                const e = m.entry
                return (
                  <tr key={m.model} title={`source: ${String(e.source ?? '')}${e.refusal_fallbacks ? '\nserver-side refusal fallbacks' : ''}\ncache minimum ${String(e.cache_min_tokens ?? '')} tokens${e.vision ? '\nvision' : ''}`}>
                    <td><code>{m.model}</code></td>
                    <td className="muted">{String(e.provider ?? '')}</td>
                    <td>{tokens(e.context_window)}</td>
                    <td>{tokens(e.max_output_tokens)}</td>
                    <td>{price(e.input_per_mtok)}</td>
                    <td>{price(e.output_per_mtok)}</td>
                    <td className="muted">{price(e.cache_read_per_mtok)} / {price(e.cache_write_per_mtok)}</td>
                    <td className="muted">{String(e.thinking ?? '')}{e.effort ? ' · effort' : ''}</td>
                    <td>{m.profiles.join(', ')}</td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        )}
      </ObsSection>

      <ObsSection id="sessions" title="Sessions" open={!!open.sessions} onToggle={() => toggle('sessions')} count={`${sessions.length}`}>
        {sessions.length === 0 ? <div className="muted pad">none</div> : (
          <table className="obs-table">
            <thead><tr><th>session</th><th>last active</th><th>turns</th><th>tools</th><th>cost</th><th>tokens in / out</th><th>execution</th><th>waiting</th></tr></thead>
            <tbody>
              {sessions.map((s) => (
                <tr key={s.session_id} className={s.session_id === currentSession ? 'mine' : ''} title={`${s.session_id}\nclick to open it`} onClick={() => onPickSession?.(s.session_id)}>
                  <td>{s.title ?? s.label ?? <code>{short(s.session_id)}</code>}{s.session_id === currentSession && <span className="muted"> (open)</span>}</td>
                  <td className="muted small">{ago(Math.max(s.last_active_ms ?? 0, s.created_at_unix_ms), now)}</td>
                  <td>{s.turns}</td>
                  <td>{s.tool_calls ?? 0}</td>
                  <td>{money(s.cost_usd)}</td>
                  <td className="muted">{fmt(s.usage.input_tokens)} / {fmt(s.usage.output_tokens)}</td>
                  <td>{s.execution_state ? <State s={s.execution_state} /> : <span className="muted">none</span>}</td>
                  <td className={(s.pending_confirms ?? 0) > 0 ? 'accent' : 'muted'}>{s.pending_confirms ?? 0}</td>
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

/// One line per node: what it says, without opening the JSON.
function nodeSummary(n: NodeInfo): string {
  const d = (n.detail ?? {}) as Record<string, unknown>
  const t = n.text.replace(/\s+/g, ' ')
  switch (n.kind) {
    case 'user_message': return t.slice(0, 160)
    case 'assistant_message': return `${String(d.model ?? '')} · ${String(d.stop_reason ?? '')} · ${money(d.cost_usd as number | null)}${(d.tool_calls as unknown[] | undefined)?.length ? ` · ${(d.tool_calls as unknown[]).length} tool call(s)` : ''} · ${t.slice(0, 100)}`
    case 'tool_call': return `${String(d.tool ?? '')} ${JSON.stringify(d.input ?? {}).slice(0, 100)} · ${String((d.decision as { posture?: string; mode?: string } | null)?.posture ?? (d.decision as { mode?: string } | null)?.mode ?? (d.result as { gate?: string } | null)?.gate ?? '')}`
    case 'tool_result': return `${String(d.tool ?? '')} · ${String(d.status ?? '')}${d.late ? ' · late' : ''} · ${t.slice(0, 100)}`
    default: return t.slice(0, 120)
  }
}

/// One line per ledger row: the fields a human wants without opening the JSON.
function summarize(r: LedgerEntry): string {
  const d = (r.data ?? {}) as Record<string, unknown>
  const g = (k: string) => d[k]
  const s = (k: string) => { const v = g(k); return v == null ? '' : typeof v === 'string' ? v : JSON.stringify(v) }
  switch (true) {
    case r.kind === 'action.planned': return `${s('tool')} · reserved ${s('reserved')} · ${s('retry_class').replace(/[{}"]/g, '')}`
    case r.kind === 'action.dispatched': return `${s('tool')}${g('external_op_id') ? ` · ext ${s('external_op_id')}` : ''}`
    // Rows from before theseus-8az say `action.denied`.
    case r.kind === 'action.declined' || r.kind === 'action.denied': return `${s('tool')} · declined by ${s('by')} · ${s('reason')}`
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
    case r.kind === 'context.compiled': return `${s('decision')}${g('trigger') ? ` (${s('trigger')})` : ''} · ${s('prefix_nodes')}+${s('tail_nodes')} nodes · ${s('messages')} msg · ~${s('est_tokens')} tok`
    case r.kind === 'context.recompiled': return `${s('trigger')} · ${s('strategy')} · ${s('includes')} node(s)${g('strip_thinking') ? ' · thinking stripped' : ''}`
    case r.kind === 'tool.denied': return `${s('tool')} · ${s('reason')}`
    case r.kind === 'tool.notified': return `notified · ${s('tool')} · ${s('summary')} · ${s('setting')}`
    case r.kind === 'tool.confirm_requested': return `${s('tool')} · ${s('reason')}`
    case r.kind === 'tool.job_started': return `${JSON.stringify(g('argv') ?? [])} · pid ${s('pid')}`
    case r.kind === 'action.confirm_answered': return `${g('approved') ? 'approved' : 'declined'} by ${s('by')}${g('note') ? ` · ${s('note')}` : ''}`
    case r.kind === 'turn.trace': return 'timing tree (open for spans)'
    case r.kind === 'discord.message.in': return `${s('place')} · from ${s('author')} · ${s('chars')} chars`
    case r.kind === 'discord.message.out': return `${s('place')} · ${s('chars')} chars${g('buttons') ? ' · with Approve/Decline' : ''} · ${s('part')}`
    case r.kind === 'discord.confirm': return `${g('approve') ? 'approve' : 'decline'} by ${s('by')}${g('ok') ? '' : ` · failed: ${s('error')}`}`
    case r.kind === 'discord.command': return `/${s('command')} by ${s('by')}`
    case r.kind === 'discord.ignored': return `${s('author')} (${s('author_id')}) · ${s('reason')}`
    case r.kind === 'discord.bound': return `${s('label')} → this session`
    case r.kind === 'discord.ready': return `${s('bot')} · ${s('guilds')} guild(s) · bindings ${s('revision')}`
    case r.kind === 'discord.disconnected': return s('why')
    case r.kind === 'discord.error': return `${s('op')}: ${s('error')}`
    case r.kind === 'store.restored': return `from ${s('from')} · ${s('records')} records · ${s('sessions')} sessions${Number(g('truncated_bytes') ?? 0) > 0 ? ` · cut ${s('truncated_bytes')} torn bytes` : ''}`
    case r.kind === 'reconcile': return `woke ${(g('woke_due') as unknown[] | undefined)?.length ?? 0} · unknown ${(g('marked_unknown') as unknown[] | undefined)?.length ?? 0} · settled ${(g('settled_from_evidence') as unknown[] | undefined)?.length ?? 0} · ${s('elapsed_us')} µs`
    default: { const t = JSON.stringify(r.data); return t.length > 120 ? `${t.slice(0, 120)}…` : t }
  }
}
