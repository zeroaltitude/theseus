import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { KeyboardEvent } from 'react'
import { ProtocolClient } from './protocol'
import type { Health, ProfileList, ProviderErrorData, RpcError, SessionInfo, TurnResult, Usage } from './protocol'
import TraceView from './TraceView'
import Logo from './Logo'
import './App.css'

// One exchange: a prompt, the streamed reply, the events that produced it,
// and the turn result (tokens, timing, model). Thinking and tool calls will
// render into `events` as the harness grows; the shape is ready for them.
interface Exchange {
  key: number
  prompt: string
  reply: string
  turnId: string | null
  result: TurnResult | null
  error: { message: string; data?: ProviderErrorData } | null
  events: { at: number; method: string; params: unknown }[]
  startedAt: number
  pending: boolean
  showTrace: boolean
}

type Status = 'connecting' | 'open' | 'closed'

const fmt = (n: number) => n.toLocaleString()

function UsageLine({ u, prefix }: { u: Usage; prefix?: string }) {
  const cache = u.cache_read_input_tokens + u.cache_creation_input_tokens
  return (
    <span className="usage">
      {prefix}in <b>{fmt(u.input_tokens)}</b> · out <b>{fmt(u.output_tokens)}</b>
      {cache > 0 && <> · cache r{fmt(u.cache_read_input_tokens)} w{fmt(u.cache_creation_input_tokens)}</>}
    </span>
  )
}

export default function App() {
  const client = useMemo(() => new ProtocolClient(), [])
  const [status, setStatus] = useState<Status>('connecting')
  const [health, setHealth] = useState<Health | null>(null)
  const [profiles, setProfiles] = useState<ProfileList | null>(null)
  const [session, setSession] = useState<SessionInfo | null>(null)
  const [exchanges, setExchanges] = useState<Exchange[]>([])
  const [input, setInput] = useState('')
  const [showEvents, setShowEvents] = useState(false)
  const nextKey = useRef(1)
  const bottom = useRef<HTMLDivElement>(null)
  const textarea = useRef<HTMLTextAreaElement>(null)

  const refreshHealth = useCallback(async () => {
    try {
      setHealth(await client.call<Health>('health'))
      setProfiles(await client.call<ProfileList>('profile.list'))
    } catch { /* shown by status */ }
  }, [client])

  const useProfile = useCallback(async (name: string) => {
    try {
      await client.call('profile.use', { name })
      setProfiles(await client.call<ProfileList>('profile.list'))
      setHealth(await client.call<Health>('health'))
    } catch (e) { console.error(e) }
  }, [client])

  // Connect, open a session, subscribe to notifications.
  useEffect(() => {
    client.onStatus = setStatus
    ;(async () => {
      try {
        await client.connect()
        await refreshHealth()
        setSession(await client.call<SessionInfo>('session.open', { label: 'web' }))
      } catch (e) {
        console.error(e)
      }
    })()
    const unsub = client.onNotify((method, params) => {
      if (method === 'profile.changed') { void refreshHealth(); return }
      const p = params as { turn_id?: string; text?: string; session_id?: string }
      setExchanges((xs) => {
        // Route by turn_id once known, else to the newest pending exchange.
        const idx = xs.findIndex((x) => x.pending && (x.turnId === null || x.turnId === p.turn_id))
        if (idx < 0) return xs
        const x = { ...xs[idx], events: [...xs[idx].events, { at: Date.now(), method, params }] }
        if (method === 'turn.started' && p.turn_id) x.turnId = p.turn_id
        if (method === 'model.delta' && p.text) x.reply = x.reply + p.text
        const out = xs.slice(); out[idx] = x; return out
      })
    })
    return () => { unsub(); client.close() }
  }, [client, refreshHealth])

  useEffect(() => { bottom.current?.scrollIntoView({ behavior: 'smooth' }) }, [exchanges])

  const submit = useCallback(async () => {
    const prompt = input.trim()
    if (!prompt || status !== 'open') return
    setInput('')
    const key = nextKey.current++
    setExchanges((xs) => [...xs, {
      key, prompt, reply: '', turnId: null, result: null, error: null, events: [], startedAt: Date.now(), pending: true, showTrace: false,
    }])
    try {
      const result = await client.call<TurnResult>('turn.submit', { session_id: session?.session_id, input: prompt })
      setExchanges((xs) => xs.map((x) => x.key === key ? { ...x, result, reply: result.output, pending: false } : x))
    } catch (err) {
      const e = err as RpcError
      setExchanges((xs) => xs.map((x) => x.key === key
        ? { ...x, error: { message: e.message, data: e.data as ProviderErrorData | undefined }, pending: false }
        : x))
    } finally {
      refreshHealth()
      if (session) {
        client.call<{ sessions: SessionInfo[] }>('session.list')
          .then((l) => setSession(l.sessions.find((s) => s.session_id === session.session_id) ?? session))
          .catch(() => {})
      }
      textarea.current?.focus()
    }
  }, [client, input, refreshHealth, session, status])

  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void submit() }
  }

  return (
    <div className="app">
      <header>
        <div className="brand">
          <Logo /> <strong>Theseus</strong>
          {health && <span className="muted"> v{health.version}</span>}
        </div>
        {profiles && (
          <label className="profile" title={`live profile (from ${profiles.live_source}); persists across restarts`}>
            <span className="muted">live</span>
            <select value={profiles.live} onChange={(e) => void useProfile(e.target.value)}>
              {profiles.profiles.map((p) => (
                <option key={p.name} value={p.name}>{p.name} · {p.provider}/{p.model}</option>
              ))}
            </select>
          </label>
        )}
        <div className={`status ${status}`}>{status}</div>
        {health && (
          <div className="totals">
            <span>sessions {health.sessions}</span>
            <span>turns {health.turns}</span>
            <span className={health.provider_errors ? 'warn' : ''}>provider errors {health.provider_errors}</span>
            <UsageLine u={health.usage_total} prefix="total " />
          </div>
        )}
      </header>

      <main>
        {exchanges.length === 0 && (
          <div className="empty">
            One prompt, one loop, one reply. Every hook site is visited; nothing fires yet.
            {session && <div className="muted">session {session.session_id}</div>}
          </div>
        )}
        {exchanges.map((x) => (
          <section key={x.key} className={`exchange ${x.error ? 'failed' : ''}`}>
            <div className="prompt"><pre>{x.prompt}</pre></div>
            <div className="reply">
              <pre>{x.reply}{x.pending && <span className="cursor">▍</span>}</pre>
              {x.error && (
                <div className="error">
                  <strong>turn failed</strong>
                  {x.error.data && <> · class <code>{x.error.data.class}</code>
                    {x.error.data.transient ? ' · transient' : ' · permanent'}
                    {x.error.data.usage_unknown && ' · usage unknown (reservation held)'}</>}
                  <div>{x.error.message}</div>
                </div>
              )}
              <footer>
                {x.result ? (
                  <>
                    <span title="profile → provider/model">{x.result.profile} → {x.result.provider}/{x.result.model}</span>
                    <span>{x.result.loops} loop{x.result.loops === 1 ? '' : 's'}</span>
                    <span>{x.result.stop_reason}</span>
                    <UsageLine u={x.result.usage} />
                    <button type="button" className="link" title="show the turn's full timing tree"
                      onClick={() => setExchanges((xs) => xs.map((y) => y.key === x.key ? { ...y, showTrace: !y.showTrace } : y))}>
                      {fmt(x.result.elapsed_ms)} ms{x.result.first_token_ms != null && <> · first token {fmt(x.result.first_token_ms)} ms</>} ▾
                    </button>
                    {x.result.request_id && <span className="muted" title="provider request id">{x.result.request_id}</span>}
                  </>
                ) : x.pending ? <span className="muted">running…</span> : null}
                {x.events.length > 0 && (
                  <button type="button" className="link" onClick={() => setShowEvents((v) => !v)}>
                    {x.events.length} events
                  </button>
                )}
              </footer>
              {x.showTrace && x.result?.trace && <TraceView root={x.result.trace} />}
              {x.error?.data?.trace && (
                <details className="trace-details"><summary className="muted">timing up to the failure</summary><TraceView root={x.error.data.trace} /></details>
              )}
              {showEvents && x.events.length > 0 && (
                <ol className="events">
                  {x.events.filter((e) => e.method !== 'model.delta').map((e, i) => (
                    <li key={i}><code>{e.method}</code> <span className="muted">{JSON.stringify(e.params)}</span></li>
                  ))}
                  <li className="muted">{x.events.filter((e) => e.method === 'model.delta').length} model.delta frames</li>
                </ol>
              )}
            </div>
          </section>
        ))}
        <div ref={bottom} />
      </main>

      <form className="composer" onSubmit={(e) => { e.preventDefault(); void submit() }}>
        <textarea
          ref={textarea}
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={onKey}
          placeholder={status === 'open' ? 'Ask Theseus… (Enter to send, Shift+Enter for a new line)' : 'not connected'}
          disabled={status !== 'open'}
          rows={3}
          autoFocus
        />
        <div className="composer-side">
          <button type="submit" disabled={status !== 'open' || !input.trim()}>Submit</button>
          {session && <div className="session muted" title={session.session_id}>
            {session.turns} turn{session.turns === 1 ? '' : 's'} · <UsageLine u={session.usage} />
          </div>}
        </div>
      </form>
    </div>
  )
}
