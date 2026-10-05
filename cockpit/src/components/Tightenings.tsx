// The tightenings with their undo, one list for the Boundaries board's gate and the Policy view (M7 42b): each tool that
// asks first because someone pressed “Make actions like this ask in the future”, who pressed it and where, and the undo,
// confirmed first. `disabled` (the time machine's past) turns the undo off.
import { useState } from 'react'
import { CircleCheck, Lock, Undo2 } from 'lucide-react'
import type { LedgerEntry, Tightening } from '@protocol'
import { ago, stamp } from '@/lib/format'
import { Btn } from '@/components/ui'
import { useAct } from '@/components/ConfirmCard'

export function Tightenings({ tight, title, now, onOpen, log, disabled }: {
  tight: readonly Tightening[]
  title: (s?: string | null) => string
  now: number
  onOpen: (sid: string, cid?: string) => void
  /** The recent `policy.tightened` and `policy.untightened` rows, newest first. */
  log?: readonly LedgerEntry[]
  disabled?: boolean
}) {
  const { busy, run } = useAct()
  // What the last undo here did, as the Observatory's policy note said it.
  const [note, setNote] = useState<string | null>(null)
  return (
    <div className="flex flex-col gap-2" data-tightenings>
      {note && <div className="flex items-center gap-1.5 px-1 text-[11.5px] text-live"><CircleCheck size={12} /> {note}</div>}
      {!tight.length && <div className="px-1 text-[12px] text-ink-faint">no tool asks first beyond its configured posture</div>}
      {tight.map((t) => (
        <div key={t.tool} className="flex items-center gap-3 rounded-lg px-3 py-1.5 ring-1 ring-line" data-tightened={t.tool}>
          <Lock size={14} className="text-wait" />
          <div className="min-w-0 flex-1">
            <div className="text-[12.5px]"><span className="num text-tool">{t.tool}</span> <span className="text-ink-dim">asks first ({t.posture})</span></div>
            <div className="num text-[11px] text-ink-faint">
              pressed by {t.by} via {t.via} · {ago(t.at_ms, now)}
              {t.session_id && <> · in <button type="button" onClick={() => onOpen(t.session_id!, t.correlation_id ?? undefined)} title={`open the session it was pressed in (${t.session_id})`}
                className="underline decoration-dotted underline-offset-2 hover:text-live">{title(t.session_id)}</button></>}
            </div>
          </div>
          {disabled
            ? <span className="num text-[11px] text-ink-faint">the undo is off in the past</span>
            : <Btn tone="ok" busy={busy === t.tool} onClick={async () => { if (await run(t.tool, 'policy.untighten', { tool: t.tool }, `Let ${t.tool} go back to its configured posture?`)) setNote(`${t.tool} is back to what the config says`) }}><Undo2 size={12} /> Undo</Btn>}
        </div>
      ))}
      {!!log?.length && (
        <div className="mt-0.5">
          {log.map((r) => {
            const d = (r.data ?? {}) as { tool?: string; by?: string }
            return (
              <div key={r.position} className="num flex items-baseline gap-2 py-[1px] text-[11px]">
                <span className="w-28 shrink-0 text-ink-faint">{stamp(r.at_unix_ms)}</span>
                <span className={r.kind === 'policy.tightened' ? 'text-wait' : 'text-ok'}>{r.kind === 'policy.tightened' ? 'tightened' : 'undone'}</span>
                <span className="truncate text-ink-dim">{d.tool} · by {d.by}</span>
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}
