import { useCallback, useEffect, useMemo, useState } from 'react'
import type { ProtocolClient } from './protocol'
import type { ActionInfo, CatalogList, CompilationInfo, ConfigStatus, ContextFileRef, ExecutionInfo, Health, LedgerEntry, NodeInfo, SessionInfo, StartupPhase, ToolList, WakeInfo } from './protocol'

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
/// A wake's time from now: `in 9m`, or `due 3m ago` while its session is busy (DD8).
const until = (ms: number, now: number) => {
  const s = Math.round(Math.abs(ms - now) / 1000)
  const t = s < 60 ? `${s}s` : s < 3600 ? `${Math.round(s / 60)}m` : s < 86_400 ? `${Math.round(s / 3600)}h` : `${Math.round(s / 86_400)}d`
  return ms >= now ? `in ${t}` : `due ${t} ago`
}

const STATE_CLASS: Record<string, string> = {
  running: 'ok', queued: 'accent', waiting: 'muted', blocked: 'warn',
  cancelled: 'bad', failed: 'bad', budget_exhausted: 'bad', complete: 'ok',
  planned: 'muted', authorized: 'muted', dispatched: 'accent', succeeded: 'ok', outcome_unknown: 'warn',
}
function State({ s }: { s: string }) {
  return <span className={`pill ${STATE_CLASS[s] ?? ''}`}>{s.replace('_', ' ')}</span>
}

const LEDGER_FAMILIES = ['all', 'tool.', 'context.', 'execution.', 'action.', 'approval.', 'policy.', 'completion.', 'budget.', 'turn.', 'loop.', 'provider.', 'hook.', 'startup.', 'config.', 'reconcile', 'session.', 'discord.', 'store.'] as const
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

  // "Should have asked" and its undo (theseus-sgh). A refused undo says why here.
  const [policyNote, setPolicyNote] = useState<string | null>(null)
  const tighten = useCallback(async (tool: string, correlationId: string) => {
    try {
      await client.call('policy.tighten', { tool, correlation_id: correlationId || undefined })
      setPolicyNote(`${tool} asks first from now on`)
      await refresh()
    } catch (err) { setPolicyNote((err as { message?: string }).message ?? String(err)) }
  }, [client, refresh])
  const untighten = useCallback(async (tool: string) => {
    if (!confirm(`Undo the tightening of ${tool}? It goes back to what the config says, which may run it without asking.`)) return
    try {
      await client.call('policy.untighten', { tool })
      setPolicyNote(`${tool} is back to what the config says`)
      await refresh()
    } catch (err) { setPolicyNote((err as { message?: string }).message ?? String(err)) }
  }, [client, refresh])
  const tightenings = health?.tightenings ?? []
  const isTightened = (tool: string) => tightenings.some((t) => t.tool === tool)

  // Pending wakes (DD8): health lists them; a cancel is `wake.cancel`, and
  // its author is the web UI.
  const wakes: WakeInfo[] = health?.wakes ?? []
  const cancelWake = useCallback(async (w: WakeInfo) => {
    if (!confirm(`Cancel wake ${w.short}, due ${w.due_local}? Its turn will not run: "${w.note}"`)) return
    try {
      await client.call('wake.cancel', { wake: w.wake_id })
      await refresh()
    } catch (err) { setError((err as { message?: string }).message ?? String(err)) }
  }, [client, refresh])

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
  const ch = health?.children
  const approval = health?.approval
  const approvalRows = ledger.filter((r) => r.kind.startsWith('approval.')).slice(0, 8)
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
  // The context files of this session's current compilation (theseus-58a).
  const contextFiles = currentSession
    ? ((compilations.find((c) => c.current)?.manifest.context_files as ContextFileRef[] | undefined) ?? [])
    : []

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
        {health?.context && (health.context.system_files.length > 0 || health.context.persona || health.context.personas.length > 0) && (
          <div className="pad small" title="the config's context files: the system level, which every session gets, then the persona in play's; until Jev chooses one, the persona is [context] default_persona">
            context files: <b>{health.context.system_files.length}</b> at the system level
            {' · '}{health.context.persona
              ? <>persona <b>{health.context.persona}</b> in play, with <b>{health.context.persona_files.length}</b></>
              : <span className={health.context.personas.length ? 'warn' : 'muted'}>no persona in play{health.context.personas.length ? ` (defined: ${health.context.personas.join(', ')})` : ''}</span>}
          </div>
        )}
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
        {contextFiles.length > 0 && (
          <table className="obs-table" title="the files the current compilation's system block carries, in order; an edit changes the digest and recompiles the next turn">
            <thead><tr><th>context file</th><th>level</th><th>digest</th><th>bytes</th></tr></thead>
            <tbody>
              {contextFiles.map((f, i) => (
                <tr key={`${i}:${f.path}`}>
                  <td className="small"><code>{f.path}</code></td>
                  <td className="muted small">{f.persona ? `persona ${f.persona}` : 'system'}</td>
                  <td className={f.missing ? 'warn small' : 'muted small'}>{f.missing ? `missing: ${f.missing}` : <code>{f.digest}</code>}</td>
                  <td className={f.cut ? 'warn small' : 'muted small'}>{f.missing ? '' : `${fmt(f.bytes)}${f.cut ? ' (cut)' : ''}`}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </ObsSection>

      <ObsSection id="tools" title="Tools" open={!!open.tools} onToggle={() => toggle('tools')}
        count={tools ? `${tools.tools.length} toollets · ${tools.calls_total} call${tools.calls_total === 1 ? '' : 's'} since start · shell fallback ${(tools.shell_fallback_ratio * 100).toFixed(0)}%${tightenings.length ? ` · ${tightenings.length} tightened` : ''}` : ''}>
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
                    <td title={t.setting ? `${t.setting}${t.config_posture && t.config_posture !== t.policy ? `\nthe config says ${t.config_posture} (${t.config_setting ?? ''})` : ''}` : ''}>
                      <span className={`pill ${t.policy === 'open' ? 'ok' : t.policy === 'notify' ? 'warn' : t.policy === 'approve' ? 'accent' : 'bad'}`}>{t.policy}</span>
                      {t.tightened && t.config_posture !== t.policy && <span className="accent small"> tightened</span>}
                    </td>
                    <td>{t.calls || <span className="muted">0</span>}</td>
                    <td className="muted small desc" title={t.description}>{t.description}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <div className="pad small muted">
              <b>Should have asked:</b> one press on a notice makes that tool ask first from then on. It is stored, never in the config,
              and only tightens: the stricter of it and the config wins. Undo returns the tool to what the config says, so it counts only
              where an approval would.
            </div>
            {policyNote && <div className="pad small accent">{policyNote}</div>}
            {tightenings.length > 0 && (
              <table className="obs-table">
                <thead><tr><th>tightened</th><th>since</th><th>by</th><th>via</th><th>the call</th><th></th></tr></thead>
                <tbody>
                  {tightenings.map((t) => (
                    <tr key={t.tool} title={t.digest ? `proposal digest ${t.digest}` : 'pressed without naming a call'}>
                      <td><code>{t.tool}</code> <span className="muted small">asks first</span></td>
                      <td className="muted small" title={clock(t.at_ms)}>{ago(t.at_ms, now)}</td>
                      <td className="small">{t.by}</td>
                      <td className="muted small">{t.via ?? ''}</td>
                      <td className="muted small">{t.correlation_id ? <code>{short(t.correlation_id)}</code> : '—'}{t.session_id && <> · {sessionTitle(t.session_id)}</>}</td>
                      <td><button type="button" className="link danger" onClick={() => void untighten(t.tool)}>undo</button></td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
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
            {!!k.lingering_wrappers && (
              <div><span className="muted">lingering job wrappers</span> <b>{k.lingering_wrappers}</b>
                <span className="muted"> (a command exited and left processes running; each wrapper waits for them, so they stay under their job)</span></div>
            )}
            {ch && (
              <div><span className="muted">children</span> <b>{ch.wrappers_running}</b> <span className="muted">job wrappers running</span>
                {ch.wrappers_lingering > 0 && <>, <b>{ch.wrappers_lingering}</b> <span className="muted">lingering</span></>}
                <span className="muted"> · adopted orphans</span> <b className={ch.orphans ? 'warn' : ''}>{ch.orphans}</b>
                <span className="muted"> · zombies</span> <b className={ch.zombies ? 'warn' : ''}>{ch.zombies}</b>
                <span className="muted"> · reaped {fmt(ch.reaped_wrappers)} wrappers, {fmt(ch.reaped_orphans)} orphans</span>
                {!ch.subreaper && <span className="muted"> · not a subreaper</span>}
                <span className="muted"> (an orphan is a job's process whose wrapper died; it cannot answer an approval)</span></div>
            )}
            <div><span className="muted">secret broker</span>{' '}
              {(health!.broker ?? []).length === 0
                ? <span className="muted">no grants</span>
                : (health!.broker ?? []).map((g, i) => (
                  <span key={`${g.to}/${g.variable ?? g.secret}`}>{i > 0 && <span className="muted"> · </span>}
                    <b>{g.to}</b> <span className="muted">gets</span> <b>{g.variable ?? g.secret}</b>
                    <span className="muted"> ({g.secret}, {g.posture}), used {fmt(g.uses)}×</span></span>))}
              <span className="muted"> (names only: a value never leaves the daemon but as the grant's variable)</span></div>
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

      <ObsSection id="startup" title="Startup" open={open.startup ?? true} onToggle={() => toggle('startup')}
        count={startupCount(health)}>
        <StartupView health={health} />
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
                {b.members_intent != null && <div><span className="muted">Server Members intent</span> <b className={b.members_intent ? 'ok' : 'warn'}>{b.members_intent ? 'on' : 'off'}</b>
                  <span className="muted small"> · {b.members_intent ? 'who can view a guild channel is checked for approvals' : 'a guild channel cannot be verified for approvals, so none is trusted'}</span></div>}
                {b.connected_at_ms > 0 && <div><span className="muted">connected</span> {ago(b.connected_at_ms, now)}{b.latency_ms != null && <span className="muted"> · heartbeat {b.latency_ms} ms</span>}</div>}
                <div><span className="muted">traffic</span> <b>{b.messages_in}</b> in · <b>{b.messages_out}</b> sent · <b>{b.edits}</b> edits · <b>{b.interactions}</b> button/command presses
                  · <b className={b.ignored ? 'warn' : ''}>{b.ignored}</b> ignored · <b className={b.errors ? 'bad' : ''}>{b.errors}</b> errors</div>
                {b.last_error && <div><span className="muted">last error</span> <span className="warn small">{b.last_error}</span></div>}
                {b.outbox && <div title="replies, cards, and notices are written when they happen and sent once, in order per place, when Discord can take them">
                  <span className="muted">outbox</span> <b className={b.outbox.pending ? 'warn' : 'ok'}>{b.outbox.pending}</b> pending
                  {b.outbox.pending > 0 && b.outbox.oldest_pending_ms > 0 && <span className="muted"> · oldest {ago(b.outbox.oldest_pending_ms, now)}</span>}
                  {' · '}<b>{b.outbox.sent}</b> sent · <b className={b.outbox.failed ? 'bad' : ''}>{b.outbox.failed}</b> refused
                  {b.outbox.last_error && <span className="warn small"> · last error {b.outbox.last_error_ms ? ago(b.outbox.last_error_ms, now) : ''}: {b.outbox.last_error}</span>}</div>}
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

      <ObsSection id="approval" title="Approval" open={open.approval ?? true} onToggle={() => toggle('approval')}
        count={approval?.configured ? `${approval.channels.filter((c) => c.state === 'trusted').length} of ${approval.channels.length} channels trusted` : 'no rule'}>
        {!approval?.configured ? (
          <div className="muted pad">no <code>[approval]</code> section: the CLI, this web UI, and a place's listed Discord users can answer a waiting call</div>
        ) : (
          <>
            <div className="kv">
              <div><span className="muted">trusted users</span> {approval.trusted_users.length === 0 ? <span className="muted">nobody on Discord</span> :
                approval.trusted_users.map((u) => <code key={u}>{u} </code>)}</div>
              <div className="muted small">an answer counts only from a trusted user through a trusted channel; any other is refused with the reason, and the call keeps waiting</div>
            </div>
            <table className="obs-table">
              <thead><tr><th>channel</th><th>state</th><th>why</th><th>checked</th></tr></thead>
              <tbody>
                {approval.channels.map((c) => (
                  <tr key={c.channel}>
                    <td><code>{c.channel}</code></td>
                    <td><span className={`pill ${c.state === 'trusted' ? 'ok' : 'warn'}`}>{c.state === 'trusted' ? 'trusted' : 'not trusted'}</span></td>
                    <td className="small">{c.detail}</td>
                    <td className="muted small">{c.checked_at_ms ? ago(c.checked_at_ms, now) : '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            {approvalRows.length > 0 && (
              <table className="obs-table">
                <thead><tr><th>when</th><th>kind</th><th>what</th></tr></thead>
                <tbody>
                  {approvalRows.map((r) => (
                    <tr key={r.position} title={JSON.stringify(r.data, null, 2)}>
                      <td className="muted small">{clock(r.at_unix_ms)}</td>
                      <td className={r.kind === 'approval.refused' ? 'warn' : ''}>{r.kind.replace('approval.', '')}</td>
                      <td className="small">{summarize(r)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </>
        )}
      </ObsSection>

      <ObsSection id="wakes" title="Wakes" open={open.wakes ?? true} onToggle={() => toggle('wakes')} count={`${wakes.length} pending`}>
        {wakes.length === 0 ? <div className="muted pad">none pending: a conversation sets one with <code>wake.at</code> ("remind me in 10 minutes to check the build"), and at its time the session gets a turn whose input is the note</div> : (
          <table className="obs-table">
            <thead><tr><th>wake</th><th>due</th><th>session</th><th>note</th><th></th></tr></thead>
            <tbody>
              {wakes.map((w) => (
                <tr key={w.wake_id} className={w.session_id === currentSession ? 'mine' : ''}
                  title={`${w.wake_id}\nset ${clock(w.set_at_ms)}${w.target ? `\nits reply goes to ${w.target}` : ''}`}>
                  <td><code>{w.short}</code></td>
                  <td title={w.due_local}>{clock(w.due_at_ms).slice(0, 8)} <span className={w.due_at_ms < now ? 'warn small' : 'muted small'}>{until(w.due_at_ms, now)}</span></td>
                  <td><button type="button" className="link" onClick={() => onPickSession?.(w.session_id)}>{w.session_title ?? short(w.session_id)}</button> <State s={w.state} /></td>
                  <td>{w.note}</td>
                  <td><button type="button" className="link danger" onClick={() => void cancelWake(w)}>cancel</button></td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </ObsSection>

      <ObsSection id="executions" title="Executions" open={!!open.executions} onToggle={() => toggle('executions')} count={`${execs.length}`}>
        {execs.length === 0 ? <div className="muted pad">none yet: the first prompt opens a session and its execution</div> : (
          <table className="obs-table">
            <thead><tr><th>execution</th><th>kind</th><th>state</th><th>turns</th><th>outstanding</th><th>queued</th><th>budget</th><th>updated</th><th></th></tr></thead>
            <tbody>
              {execs.map((e) => {
                const b = e.budget
                const pct = (n: number) => `${Math.min(100, (n / Math.max(1e-6, b.limit_usd)) * 100)}%`
                const before = b.units_before ? `\nbefore dollar budgets: ${fmt(b.units_before.spent)} of ${fmt(b.units_before.limit)} units` : ''
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
                      <div className="budget" title={`spent ${money(b.spent_usd)} since ${(b.resets ?? 0) > 0 ? `the last of ${b.resets} reset(s)` : 'it opened'} · reserved ${money(b.reserved_usd)} · held unknown ${money(b.held_unknown_usd)} · available ${money(b.available_usd)} · limit ${money(b.limit_usd)}${before}`}>
                        <div className="bar spent" style={{ width: pct(b.spent_usd) }} />
                        <div className="bar reserved" style={{ left: pct(b.spent_usd), width: pct(b.reserved_usd) }} />
                        <div className="bar held" style={{ left: pct(b.spent_usd + b.reserved_usd), width: pct(b.held_unknown_usd) }} />
                      </div>
                      <span className="muted small">{money(b.spent_usd)}{b.reserved_usd > 0 && <> +{money(b.reserved_usd)} rsv</>}{b.held_unknown_usd > 0 && <span className="warn"> +{money(b.held_unknown_usd)} held</span>} / {money(b.limit_usd)}{(b.resets ?? 0) > 0 && <> · ↺{b.resets}</>}</span>
                      {b.question && <span className="pill warn" title={`the session reached its limit and asks you: ${b.question}`}>at its limit</span>}
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
                    <td className="muted small">{a.reserved_usd ? money(a.reserved_usd) : ''}</td>
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
              {r.kind === 'tool.notified' && <NoticeAsk r={r} tightened={isTightened} onTighten={tighten} />}
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

/// "Should have asked" on a notice's ledger row (theseus-sgh), or a note that its tool asks first now.
function NoticeAsk({ r, tightened, onTighten }: {
  r: LedgerEntry; tightened: (tool: string) => boolean; onTighten: (tool: string, correlationId: string) => Promise<void>
}) {
  const d = (r.data ?? {}) as Record<string, unknown>
  const tool = typeof d.tool === 'string' ? d.tool : ''
  if (!tool) return null
  if (tightened(tool)) return <span className="muted small"> · asks first now</span>
  const corr = typeof d.correlation_id === 'string' ? d.correlation_id : ''
  return (
    <button type="button" className="link small" title={`${tool} asks first from now on, on every surface; undo it in the Tools view`}
      onClick={(e) => { e.stopPropagation(); void onTighten(tool, corr) }}>should have asked</button>
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

const SECRETS_CLASS: Record<string, string> = { ready: 'ok', resolving: 'warn', failed: 'bad' }
const CONFIG_CLASS: Record<string, string> = { confirmed: 'ok', confirming: 'warn', held: 'bad', restarting: 'warn' }

/// When the last start served, and where its secrets stand, in a few words.
function startupCount(health: Health | null): string {
  const phases = health?.startup ?? []
  const ends = phases.filter((p) => !p.background && p.end_us != null).map((p) => p.end_us!)
  const serving = ends.length ? `serving at ${fmtUs(Math.max(...ends))}` : ''
  const s = health?.secrets?.state
  const secrets = s === 'failed'
    ? `secrets failed: ${health!.secrets!.failed.map((f) => f.name).join(', ')}`
    : s ? `secrets ${s}` : ''
  const c = health?.config
  const config = c?.state && c.state !== 'confirmed' ? `config ${c.state}` : ''
  return [serving, config, secrets].filter(Boolean).join(' · ')
}

/// Where the config came from, whether it may act, and what a restart onto the vault's
/// changed note changed (theseus-2fo).
function ConfigLine({ c }: { c: ConfigStatus }) {
  const from = c.source === 'file' ? `file ${c.reference}`
    : c.started_from === 'copy' ? `the copy of ${c.reference}` : c.reference
  return (
    <>
      <div>
        <span className="muted">config</span> <span className={`pill ${CONFIG_CLASS[c.state] ?? ''}`}>{c.state}</span>
        <span className="muted small"> {from}</span>
        {c.state === 'confirmed' && c.confirmed_ms != null && c.source === 'vault' &&
          <span className="muted small"> · {c.started_from === 'copy' ? 'confirmed by the vault' : 'read before serving'} {fmt(c.confirmed_ms)} ms after start</span>}
        {c.detail && <span className={c.state === 'held' ? 'bad small' : 'muted small'}> · {c.detail}</span>}
        {c.state === 'held' && c.retry_in_ms != null && <span className="muted small"> · read again in {Math.ceil(c.retry_in_ms / 1000)} s</span>}
      </div>
      {c.state !== 'confirmed' && c.source === 'vault' &&
        <div className="warn small">Nothing acts until the vault confirms the copy: reads answer, and every method that acts waits.</div>}
      {c.restarted &&
        <div className="warn small">Restarted {new Date(c.restarted.at_unix_ms).toLocaleTimeString()} onto the vault's note, which had changed since the copy: {c.restarted.tables.join(', ')}.</div>}
    </>
  )
}

/// What a phase found, from its detail.
function phaseOutcome(p: StartupPhase): string {
  const d = p.detail ?? {}
  const parts: string[] = []
  if (typeof d.outcome === 'string') parts.push(d.outcome)
  if (typeof d.state === 'string') parts.push(d.state)
  if (typeof d.method === 'string') parts.push(d.method)
  if (typeof d.secret === 'string') parts.push(`secret ${d.secret}`)
  if (typeof d.waited_ms === 'number') parts.push(`waited ${fmt(d.waited_ms)} ms`)
  if (typeof d.source === 'string') parts.push(`from ${d.source}`)
  if (typeof d.last_position === 'number') parts.push(`${fmt(d.last_position)} positions`)
  if (typeof d.replayed_into_index === 'number' && d.replayed_into_index > 0) parts.push(`${fmt(d.replayed_into_index)} replayed`)
  if (typeof d.login === 'string') parts.push(d.login)
  if (typeof d.error === 'string') parts.push(d.error)
  if (Array.isArray(d.failed) && d.failed.length > 0) parts.push(`failed: ${d.failed.join(', ')}`)
  if (Array.isArray(d.steps)) {
    parts.push((d.steps as { name: string; us: number }[]).map((s) => `${s.name} ${fmtUs(s.us)}`).join(' · '))
  }
  return parts.join(' · ')
}

/// The last start (theseus-qa0): the phases on the path to serving, then those after it
/// (the secrets, and each consumer's wait for its own), on one axis from process start,
/// so a slow start names its cause the first time it happens.
function StartupView({ health }: { health: Health | null }) {
  const phases = health?.startup ?? []
  const s = health?.secrets
  if (!phases.length && !s?.state) {
    return <div className="muted pad">no startup phases: a daemon older than M3.5 (theseus-qa0)</div>
  }
  const ends = phases.filter((p) => !p.background && p.end_us != null).map((p) => p.end_us!)
  const serving = ends.length ? Math.max(...ends) : 0
  const span = Math.max(1, serving, ...phases.map((p) => p.end_us ?? p.start_us))
  // The part of the start path no phase names: a slow start that shows here has an unnamed cause.
  const named = phases.filter((p) => !p.background && p.end_us != null).reduce((t, p) => t + (p.end_us! - p.start_us), 0)
  const between = Math.max(0, serving - named)
  return (
    <>
      <div className="kv">
        <div><span className="muted">serving</span> <b>{fmtUs(serving)}</b> <span className="muted">after the process started</span>
          {between >= 500 && <span className={between > serving / 4 ? 'warn small' : 'muted small'}> · {fmtUs(between)} between phases, which no phase names</span>}</div>
        {health?.config?.state && <ConfigLine c={health.config} />}
        {s?.state && (
          <div>
            <span className="muted">secrets</span> <span className={`pill ${SECRETS_CLASS[s.state] ?? ''}`}>{s.state}</span>
            {s.settled_ms != null && <span className="muted small"> settled {fmt(s.settled_ms)} ms after start{s.method ? `, by ${s.method}` : ''}{s.rounds > 1 ? `, in ${s.rounds} rounds` : ''}</span>}
            {s.ready.length > 0 && <span className="muted small"> · ready: {s.ready.join(', ')}</span>}
            {s.resolving.length > 0 && <span className="warn small"> · resolving: {s.resolving.join(', ')}</span>}
          </div>
        )}
        {(s?.failed ?? []).map((f) => (
          <div key={f.name} className="bad small">{f.name} did not resolve: {f.error}. Whatever needs it waits, and never runs without it.</div>
        ))}
        {s?.retry_in_ms != null && s.failed.length > 0 && <div className="muted small">fetched again in {Math.ceil(s.retry_in_ms / 1000)} s</div>}
      </div>
      <table className="obs-table startup-phases">
        <thead><tr><th>phase</th><th>began</th><th>took</th><th className="phase-axis">from process start to {fmtUs(span)}</th><th>found</th></tr></thead>
        <tbody>
          {phases.map((p) => {
            const end = p.end_us ?? span
            return (
              <tr key={`${p.name}-${p.start_us}`}>
                <td><code>{p.name}</code>{p.background && <span className="muted small"> after serving</span>}</td>
                <td className="muted small">{fmtUs(p.start_us)}</td>
                <td>{p.end_us == null ? <span className="warn">running</span> : fmtUs(p.end_us - p.start_us)}</td>
                <td className="phase-axis">
                  <div className="phase-track">
                    <div className={`phase-bar ${p.background ? 'after' : 'path'}`}
                      style={{ left: `${(p.start_us / span) * 100}%`, width: `${Math.max(0.4, ((end - p.start_us) / span) * 100)}%` }} />
                  </div>
                </td>
                <td className="muted small">{phaseOutcome(p)}</td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </>
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
    // Rows from before theseus-0sg reserve units (`reserved`); later ones dollars.
    case r.kind === 'action.planned': return `${s('tool')} · reserved ${g('reserved_usd') != null ? money(Number(g('reserved_usd'))) : `${s('reserved')} units`} · ${s('retry_class').replace(/[{}"]/g, '')}`
    case r.kind === 'action.dispatched': return `${s('tool')}${g('external_op_id') ? ` · ext ${s('external_op_id')}` : ''}`
    // Rows from before theseus-8az say `action.denied`.
    case r.kind === 'action.declined' || r.kind === 'action.denied': return `${s('tool')} · declined by ${s('by')} · ${s('reason')}`
    case r.kind.startsWith('action.'): return `${s('outcome') || s('cancel') || ''}${g('duration_ms') != null ? ` · ${s('duration_ms')} ms` : ''}${g('cost_usd') != null ? ` · ${money(Number(g('cost_usd')))}` : ''}${g('usage_units') != null ? ` · ${s('usage_units')} units` : ''}${g('execution_state') ? ` · execution ${s('execution_state')}` : ''}`
    case r.kind === 'execution.running': return `turn ${s('turn')} · ${s('queued_results')} queued result(s)`
    case r.kind === 'execution.waiting': return `wake ${s('wake')} · turn ${s('turn')} took ${s('turn_ms')} ms`
    case r.kind === 'execution.queued': return `why ${s('why')}`
    case r.kind.startsWith('execution.'): return `${s('reason') || s('why') || s('by') || ''}`
    case r.kind === 'budget.asked': return `at its limit: spent ${money(Number(g('spent_usd')))} of ${money(Number(g('limit_usd')))}, the call needs ${money(Number(g('needed_usd')))}`
    case r.kind === 'budget.reset': return `spend reset to $0 by ${s('by')} · it was ${money(Number(g('spent_before_usd')))} of ${money(Number(g('limit_usd')))} · reset ${s('resets')}`
    case r.kind === 'budget.migrated': return `unit budget read in dollars · ${s('state')} · spent ${money(Number(g('spent_usd')))} of ${money(Number(g('limit_usd')))}`
    // theseus-3pj: an open session took the config's changed spend limit.
    case r.kind === 'budget.limit_changed': return `limit ${money(Number(g('from_usd')))} → ${money(Number(g('to_usd')))} (the config's) · spent ${money(Number(g('spent_usd')))}, ${money(Number(g('available_usd')))} left${g('proceeds') ? ' · the waiting call proceeds' : ''}${g('withdrew') ? ' · its question withdrawn' : ''}`
    case r.kind.startsWith('budget.'): return `${s('units') ? `${s('units')} units · ` : ''}${s('purpose') || ''}${g('actual') != null ? `actual ${s('actual')}` : ''}${g('available_after') != null ? ` · ${s('available_after')} available` : ''}`
    case r.kind.startsWith('completion.'): return `${s('producer')} · ${s('outcome') || ''}${g('seen') ? ` · seen ${s('seen')}` : ''}`
    case r.kind === 'startup.step': return `${s('step')} ${s('name')}${g('requeued') ? ` · requeued ${s('requeued')}` : ''}${g('drained') != null ? ` · drained ${s('drained')}` : ''}`
    case r.kind === 'turn.started': return `${s('profile')} → ${s('provider')}/${s('model')} · ${s('input_chars')} chars`
    case r.kind === 'turn.ended': return `${s('loops')} loop(s) · ${s('stop_reason')} · ${s('elapsed_ms')} ms · first token ${s('first_token_ms')} ms`
    case r.kind === 'turn.failed': return s('reason')
    case r.kind === 'provider.call': return `${s('model')} · ${s('stop_reason')} · ${JSON.stringify(g('usage') ?? {})}`
    case r.kind === 'provider.error': return `${s('class')}${g('transient') ? ' transient' : ''}${g('usage_unknown') ? ' usage unknown' : ''} · ${s('message')}`
    // Rows from before theseus-hco, which removed the hook system; old stores keep them.
    case r.kind === 'hook.site': return `${s('event')} · ${s('handlers')} handler(s) · ${s('outcome')}`
    case r.kind === 'loop.ended': return `loop ${s('loop')} · ${s('decision').replace(/[{}"]/g, '')}`
    case r.kind === 'context.compiled': return `${s('decision')}${g('trigger') ? ` (${s('trigger')})` : ''} · ${s('prefix_nodes')}+${s('tail_nodes')} nodes · ${s('messages')} msg · ~${s('est_tokens')} tok`
    case r.kind === 'context.recompiled': return `${s('trigger')} · ${s('strategy')} · ${s('includes')} node(s)${g('strip_thinking') ? ' · thinking stripped' : ''}`
    case r.kind === 'tool.denied': return `${s('tool')} · ${s('reason')}`
    case r.kind === 'tool.notified': return `notified · ${s('tool')} · ${s('summary')} · ${s('setting')}${g('granted') ? ` · 🔑 ${s('granted')}` : ''}`
    case r.kind === 'secret.granted': return `${g('program') ? `${s('program')} got ${s('variable')}` : `${s('tool')} got`} (${s('secret')}) · ${s('correlation_id')}`
    case r.kind === 'secret.withheld': return `${s('program')} got no ${s('variable')} (${s('secret')}): ${s('why')}`
    case r.kind === 'job.wrapper_lost': return `${s('tool')} · job ${s('correlation_id')} lost its wrapper (pid ${s('pid')}, signal ${s('signal')}) before it reported · outcome unknown`
    case r.kind === 'tool.confirm_requested': return `${s('tool')} · ${s('reason')}`
    case r.kind === 'tool.job_started': return `${JSON.stringify(g('argv') ?? [])} · pid ${s('pid')}`
    case r.kind === 'action.confirm_answered': return `${g('approved') ? 'approved' : 'declined'} by ${s('by')}${g('via') ? ` via ${s('via')}` : ''}${g('note') ? ` · ${s('note')}` : ''}`
    case r.kind === 'approval.refused': return `${g('act') === 'policy.untighten' ? 'undo of ' : g('act') === 'policy.tighten' ? 'should have asked for ' : ''}${s('tool')} · ${s('who')} via ${s('via')} did not count: ${s('why')}`
    case r.kind === 'policy.tightened': return `${s('tool')} asks first: tightened by ${s('by')} via ${s('via')}${g('correlation_id') ? ` · from ${s('correlation_id')}` : ''}${g('changed') === false ? ` · the config already asks (${s('config_setting')})` : ` · the config says ${s('config_posture')}`}`
    case r.kind === 'policy.untightened': return `${s('tool')} back to ${s('posture')} (${s('setting')}) · undone by ${s('by')} via ${s('via')} · tightened by ${s('tightened_by')}`
    case r.kind === 'discord.tighten': return `should have asked: ${s('tool')} by ${s('by')}${g('ok') ? '' : ` · failed: ${s('error')}`}`
    case r.kind === 'approval.channel_checked': return `${s('channel')} · ${g('trusted') ? 'trusted' : 'not trusted'}: ${s('detail')}`
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
