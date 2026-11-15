// A button that acts on the Configuration page's More controls tab. Its posture is its verb's (`verbs.ts`): `do` presses
// once, `arm` the second press within four seconds, `ask` opens a sheet that says what will happen and does nothing until
// it is confirmed (and, for an act that cannot be taken back, until its name is typed). Every press is one protocol call;
// the daemon's own words come back on the page's list of notes (`notes.ts`), and in the sheet when it refuses there, and
// the page's reads are read again. Off under the time
// machine, as every act of the Operations page is.
import { useEffect, useRef, useState, type ReactNode } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { X } from 'lucide-react'
import { ShipsWheel } from '@/components/ShipsWheel'
import { call } from '@/lib/rpc'
import { useAsOf } from '@/lib/timemachine'
import { cn } from '@/lib/format'
import { PAST_WORDS } from '@/ops/act'
import { wordsOf } from '@/ops/refusal'
import { FIELD } from '@/ops/Sheet'
import { useNotes } from './notes'
import { configVerb } from './verbs'

export { FIELD }

const TONES = { live: 'text-live ring-live/40 hover:bg-live/10', fault: 'text-fault ring-fault/40 hover:bg-fault/10', plain: 'text-ink-dim ring-line hover:text-ink' } as const

interface Said { ok: boolean; text: string }

export function ActButton({ verb, params, label, effect, tone = 'live', disabled, confirmWord, submit, words, onDone, title }: {
  verb: string
  /** What the call carries; built by the control from its fields. */
  params: unknown
  label: ReactNode
  /** What will happen, in plain words: the sheet's text for an `ask` act, the arm's hint otherwise. */
  effect: string
  tone?: keyof typeof TONES
  disabled?: boolean
  /** An act that cannot be taken back: the name the person must type before the sheet's button works. */
  confirmWord?: string
  /** The sheet's button; default "Yes, do it". */
  submit?: string
  /** The line said after, from the daemon's answer; default "done". */
  words?: (result: any) => string
  onDone?: (result: any) => void
  title?: string
}) {
  const v = configVerb(verb)
  const qc = useQueryClient()
  const past = useAsOf((s) => s.t !== null)
  const [busy, setBusy] = useState(false)
  const [armed, setArmed] = useState(false)
  const [asking, setAsking] = useState(false)
  const [typed, setTyped] = useState('')
  const [said, setSaid] = useState<Said | null>(null)
  const note = useNotes((n) => n.add)
  const disarm = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  useEffect(() => () => clearTimeout(disarm.current), [])

  const go = async () => {
    setBusy(true); setSaid(null)
    try {
      const r = await call(v.methods[0], params)
      await qc.invalidateQueries()
      setSaid(null)
      note(true, `${v.label}: ${words ? words(r) : 'done'}`)
      onDone?.(r)
      setAsking(false); setTyped('')
    } catch (e) {
      setSaid({ ok: false, text: wordsOf(e) })
      // A refusal in a sheet is read there; one with no sheet has nowhere else to be said.
      if (v.posture !== 'ask') note(false, `${v.label}: ${wordsOf(e)}`)
    } finally { setBusy(false) }
  }
  const press = () => {
    if (v.posture === 'ask') { setSaid(null); setAsking(true); return }
    if (v.posture === 'arm' && !armed) {
      setArmed(true); setSaid(null)
      disarm.current = setTimeout(() => setArmed(false), 4000)
      return
    }
    clearTimeout(disarm.current); setArmed(false)
    void go()
  }
  const off = disabled || past || busy
  return (
    <>
      <button type="button" onClick={press} disabled={off} data-verb={verb} data-posture={v.posture}
        title={past ? PAST_WORDS : armed ? `Press again to confirm: ${effect}` : title ?? effect}
        className={cn('inline-flex items-center gap-1.5 rounded-md px-2.5 py-1 text-[12px] font-medium ring-1 ring-inset disabled:cursor-not-allowed disabled:opacity-40', TONES[tone], armed && 'bg-fault/10 text-fault ring-fault/50')}>
        {busy && <ShipsWheel />}{armed ? 'Press again to confirm' : label}
      </button>
      {asking && (
        <div className="fixed inset-0 z-40 grid place-items-start justify-center bg-black/50 pt-[12vh] backdrop-blur-sm" role="dialog" aria-modal aria-label={v.label}
          onMouseDown={(e) => { if (e.target === e.currentTarget && !busy) setAsking(false) }}
          onKeyDown={(e) => { if (e.key === 'Escape' && !busy) setAsking(false) }}>
          <div className="brass-card flex max-h-[76vh] w-[500px] max-w-[94vw] flex-col gap-3 overflow-auto p-4 shadow-2xl">
            <div className="flex items-center gap-2">
              <h2 className="text-[14px] font-semibold text-ink">{v.label}</h2>
              <button type="button" onClick={() => !busy && setAsking(false)} aria-label="close" title="close" className="ml-auto text-ink-faint hover:text-ink"><X size={14} /></button>
            </div>
            <p className="whitespace-pre-line text-[13px] leading-snug text-ink-dim">{effect}</p>
            {confirmWord && (
              <label className="flex flex-col gap-1 text-[12px] text-ink-faint">
                Type <span className="num text-ink">{confirmWord}</span> to confirm
                <input autoFocus value={typed} onChange={(e) => setTyped(e.target.value)} className={FIELD} spellCheck={false} />
              </label>
            )}
            {said && !said.ok && <div role="alert" className="text-[12px] text-fault">{said.text}</div>}
            <div className="flex items-center justify-end gap-2">
              <button type="button" onClick={() => setAsking(false)} disabled={busy} className="rounded-md px-2.5 py-1 text-[12px] text-ink-dim ring-1 ring-inset ring-line hover:text-ink">Cancel</button>
              <button type="button" onClick={() => void go()} disabled={busy || (!!confirmWord && typed.trim() !== confirmWord)} autoFocus={!confirmWord}
                className={cn('inline-flex items-center gap-1.5 rounded-md px-2.5 py-1 text-[12px] font-medium ring-1 ring-inset disabled:cursor-not-allowed disabled:opacity-40', TONES[tone === 'plain' ? 'live' : tone])}>
                {busy && <ShipsWheel />}{submit ?? 'Yes, do it'}
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  )
}
