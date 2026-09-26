import type { SessionInfo } from './protocol'

// Every session the store holds, most recently active first. Picking one
// resumes it: its whole history renders from the store and new turns continue
// it (the model's context is that history). "New" starts nothing until the
// first message: sessions are opened lazily, not per page load.

const ago = (ms: number, now: number) => {
  const s = Math.max(0, Math.round((now - ms) / 1000))
  return s < 60 ? `${s}s ago` : s < 3600 ? `${Math.round(s / 60)}m ago` : s < 86_400 ? `${Math.round(s / 3600)}h ago` : `${Math.round(s / 86_400)}d ago`
}
const money = (n: number) => n === 0 ? '$0' : n < 0.01 ? `$${n.toFixed(4)}` : `$${n.toFixed(3)}`

export default function Sessions({ sessions, current, onPick, onNew, now }: {
  sessions: SessionInfo[]; current: string | null
  onPick: (id: string) => void; onNew: () => void; now: number
}) {
  return (
    <nav className="sessions">
      <div className="sessions-head">
        <strong>Sessions</strong> <span className="muted">{sessions.length}</span>
        <button type="button" className="new" onClick={onNew} title="start a new session with your next message">+ new</button>
      </div>
      {current === null && <div className="s-item on draft"><div className="s-title">New session</div><div className="muted small">opens with your first message</div></div>}
      <ul>
        {sessions.map((s) => {
          const last = Math.max(s.last_active_ms ?? 0, s.created_at_unix_ms)
          const waiting = s.pending_confirms ?? 0
          return (
            <li key={s.session_id} className={`s-item ${s.session_id === current ? 'on' : ''}`} onClick={() => onPick(s.session_id)}
              title={`${s.session_id}${s.model ? `\n${s.profile ?? ''} → ${s.model}` : ''}${s.execution_state ? `\nexecution ${s.execution_state}` : ''}`}>
              <div className="s-title">{s.title ?? s.label ?? <span className="muted">untitled</span>}</div>
              <div className="s-meta muted small">
                {ago(last, now)} · {s.turns} turn{s.turns === 1 ? '' : 's'}
                {(s.tool_calls ?? 0) > 0 && <> · {s.tool_calls} tool{s.tool_calls === 1 ? '' : 's'}</>}
                {' · '}{money(s.cost_usd ?? 0)}
                {waiting > 0 && <span className="pill accent needs">needs you · {waiting}</span>}
                {s.execution_state === 'running' && <span className="pill ok">running</span>}
                {s.execution_state === 'queued' && <span className="pill accent">queued</span>}
              </div>
            </li>
          )
        })}
      </ul>
    </nav>
  )
}
