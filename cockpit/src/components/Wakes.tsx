// The wakes Theseus has set, moved here from Actions (theseus-p17fd): every field the old panel showed (the note, its
// short id, the session or task it belongs to, its state, when it is due and how long from now, when it was set and
// where its reply goes) and Cancel, and the series, pause and purpose it did not show. `wakeFacts` (lib/agenda.ts) is the
// one place the fields are worked out; test/agenda.test.ts holds them to the old panel's list.
import { Hourglass, OctagonX, Pause, Play } from 'lucide-react'
import { useNavigate } from 'react-router'
import type { WakeInfo } from '@protocol'
import { useAct } from '@/components/ConfirmCard'
import { Btn, Empty, StatePill } from '@/components/ui'
import { clock, short, span } from '@/lib/format'
import { wakeFacts } from '@/lib/agenda'

export function Wakes({ wakes, now }: { wakes: readonly WakeInfo[]; now: number }) {
  const nav = useNavigate()
  const { busy, run } = useAct()
  if (!wakes.length) return <Empty>no wakes set</Empty>
  return (
    <div className="flex flex-col gap-1.5" data-testid="wakes">
      {wakes.map((w) => {
        const f = wakeFacts(w, now, span, clock, short)
        return (
          <div key={f.wake_id} className="flex items-center gap-2 rounded-lg px-3 py-2 ring-1 ring-line">
            <Hourglass size={14} className="text-live" />
            <div className="min-w-0 flex-1" title={f.tip}>
              <div className="truncate text-[12.5px] text-ink">{f.note}</div>
              <div className="num text-[11px] text-ink-faint">
                {f.short} · <button onClick={() => nav(`/session/${f.session_id}`)} className="hover:text-live">{f.where}</button> <StatePill state={f.state} /> · {f.due_local} · <span className={f.overdue ? 'text-wait' : 'text-live'}>{f.relative}</span>
                {f.repeat && <> · {f.repeat}</>}{f.paused && <> · paused</>}
              </div>
            </div>
            {f.purpose === 'watch' || f.purpose === 'routine' ? null : f.paused
              ? <Btn tone="ok" busy={busy === `${f.wake_id}:resume`} onClick={() => run(`${f.wake_id}:resume`, 'wake.resume', { wake: f.wake_id })}><Play size={13} /> Resume</Btn>
              : <Btn tone="wait" busy={busy === `${f.wake_id}:pause`} onClick={() => run(`${f.wake_id}:pause`, 'wake.pause', { wake: f.wake_id })}><Pause size={13} /> Pause</Btn>}
            <Btn tone="fault" busy={busy === f.wake_id} onClick={() => run(f.wake_id, 'wake.cancel', { wake: f.wake_id }, `Cancel the wake “${f.note}”?`)}><OctagonX size={13} /> Cancel</Btn>
          </div>
        )
      })}
    </div>
  )
}
