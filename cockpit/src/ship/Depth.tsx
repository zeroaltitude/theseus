// The depth gauge (theseus-hnof): where the camera is, in the four depths the chart reads at, and a stop to go to each.
//   FLEET: every harbour and ship, their states in words.
//   SHIP:  one session: its turns as benches, its oars, its tasks in tow.
//   TURN:  one turn: its message, its model calls, each tool call by name with its result.
//   CALL:  one call's data: the inspector, with its input, output and gate.
// Scrolling moves between them continuously; the gauge's needle follows.
import { type Depth } from './words'

export type { Depth }

const STOPS: { d: Depth; word: string; title: string }[] = [
  { d: 'fleet', word: 'Fleet', title: 'Every place and session (Home)' },
  { d: 'ship', word: 'Ship', title: 'One session: its turns, its oars, its tasks' },
  { d: 'turn', word: 'Turn', title: 'One turn: its calls by name, and their results' },
  { d: 'call', word: 'Call', title: 'One call’s data: input, output, its gate' },
]

export function DepthGauge({ depth, onGo }: { depth: Depth; onGo: (d: Depth) => void }) {
  return (
    <div data-ship-ui className="ship-depth brass-card pointer-events-auto !px-2 !py-1.5" title="How deep you are: scroll, or pick a depth">
      <div className="ship-engraved mb-0.5 px-1 text-[9px]">Depth</div>
      {STOPS.map((s) => (
        <button key={s.d} className="ship-depth-stop w-full" data-on={depth === s.d ? '1' : ''} title={s.title} onClick={() => onGo(s.d)}>
          <i />{s.word}
        </button>
      ))}
    </div>
  )
}
