import type { SessionInfo } from './protocol'

// Every session the store holds, most recently active first. Picking one
// resumes it: its whole history renders from the store and new turns continue
// it (the model's context is that history). "New" starts nothing until the
// first message: sessions are opened lazily, not per page load.
//
// A task session (DD7) nests under the session that started it, oldest
// first, with its state and its spend of its carved limit: the tree.

const ago = (ms: number, now: number) => {
  const s = Math.max(0, Math.round((now - ms) / 1000))
  return s < 60 ? `${s}s ago` : s < 3600 ? `${Math.round(s / 60)}m ago` : s < 86_400 ? `${Math.round(s / 3600)}h ago` : `${Math.round(s / 86_400)}d ago`
}
const money = (n: number) => n === 0 ? '$0' : n < 0.01 ? `$${n.toFixed(4)}` : `$${n.toFixed(3)}`

/** A task's state as a pill: what it is doing, or how it ended. */
const taskPill = (state: string | null | undefined) => {
  switch (state) {
    case 'running': return <span className="pill ok">running</span>
    case 'queued': return <span className="pill accent">queued</span>
    case 'waiting': return <span className="pill muted">waiting</span>
    case 'complete': return <span className="pill ok">done</span>
    case 'failed': return <span className="pill bad">failed</span>
    case 'cancelled': return <span className="pill muted">cancelled</span>
    default: return state ? <span className="pill muted">{state}</span> : null
  }
}

export default function Sessions({ sessions, current, onPick, onNew, now }: {
  sessions: SessionInfo[]; current: string | null
  onPick: (id: string) => void; onNew: () => void; now: number
}) {
  // Tasks go under their parent when it is listed; any other session, and a
  // task whose parent is gone, stays at the top.
  const listed = new Set(sessions.map((s) => s.session_id))
  const children = new Map<string, SessionInfo[]>()
  const top: SessionInfo[] = []
  for (const s of sessions) {
    const p = s.parent_session_id
    if (p && listed.has(p)) children.set(p, [...(children.get(p) ?? []), s])
    else top.push(s)
  }
  for (const kids of children.values()) kids.sort((a, b) => a.created_at_unix_ms - b.created_at_unix_ms)

  const item = (s: SessionInfo, task: boolean) => {
    const last = Math.max(s.last_active_ms ?? 0, s.created_at_unix_ms)
    const waiting = s.pending_confirms ?? 0
    return (
      <li key={s.session_id} className={`s-item ${task ? 'task' : ''} ${s.session_id === current ? 'on' : ''}`} onClick={() => onPick(s.session_id)}
        title={`${s.session_id}${s.model ? `\n${s.profile ?? ''} → ${s.model}` : ''}${s.execution_state ? `\nexecution ${s.execution_state}` : ''}`}>
        <div className="s-title">{task && <span className="muted">↳ task </span>}{s.title ?? s.label ?? <span className="muted">untitled</span>}</div>
        <div className="s-meta muted small">
          {ago(last, now)} · {s.turns} turn{s.turns === 1 ? '' : 's'}
          {(s.tool_calls ?? 0) > 0 && <> · {s.tool_calls} tool{s.tool_calls === 1 ? '' : 's'}</>}
          {' · '}{money(s.cost_usd ?? 0)}{task && s.limit_usd != null && <> of {money(s.limit_usd)}</>}
          {waiting > 0 && <span className="pill accent needs">needs you · {waiting}</span>}
          {task ? taskPill(s.execution_state) : <>
            {s.execution_state === 'running' && <span className="pill ok">running</span>}
            {s.execution_state === 'queued' && <span className="pill accent">queued</span>}
          </>}
        </div>
      </li>
    )
  }

  return (
    <nav className="sessions">
      <div className="sessions-head">
        <strong>Sessions</strong> <span className="muted">{sessions.length}</span>
        <button type="button" className="new" onClick={onNew} title="start a new session with your next message">+ new</button>
      </div>
      {current === null && <div className="s-item on draft"><div className="s-title">New session</div><div className="muted small">opens with your first message</div></div>}
      <ul>
        {top.map((s) => [item(s, !!s.parent_session_id), ...(children.get(s.session_id) ?? []).map((k) => item(k, true))])}
      </ul>
    </nav>
  )
}
