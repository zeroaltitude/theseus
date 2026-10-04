// Continue a session from the cockpit: one turn per send. The reply streams into the transcript above, through
// the session's watch; this only submits. Enter sends, Shift+Enter is a new line. A send while a turn runs queues
// behind it, as the Observatory's did (theseus-vm3n.6): the turn is never refused, and the daemon admits it in order.
// Each send shows in the transcript as a draft until the session writes it (`lib/drafts.ts`).
import { useState } from 'react'
import { SendHorizontal } from 'lucide-react'
import type { ProfileList, ProviderErrorData } from '@protocol'
import { call, useRpc } from '@/lib/rpc'
import { addDraft, dropDraft, failDraft } from '@/lib/drafts'
import { cn } from '@/lib/format'

export function Composer({ sessionId, busy }: { sessionId: string; busy: boolean }) {
  const { data: pl } = useRpc<ProfileList>('profile.list', undefined, 30_000)
  const [text, setText] = useState('')
  const [profile, setProfile] = useState<string>('')
  const [error, setError] = useState<string | null>(null)
  // Sends in flight: each resolves when its turn ends, so a second send while the first runs is a queued turn.
  const [pending, setPending] = useState(0)
  const send = () => {
    const input = text.trim()
    if (!input) return
    setPending((n) => n + 1)
    setError(null)
    const draft = addDraft(sessionId, input)
    // The call resolves when the turn ends; the transcript follows it live meanwhile.
    call('turn.submit', { session_id: sessionId, input, profile: profile || undefined })
      .then(() => dropDraft(draft))
      .catch((e: any) => {
        const data = e?.data as ProviderErrorData | undefined
        // Never admitted (no turn): the draft says why, where it was.
        if (!data?.turn_id) { failDraft(draft, e?.message ?? String(e)); return }
        // A turn the provider failed says its class, and whether it may be tried again; the transcript shows the
        // turn's failure from its row.
        dropDraft(draft)
        setError(`${data.class ? `turn failed · class ${data.class}${data.transient ? ' · transient' : ' · permanent'}${data.usage_unknown ? ' · usage unknown (reservation held)' : ''}: ` : ''}${e?.message ?? String(e)}`)
      })
      .finally(() => setPending((n) => n - 1))
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
          placeholder={busy ? 'a turn is running… (a message you send now queues behind it)' : 'Continue this session… (Enter to send, Shift+Enter for a new line)'}
          className="min-h-9 flex-1 resize-none rounded-lg bg-white/[0.04] px-3 py-2 text-[13px] text-ink outline-none ring-1 ring-line placeholder:text-ink-faint focus:ring-live/40"
        />
        <select value={profile} onChange={(e) => setProfile(e.target.value)} title="profile for this turn"
          className="h-9 rounded-lg bg-white/[0.04] px-2 text-[12px] text-ink-dim outline-none ring-1 ring-line">
          <option value="">{pl ? `${pl.live} (live)` : 'live'}</option>
          {(pl?.profiles ?? []).filter((p) => !p.live).map((p) => <option key={p.name} value={p.name}>{p.name} · {p.model}</option>)}
        </select>
        <button onClick={send} disabled={!text.trim()} title={busy || pending > 0 ? 'Send (queues behind the running turn)' : 'Send'}
          className={cn('grid h-9 w-9 place-items-center rounded-lg ring-1 transition-colors',
            text.trim() ? 'bg-live/15 text-live ring-live/40 hover:bg-live/25' : 'text-ink-faint ring-line')}>
          <SendHorizontal size={16} />
        </button>
      </div>
    </div>
  )
}
