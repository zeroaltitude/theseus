// What Theseus does on its own, the controls: say whether the last thing it said first was welcome (`initiative.rate`, with
// the week's measure from `initiative.week` beside it), and pause or resume its self-improvement (`self.pause`, `self.resume`).
// A pause stops only new builds and clears when Theseus restarts; a halt (the kill switch, on the Models and system tab)
// stops every self step, and only the owner resumes from one, which asks first and says what runs again.
import { useState } from 'react'
import { PauseCircle, ThumbsDown, ThumbsUp } from 'lucide-react'
import type { Health, InitiativeWeekResult } from '@protocol'
import { Empty, Panel, Pill } from '@/components/ui'
import { pct } from '@/lib/format'
import { useRpc } from '@/lib/rpc'
import { ActButton, FIELD } from '../ActButton'
import { PAUSE_WORDS, RESUME_WORDS, UNPAUSE_WORDS } from './logic'

export function Initiative() {
  const { data } = useRpc<InitiativeWeekResult>('initiative.week', {}, 60_000)
  return (
    <Panel help="simple.config.initiative" title="Was what Theseus said first welcome?" icon={<ThumbsUp size={13} />} bodyClassName="flex flex-col gap-3 p-4">
      {!data ? <Empty>reading…</Empty> : (
        <p className="text-[13px] text-ink-dim">
          In the last seven days Theseus spoke first {data.spoke} {data.spoke === 1 ? 'time' : 'times'}: {data.up} welcomed, {data.down} not{data.up_share !== undefined && data.up_share !== null ? ` (${pct(data.up_share)} welcomed)` : ''}.
          It kept {data.kept} of {data.kept + data.unkept} promises to follow up{data.kept_share !== undefined && data.kept_share !== null ? ` (${pct(data.kept_share)})` : ''}. It is set to speak first at the “{data.level}” level.
        </p>
      )}
      <div className="flex flex-wrap items-center gap-2">
        <ActButton verb="initiative_up" params={{ rating: 'up' }} label={<><ThumbsUp size={13} />The latest was welcome</>} effect="Mark the latest message Theseus sent first as welcome. Theseus counts it when it decides how much to speak first." tone="plain" words={() => 'noted: welcome'} />
        <ActButton verb="initiative_down" params={{ rating: 'down' }} label={<><ThumbsDown size={13} />The latest was not</>} effect="Mark the latest message Theseus sent first as not welcome. Theseus counts it when it decides how much to speak first." tone="plain" words={() => 'noted: not welcome'} />
      </div>
    </Panel>
  )
}

export function SelfPause({ h }: { h?: Health }) {
  const s = h?.self_improve
  const [why, setWhy] = useState('')
  return (
    <Panel help="simple.config.selfpause" title="Pause or resume its self-improvement" icon={<PauseCircle size={13} />} bodyClassName="flex flex-col gap-3 p-4">
      <p className="flex flex-wrap items-center gap-2 text-[13px] text-ink-dim">
        {!s ? 'reading…' : s.halted ? <><Pill tone="fault">halted</Pill>{s.why ? `: ${s.why}` : ''}{s.by ? ` (by ${s.by})` : ''} Every self step is stopped until you resume it.</> : <><Pill tone="ok">{s.mode}</Pill>The self-improvement loop is not halted.</>}
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <input value={why} onChange={(e) => setWhy(e.target.value)} placeholder="why, in a few words (optional)" aria-label="why pause" className={`${FIELD} w-[260px]`} />
        <ActButton verb="self_pause" params={{ clear: false, ...(why.trim() ? { why: why.trim() } : {}) }} label="Pause new builds" effect={PAUSE_WORDS} tone="plain" onDone={() => setWhy('')} words={(r) => r.words ?? (r.paused ? 'paused' : 'not paused')} />
        <ActButton verb="self_unpause" params={{ clear: true }} label="Let builds start again" effect={UNPAUSE_WORDS} tone="plain" words={(r) => (r.changed ? 'builds may start again' : 'nothing was paused')} />
        {s?.halted && <ActButton verb="self_resume" params={{}} label="Resume after the halt" effect={RESUME_WORDS} tone="fault" submit="Yes, resume it" words={(r) => (r.changed ? 'resumed' : 'it was not halted')} />}
      </div>
    </Panel>
  )
}
