// The depth gauge (theseus-hnof): where the camera is, in the four depths the chart reads at, and a stop to go to each.
//   FLEET: every harbour and ship, their states in words.
//   SHIP:  one session: its turns as benches, its oars, its tasks in tow.
//   TURN:  one turn: its message, its model calls, each tool call by name with its result.
//   CALL:  one call's data: the inspector, with its input, output and gate.
// Scrolling moves between them continuously; the gauge's needle follows.
import { useEffect } from 'react'
import { ChevronsUpDown, Maximize2, Minimize2, Minus } from 'lucide-react'
import { HelpButton } from '@/components/Help'
import { cn } from '@/lib/format'
import type { DepthSize } from '@/lib/prefs'
import { type Depth } from './words'

export type { Depth }

const STOPS: { d: Depth; word: string; title: string }[] = [
  { d: 'fleet', word: 'Fleet', title: 'Every place and session (Home)' },
  { d: 'ship', word: 'Ship', title: 'One session: its turns, its oars, its tasks' },
  { d: 'turn', word: 'Turn', title: 'One turn: its calls by name, and their results' },
  { d: 'call', word: 'Call', title: 'One call’s data: input, output, its gate' },
]

export function DepthGauge({ depth, onGo, size, onSize }: { depth: Depth; onGo: (d: Depth) => void; size: DepthSize; onSize: (s: DepthSize) => void }) {
  // Large is an overlay: Escape puts it back to small (caught before the Ship's own Escape, which would let go of the
  // selection too).
  useEffect(() => {
    if (size !== 'max') return
    const k = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.stopPropagation(); onSize('small') } }
    document.addEventListener('keydown', k, true)
    return () => document.removeEventListener('keydown', k, true)
  }, [size, onSize])
  const now = STOPS.find((s) => s.d === depth)
  if (size === 'min') {
    return (
      <button type="button" data-ship-ui className="ship-depth ship-depth-chip brass-button pointer-events-auto !px-2 !py-0.5 text-[10px]"
        onClick={() => onSize('small')} title="Show the depth gauge" aria-label={`Depth: ${now?.word}. Show the depth gauge`}>
        <ChevronsUpDown size={11} aria-hidden /> Depth · <b className="text-ink">{now?.word}</b>
      </button>
    )
  }
  const big = size === 'max'
  return (
    <div data-ship-ui role={big ? 'dialog' : 'group'} aria-label="Depth gauge" data-size={size}
      className={cn('ship-depth brass-card pointer-events-auto', big ? 'ship-depth-big !px-3 !py-2' : '!px-1.5 !py-1')} title="How deep you are: scroll, or pick a depth">
      <div className="mb-0.5 flex items-center gap-1 px-0.5">
        <span className="ship-engraved text-[8.5px]">Depth</span>
        <span className="ml-auto flex items-center gap-0.5">
          <HelpButton id="ship.depth" title="Depth" />
          <button type="button" className="ship-depth-ctl" onClick={() => onSize('min')} title="Minimize the depth gauge" aria-label="Minimize the depth gauge"><Minus size={10} aria-hidden /></button>
          {big
            ? <button type="button" className="ship-depth-ctl" onClick={() => onSize('small')} title="Back to the small gauge (Esc)" aria-label="Back to the small gauge"><Minimize2 size={10} aria-hidden /></button>
            : <button type="button" className="ship-depth-ctl" onClick={() => onSize('max')} title="Maximize: what each depth shows" aria-label="Maximize the depth gauge"><Maximize2 size={10} aria-hidden /></button>}
        </span>
      </div>
      {STOPS.map((s) => (
        <button key={s.d} className="ship-depth-stop w-full" data-on={depth === s.d ? '1' : ''} title={s.title} onClick={() => onGo(s.d)}>
          <i />
          <span>{s.word}</span>
          {big && <span className="ship-depth-what">{s.title}</span>}
        </button>
      ))}
    </div>
  )
}
