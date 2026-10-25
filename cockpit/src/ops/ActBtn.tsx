// A button that acts on the Operations page: `Btn`'s look, off under the time machine with the banner's words.
import type { ReactNode } from 'react'
import { toneHex, type Tone } from '@/lib/taxonomy'
import { PAST_WORDS } from './act'
import type { ButtonAct, Verb } from './keys'
import { ShipsWheel } from '@/components/ShipsWheel'

/** A button that acts: `Btn`'s look, off (and saying why) when its verb is. */
export function ActBtn({ verb, on, busy, onClick, tone = 'live', title, children }: {
  verb: Verb | ButtonAct; on: (v: Verb | ButtonAct) => boolean; busy?: boolean; onClick: () => void; tone?: Tone; title?: string; children: ReactNode
}) {
  const off = !on(verb)
  return (
    <button onClick={onClick} disabled={busy || off} title={off ? PAST_WORDS : title} data-verb={verb}
      className="inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-[11.5px] font-medium transition-[filter] hover:brightness-125 focus-visible:outline focus-visible:outline-1 focus-visible:outline-gold disabled:cursor-not-allowed disabled:opacity-40"
      style={{ color: toneHex[tone], background: `${toneHex[tone]}14`, boxShadow: `inset 0 0 0 1px ${toneHex[tone]}40` }}>
      {busy && <ShipsWheel />}{children}
    </button>
  )
}
