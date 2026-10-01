import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { KeyboardEvent } from 'react'
import { ProtocolClient } from './protocol'
import type {
  ConfirmRequest, Health, NodeInfo, ProfileList, ProviderErrorData, RpcError, SessionHistory, SessionInfo, Span, Status, TightenResult,
  TurnResult, Usage,
} from './protocol'
import Transcript from './Transcript'
import type { LiveTurn, TurnError } from './Transcript'
import Sessions from './Sessions'
import Observatory from './Observatory'
import Narrative from './Narrative'
import Logo from './Logo'
import './App.css'

const fmt = (n: number) => n.toLocaleString()
const money = (n: number | undefined) => n == null ? '' : n < 0.01 ? `$${n.toFixed(4)}` : `$${n.toFixed(3)}`
const SESSION_KEY = 'theseus.session'

function UsageLine({ u, prefix }: { u: Usage; prefix?: string }) {
  const cache = u.cache_read_input_tokens + u.cache_creation_input_tokens
  return (
    <span className="usage">
      {prefix}in <b>{fmt(u.input_tokens)}</b> · out <b>{fmt(u.output_tokens)}</b>
      {cache > 0 && <> · cache r{fmt(u.cache_read_input_tokens)} w{fmt(u.cache_creation_input_tokens)}</>}
    </span>
  )
}

type P = Record<string, unknown>
const s = (v: unknown) => typeof v === 'string' ? v : ''

export default function App() {
  // Served by the daemon, the page connects to its own address. The dev page connects straight to the daemon
  // named by THESEUS_DEV_DAEMON (vite.config.ts), never through a proxy (theseus-88im).
  const client = useMemo(
    () => new ProtocolClient(import.meta.env.DEV ? `ws://${import.meta.env.VITE_THESEUS_DEV_DAEMON as string}/ws` : undefined),
    [],
  )
  const [status, setStatus] = useState<Status>('connecting')
  const [health, setHealth] = useState<Health | null>(null)
  const [profiles, setProfiles] = useState<ProfileList | null>(null)
  const [sessions, setSessions] = useState<SessionInfo[]>([])
  const [current, setCurrent] = useState<string | null>(() => localStorage.getItem(SESSION_KEY))
  const [nodes, setNodes] = useState<NodeInfo[]>([])
  const [pending, setPending] = useState<ConfirmRequest[]>([])
  const [sessionInfo, setSessionInfo] = useState<SessionInfo | null>(null)
  const [live, setLive] = useState<Record<string, LiveTurn & { sessionId: string | null }>>({})
  const [results, setResults] = useState<Record<string, TurnResult>>({})
  const [errors, setErrors] = useState<Record<string, TurnError>>({})
  const [traces, setTraces] = useState<Record<string, Span | null>>({})
  const [draft, setDraft] = useState<{ text: string; turnId: string | null; error: TurnError | null } | null>(null)
  const [input, setInput] = useState('')
  const [showObs, setShowObs] = useState(() => localStorage.getItem('theseus.obs') !== 'off')
  const [showSessions, setShowSessions] = useState(() => localStorage.getItem('theseus.sidebar') !== 'off')
  // The side pane's tab: the Observatory, or The Narrative when the daemon narrates.
  const [pane, setPane] = useState<'observatory' | 'narrative'>(() => localStorage.getItem('theseus.pane') === 'narrative' ? 'narrative' : 'observatory')
  const [tick, setTick] = useState(0)
  const [now, setNow] = useState(Date.now())
  const [loadError, setLoadError] = useState<string | null>(null)
  const currentRef = useRef<string | null>(current)
  const draftRef = useRef(draft)
  const historyTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const sessionsTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const bottom = useRef<HTMLDivElement>(null)
  const textarea = useRef<HTMLTextAreaElement>(null)

  useEffect(() => { currentRef.current = current }, [current])
  useEffect(() => { draftRef.current = draft }, [draft])
  useEffect(() => { const id = setInterval(() => setNow(Date.now()), 1000); return () => clearInterval(id) }, [])

  const refreshHealth = useCallback(async () => {
    try {
      setHealth(await client.call<Health>('health'))
      setProfiles(await client.call<ProfileList>('profile.list'))
    } catch { /* shown by status */ }
  }, [client])

  const refreshSessions = useCallback(async () => {
    try { setSessions((await client.call<{ sessions: SessionInfo[] }>('session.list')).sessions) } catch { /* status */ }
  }, [client])
  const scheduleSessions = useCallback(() => {
    if (sessionsTimer.current) clearTimeout(sessionsTimer.current)
    sessionsTimer.current = setTimeout(() => void refreshSessions(), 150)
  }, [refreshSessions])

  const loadHistory = useCallback(async (sid: string) => {
    try {
      const h = await client.call<SessionHistory>('session.history', { session_id: sid })
      if (currentRef.current !== sid) return
      setNodes(h.nodes); setPending(h.pending_confirms); setSessionInfo(h.session); setLoadError(null)
      // The draft bubble goes once its user node is in the store.
      const d = draftRef.current
      if (d?.turnId && h.nodes.some((n) => n.turn_id === d.turnId && n.kind === 'user_message')) setDraft(null)
    } catch (err) {
      const e = err as RpcError
      if (e.code === -32002) {
        // The session is gone (a different store, or deleted): start fresh.
        localStorage.removeItem(SESSION_KEY); setCurrent(null); setNodes([]); setPending([]); setSessionInfo(null)
      } else setLoadError(e.message)
    }
  }, [client])
  const scheduleHistory = useCallback(() => {
    if (historyTimer.current) clearTimeout(historyTimer.current)
    historyTimer.current = setTimeout(() => { const sid = currentRef.current; if (sid) void loadHistory(sid) }, 80)
  }, [loadHistory])

  const watch = useCallback(async (sid: string) => {
    try { await client.call('session.watch', { session_id: sid }) } catch { /* reconnect re-watches */ }
  }, [client])

  const pick = useCallback((sid: string | null) => {
    const prev = currentRef.current
    if (prev === sid) return
    if (prev) void client.call('session.unwatch', { session_id: prev }).catch(() => {})
    currentRef.current = sid
    setCurrent(sid)
    setNodes([]); setPending([]); setSessionInfo(null); setDraft(null)
    if (sid) {
      localStorage.setItem(SESSION_KEY, sid)
      void watch(sid); void loadHistory(sid)
    } else localStorage.removeItem(SESSION_KEY)
    setTick((t) => t + 1)
    textarea.current?.focus()
  }, [client, loadHistory, watch])

  const switchProfile = useCallback(async (name: string) => {
    try {
      await client.call('profile.use', { name })
      await refreshHealth()
    } catch (e) { console.error(e) }
  }, [client, refreshHealth])

  // Connect (and reconnect); on every open, re-watch the current session and reload it.
  useEffect(() => {
    client.onStatus = setStatus
    const offOpen = client.onOpen(() => {
      void refreshHealth(); void refreshSessions()
      const sid = currentRef.current
      if (sid) { void watch(sid); void loadHistory(sid) }
    })
    const offNotify = client.onNotify((method, params) => {
      const p = (params ?? {}) as P
      const turnId = s(p.turn_id)
      switch (method) {
        case 'profile.changed': void refreshHealth(); return
        case 'turn.started': {
          const sid = s(p.session_id)
          // Our own first message in a new session: this is where we learn its id.
          if (!currentRef.current && draftRef.current && !p.continuation) {
            currentRef.current = sid; setCurrent(sid); localStorage.setItem(SESSION_KEY, sid); void watch(sid)
          }
          if (draftRef.current && !draftRef.current.turnId && !p.continuation && sid === currentRef.current) {
            setDraft((d) => d && { ...d, turnId })
          }
          setLive((l) => ({ ...l, [turnId]: { turnId, sessionId: sid, continuation: !!p.continuation, startedAt: Date.now(), loops: {}, running: {}, compiles: [] } }))
          scheduleSessions()
          return
        }
        case 'model.delta':
        case 'model.thinking': {
          const loop = Number(p.loop_index ?? 0)
          const text = s(p.text)
          setLive((l) => {
            const t = l[turnId] ?? { turnId, sessionId: currentRef.current, continuation: false, startedAt: Date.now(), loops: {}, running: {}, compiles: [] }
            const b = t.loops[loop] ?? { text: '', thinking: '' }
            const nb = method === 'model.delta' ? { ...b, text: b.text + text } : { ...b, thinking: b.thinking + text }
            return { ...l, [turnId]: { ...t, loops: { ...t.loops, [loop]: nb } } }
          })
          return
        }
        case 'tool.started':
          setLive((l) => l[turnId] ? { ...l, [turnId]: { ...l[turnId], running: { ...l[turnId].running, [s(p.tool_use_id)]: { tool: s(p.tool), startedAt: Date.now(), argv: p.argv as string[] | undefined, backend: s(p.backend) } } } } : l)
          return
        case 'tool.ended':
          setLive((l) => {
            if (!l[turnId]) return l
            const running = { ...l[turnId].running }; delete running[s(p.tool_use_id)]
            return { ...l, [turnId]: { ...l[turnId], running } }
          })
          return
        case 'context.compiled':
          setLive((l) => l[turnId] ? { ...l, [turnId]: { ...l[turnId], compiles: [...l[turnId].compiles, p] } } : l)
          return
        case 'node.written': scheduleHistory(); return
        case 'confirm.requested':
        case 'confirm.resolved': scheduleHistory(); scheduleSessions(); return
        // "Should have asked" (theseus-sgh): health lists the tightenings.
        case 'policy.tightened':
        case 'policy.untightened': void refreshHealth(); setTick((t) => t + 1); return
        case 'turn.ended':
          setResults((r) => ({ ...r, [turnId]: params as TurnResult }))
          scheduleHistory(); scheduleSessions(); void refreshHealth(); setTick((t) => t + 1)
          return
        case 'turn.failed': {
          const key = turnId || `failed:${Date.now()}`
          setErrors((e) => ({ ...e, [key]: { message: s(p.error), class: s(p.class) || null } }))
          scheduleHistory(); scheduleSessions(); void refreshHealth(); setTick((t) => t + 1)
          return
        }
      }
    })
    client.connect()
    return () => { offOpen(); offNotify(); client.close() }
  }, [client, loadHistory, refreshHealth, refreshSessions, scheduleHistory, scheduleSessions, watch])

  // Sessions change under other clients too (CLI, the driver): keep the list fresh.
  useEffect(() => {
    if (status !== 'open') return
    const id = setInterval(() => void refreshSessions(), 5000)
    return () => clearInterval(id)
  }, [status, refreshSessions])

  useEffect(() => { bottom.current?.scrollIntoView({ behavior: 'smooth' }) }, [nodes.length, live, draft, pending.length])

  const submit = useCallback(async () => {
    const prompt = input.trim()
    if (!prompt || status !== 'open') return
    setInput('')
    setDraft({ text: prompt, turnId: null, error: null })
    try {
      const result = await client.call<TurnResult>('turn.submit', { session_id: currentRef.current ?? undefined, input: prompt })
      setResults((r) => ({ ...r, [result.turn_id]: result }))
      if (!currentRef.current) pick(result.session_id)
      else void loadHistory(currentRef.current)
    } catch (err) {
      const e = err as RpcError
      const data = e.data as ProviderErrorData | undefined
      if (data?.turn_id) {
        setErrors((x) => ({ ...x, [data.turn_id!]: { message: e.message, data } }))
        if (currentRef.current) void loadHistory(currentRef.current)
      } else setDraft((d) => d && { ...d, error: { message: e.message, data } })
    } finally {
      void refreshHealth(); void refreshSessions(); setTick((t) => t + 1)
      textarea.current?.focus()
    }
  }, [client, input, loadHistory, pick, refreshHealth, refreshSessions, status])

  const onConfirm = useCallback(async (correlationId: string, approve: boolean, note: string, trust?: boolean) => {
    // `trust` (theseus-9bp): approve, and trust the session again.
    await client.call('action.confirm', { correlation_id: correlationId, approve, note: note || undefined, trust: trust || undefined })
    scheduleHistory(); scheduleSessions(); setTick((t) => t + 1)
    if (trust) void refreshHealth()
  }, [client, scheduleHistory, scheduleSessions, refreshHealth])

  // "Should have asked" on a notice (theseus-sgh): the tool asks first from now on.
  const onTighten = useCallback(async (tool: string, correlationId: string) => {
    const r = await client.call<TightenResult>('policy.tighten', { tool, correlation_id: correlationId || undefined })
    void refreshHealth(); setTick((t) => t + 1)
    return r
  }, [client, refreshHealth])
  const tightened = useMemo(() => Object.fromEntries((health?.tightenings ?? []).map((t) => [t.tool, t])), [health])

  const loadTrace = useCallback(async (turnId: string) => {
    const sid = currentRef.current
    if (!sid) return
    try {
      const t = await client.call<{ rows: { turn_id: string | null; data: unknown }[] }>('ledger.tail', { kind: 'turn.trace', session_id: sid, n: 500 })
      const row = t.rows.find((r) => r.turn_id === turnId)
      setTraces((x) => ({ ...x, [turnId]: (row?.data as Span | undefined) ?? null }))
    } catch { setTraces((x) => ({ ...x, [turnId]: null })) }
  }, [client])

  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void submit() }
  }

  const waiting = sessions.reduce((n, x) => n + (x.pending_confirms ?? 0), 0)
  const firstWaiting = sessions.find((x) => (x.pending_confirms ?? 0) > 0)
  const liveHere = Object.fromEntries(Object.entries(live).filter(([, t]) => t.sessionId === current))
  const info = sessionInfo ?? sessions.find((x) => x.session_id === current) ?? null
  const busy = draft !== null && !draft.error
  const narrative = !!health?.narrative
  const choosePane = (p: 'observatory' | 'narrative') => { localStorage.setItem('theseus.pane', p); setPane(p) }
  const sessionLabel = (id: string) => { const x = sessions.find((y) => y.session_id === id); return x?.title ?? x?.label ?? null }
  const observatory = (
    <Observatory client={client} health={health} tick={tick} currentSession={current}
      onRefresh={refreshHealth} onPickSession={(id) => pick(id)}
      onCancelled={() => { void refreshHealth(); setTick((t) => t + 1) }} />
  )

  return (
    <div className="app">
      <header>
        <div className="brand">
          <button type="button" className="link sidebar-toggle" title="sessions"
            onClick={() => setShowSessions((v) => { localStorage.setItem('theseus.sidebar', v ? 'off' : 'on'); return !v })}>☰</button>
          <Logo /> <strong>Theseus</strong>
          {health && <span className="muted"> v{health.version}</span>}
          <a className="new-experience" href="/cockpit/" title="The cockpit: the same daemon, a richer view (theseus-45n5)">see the new experience →</a>
        </div>
        {profiles && (
          <label className="profile" title={`live profile (from ${profiles.live_source}); persists across restarts`}>
            <span className="muted">live</span>
            <select value={profiles.live} onChange={(e) => void switchProfile(e.target.value)}>
              {profiles.profiles.map((p) => (
                <option key={p.name} value={p.name}>{p.name} · {p.provider}/{p.model}</option>
              ))}
            </select>
          </label>
        )}
        <div className={`status ${status}`}>{status === 'connecting' && health ? 'reconnecting' : status}</div>
        {waiting > 0 && firstWaiting && (
          <button type="button" className="needs-you" onClick={() => pick(firstWaiting.session_id)} title="tool calls waiting for your confirmation">
            {waiting} waiting for you
          </button>
        )}
        {health && (
          <div className="totals">
            <span>sessions {health.sessions}</span>
            <span>turns {health.turns}</span>
            {health.cost_usd_total != null && <span title={`catalog ${health.catalog_version ?? ''}`}>spent <b>{money(health.cost_usd_total)}</b></span>}
            <span className={health.provider_errors ? 'warn' : ''}>provider errors {health.provider_errors}</span>
            {health.kernel && <span title="kernel: turns held / admission ceiling">turns held {health.kernel.turns_held}/{health.kernel.admission_ceiling}</span>}
            <UsageLine u={health.usage_total} prefix="total " />
            <button type="button" className="link"
              title={narrative
                ? 'the side pane: the Observatory (context, nodes, tools, executions, actions, ledger — live from the store) and The Narrative (each step, as it happens)'
                : 'the Observatory: context, nodes, tools, executions, actions, ledger — live from the store'}
              onClick={() => setShowObs((v) => { localStorage.setItem('theseus.obs', v ? 'off' : 'on'); return !v })}>
              {showObs ? (narrative ? 'hide pane' : 'hide observatory') : (narrative ? 'observatory · narrative' : 'observatory')}
            </button>
          </div>
        )}
      </header>

      <div className={`body ${showObs ? 'with-obs' : ''} ${showSessions ? 'with-sessions' : ''}`}>
        {showSessions && <Sessions sessions={sessions} current={current} onPick={(id) => pick(id)} onNew={() => pick(null)} now={now} />}
        <main>
          {info && (
            <div className="session-bar muted small" title={info.session_id}>
              <b className="title">{info.title ?? info.label ?? 'untitled'}</b>
              <span>{info.turns} turn{info.turns === 1 ? '' : 's'}</span>
              {(info.tool_calls ?? 0) > 0 && <span>{info.tool_calls} tool calls</span>}
              <span>{money(info.cost_usd ?? 0)}</span>
              {info.model && <span>{info.profile} → {info.model}</span>}
              {info.execution_state && <span>execution {info.execution_state}</span>}
              <code>{info.session_id}</code>
            </div>
          )}
          {loadError && <div className="error">could not load the session: {loadError}</div>}
          {nodes.length === 0 && !draft && Object.keys(liveHere).length === 0 && (
            <div className="empty">
              {current ? ((info?.turns ?? 0) > 0
                ? <>This session's {info!.turns} turn{info!.turns === 1 ? '' : 's'} ran before Theseus kept what was said (conversation content is stored from M3 on, 2026-09-26 15:58).
                    Only their numbers survive: timings, tokens, and any error are in the Observatory's ledger with <i>this session</i> checked.</>
                : 'This session has no messages yet.') : <>
                A new session opens with your first message. Theseus can read, search, and diff files under the workspace
                roots on its own; writing files and running commands wait for your confirmation, right here.
                Pick an earlier session on the left to resume it — its whole history is the model's context.
              </>}
            </div>
          )}
          <Transcript nodes={nodes} pending={pending} live={liveHere} results={results} errors={errors} traces={traces}
            onConfirm={onConfirm} onLoadTrace={(id) => void loadTrace(id)} now={now}
            tightened={tightened} onTighten={onTighten} />
          {draft && (
            <section className="exchange">
              <div className="prompt"><pre>{draft.text}</pre></div>
              {draft.error ? (
                <div className="reply failed"><div className="error"><strong>turn failed</strong>
                  {draft.error.data && <> · class <code>{draft.error.data.class}</code></>}<div>{draft.error.message}</div></div></div>
              ) : !draft.turnId && <div className="reply"><span className="muted small">waiting for admission…</span></div>}
            </section>
          )}
          <div ref={bottom} />
        </main>
        {showObs && status === 'open' && (narrative ? (
          <div className="side">
            <nav className="tabs">
              <button type="button" className={`tab ${pane === 'observatory' ? 'on' : ''}`} onClick={() => choosePane('observatory')}>Observatory</button>
              <button type="button" className={`tab ${pane === 'narrative' ? 'on' : ''}`} onClick={() => choosePane('narrative')}>The Narrative</button>
            </nav>
            {pane === 'narrative'
              ? <Narrative client={client} currentSession={current} sessionLabel={sessionLabel} onPickSession={(id) => pick(id)} />
              : observatory}
          </div>
        ) : observatory)}
      </div>

      <form className="composer" onSubmit={(e) => { e.preventDefault(); void submit() }}>
        <textarea
          ref={textarea}
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={onKey}
          placeholder={status === 'open'
            ? (current ? 'Continue this session… (Enter to send, Shift+Enter for a new line)' : 'Start a new session… (Enter to send, Shift+Enter for a new line)')
            : 'not connected — reconnecting…'}
          disabled={status !== 'open'}
          rows={3}
          autoFocus
        />
        <div className="composer-side">
          <button type="submit" disabled={status !== 'open' || !input.trim()}>{busy ? 'Send (queues)' : 'Send'}</button>
          {info ? <div className="session muted" title={info.session_id}>
            {info.turns} turn{info.turns === 1 ? '' : 's'} · {money(info.cost_usd ?? 0)} · <UsageLine u={info.usage} />
          </div> : <div className="session muted">new session</div>}
        </div>
      </form>
    </div>
  )
}
