// Continue a session from the cockpit: one turn per send. The reply streams into the transcript above, through
// the session's watch; this only submits. Enter sends, Shift+Enter is a new line.
import { useState } from 'react'
import { SendHorizontal } from 'lucide-react'
import type { ProfileList } from '@protocol'
import { call, useRpc } from '@/lib/rpc'
import { cn } from '@/lib/format'

export function Composer({ sessionId, busy }: { sessionId: string; busy: boolean }) {
  const { data: pl } = useRpc<ProfileList>('profile.list', undefined, 30_000)
  const [text, setText] = useState('')
  const [profile, setProfile] = useState<string>('')
  const [error, setError] = useState<string | null>(null)
  const [sending, setSending] = useState(false)
  const send = () => {
    const input = text.trim()
    if (!input || busy || sending) return
    setSending(true)
    setError(null)
    // The call resolves when the turn ends; the transcript follows it live meanwhile.
    call('turn.submit', { session_id: sessionId, input, profile: profile || undefined })
      .catch((e: any) => setError(e?.message ?? String(e)))
      .finally(() => setSending(false))
    setText('')
  }
  return (
    <div className="border-t border-line bg-hull/80 p-2.5">
      {error && <div className="mb-1.5 rounded-md bg-fault/10 px-2.5 py-1 text-[12px] text-fault ring-1 ring-fault/30">{error}</div>}
      <div className="flex items-end gap-2">
        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send() } }}
          rows={Math.min(6, Math.max(1, text.split('\n').length))}
          placeholder={busy ? 'a turn is running…' : 'Continue this session… (Enter to send, Shift+Enter for a new line)'}
          className="min-h-9 flex-1 resize-none rounded-lg bg-white/[0.04] px-3 py-2 text-[13px] text-ink outline-none ring-1 ring-line placeholder:text-ink-faint focus:ring-live/40"
        />
        <select value={profile} onChange={(e) => setProfile(e.target.value)} title="profile for this turn"
          className="h-9 rounded-lg bg-white/[0.04] px-2 text-[12px] text-ink-dim outline-none ring-1 ring-line">
          <option value="">{pl ? `${pl.live} (live)` : 'live'}</option>
          {(pl?.profiles ?? []).filter((p) => !p.live).map((p) => <option key={p.name} value={p.name}>{p.name} · {p.model}</option>)}
        </select>
        <button onClick={send} disabled={!text.trim() || busy || sending}
          className={cn('grid h-9 w-9 place-items-center rounded-lg ring-1 transition-colors',
            text.trim() && !busy && !sending ? 'bg-live/15 text-live ring-live/40 hover:bg-live/25' : 'text-ink-faint ring-line')}>
          <SendHorizontal size={16} />
        </button>
      </div>
    </div>
  )
}
