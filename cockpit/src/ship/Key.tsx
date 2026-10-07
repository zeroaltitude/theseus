// The key (theseus-hnof): a legend that teaches itself. Each line draws the shape as the chart draws it, says what it is
// in plain words and how the chart draws it, and how many the chart holds now. Rest on a line and every one of them
// lights on the chart while the rest dims; click to keep it lit; press ? for the tour. Folds to its title.
import { useMemo, useState } from 'react'
import { HelpCircle } from 'lucide-react'
import type { ShipModel } from './model'
import { keyLines, lights, type Glyph, type KeyGroup, type KeyLine } from './keyset'
import { SHAPES } from './words'
import './ship.css'

const GROUPS: KeyGroup[] = ['The fleet', 'A ship', 'Its oars', 'Its state']

export function KeyGlyph({ g }: { g: Glyph }) {
  const brass = '#d6a548'
  const hull = (stroke: string, glow = false) => (
    <path d="M2 9 Q4 5.6 9 5.6 L14.5 5.8 Q17.2 6.6 18 9 Q17.2 11.4 14.5 12.2 L9 12.4 Q4 12.4 2 9 Z" fill="rgb(239 227 200 / 0.14)" stroke={stroke} strokeWidth="1.3"
      style={glow ? { filter: `drop-shadow(0 0 2px ${stroke})` } : undefined} />
  )
  const oar = (blade: string, cross = false, open = false) => (
    <g>
      <line x1="3" y1="4" x2="13" y2="13" stroke="#cda463" strokeWidth="1.2" />
      <ellipse cx="14.4" cy="14.3" rx="3.4" ry="1.9" transform="rotate(42 14.4 14.3)" fill={open ? 'none' : blade} stroke={blade} strokeWidth="1" />
      {cross && <path d="M13 13 L15.8 15.6 M15.8 13 L13 15.6" stroke="#fff3f0" strokeWidth="1" />}
      <circle cx="3" cy="4" r="1.6" fill="#22d3ee" />
    </g>
  )
  return (
    <svg width="20" height="18" viewBox="0 0 20 18" aria-hidden className="shrink-0">
      {g === 'harbour' && <circle cx="10" cy="9" r="7" fill="none" stroke={brass} strokeWidth="1.1" strokeDasharray="3 1.4" />}
      {g === 'ship' && hull('#b08d57')}
      {g === 'boat' && <g><line x1="1" y1="9" x2="7" y2="9" stroke="#22d3ee" strokeWidth="0.9" strokeDasharray="1.5 1" /><path d="M7 9 Q8.5 6.6 12 6.6 L15.5 7 Q17.6 7.8 18 9 Q17.6 10.2 15.5 11 L12 11.4 Q8.5 11.4 7 9 Z" fill="rgb(239 227 200 / 0.14)" stroke="#b08d57" strokeWidth="1.1" /></g>}
      {g === 'bench' && <g>{hull('#b08d57')}<line x1="8" y1="5.8" x2="8" y2="12.2" stroke="#d6a548" strokeWidth="1" /><line x1="12" y1="5.8" x2="12" y2="12.2" stroke="#22d3ee" strokeWidth="1.2" /><line x1="15.4" y1="6.6" x2="15.4" y2="11.4" stroke="#d6a548" strokeWidth="1" /></g>}
      {g === 'message' && <circle cx="10" cy="9" r="3.4" fill="#efe3c8" style={{ filter: 'drop-shadow(0 0 3px #efe3c8)' }} />}
      {g === 'model' && <circle cx="10" cy="9" r="3.4" fill="#a78bfa" style={{ filter: 'drop-shadow(0 0 3px #a78bfa)' }} />}
      {g === 'oar' && oar('#34d399')}
      {g === 'oar-failed' && oar('#fb7185', true)}
      {g === 'oar-waiting' && oar('#fbbf24')}
      {g === 'gear' && <g><line x1="3" y1="4" x2="11.5" y2="11.6" stroke="#cda463" strokeWidth="1.2" /><circle cx="14" cy="13.6" r="3.6" fill="none" stroke="#fcd34d" strokeWidth="1.6" strokeDasharray="1.6 1.1" /><circle cx="14" cy="13.6" r="1.2" fill="#fcd34d" /></g>}
      {g === 'shield' && <g><polygon points="10,2.5 15.6,5.7 15.6,12.3 10,15.5 4.4,12.3 4.4,5.7" fill="rgb(94 234 212 / 0.1)" stroke="#5eead4" strokeWidth="1.2" /><circle cx="10" cy="9" r="2" fill="#22d3ee" /></g>}
      {g === 'web' && oar('#f472b6')}
      {g === 'sail' && <g>{hull('#22d3ee', true)}<path d="M9.6 1.4 L9.6 8 M6.4 2.2 L12.8 2.2 L12.2 6.6 L7 6.6 Z" fill="rgb(239 227 200 / 0.2)" stroke="#22d3ee" strokeWidth="0.9" /></g>}
      {g === 'lantern' && <g>{hull('#fbbf24', true)}<circle cx="3.2" cy="3.4" r="2.3" fill="#fbbf24" style={{ filter: 'drop-shadow(0 0 3px #fbbf24)' }} /></g>}
      {g === 'flare' && <g>{hull('#fb7185', true)}<circle cx="10" cy="2.6" r="2.2" fill="#fb7185" style={{ filter: 'drop-shadow(0 0 3px #fb7185)' }} /></g>}
      {g === 'anchor' && hull('#9a7745')}
      {g === 'planks' && <g>{hull('#b08d57')}<path d="M5 7.6 H15 M4 9 H16.6 M5 10.4 H15" stroke="#e3ad4f" strokeWidth="0.9" strokeDasharray="3 0.8" /></g>}
      {g === 'sea' && <g fill="none" stroke="#22d3ee" strokeWidth="1.1" strokeLinecap="round"><path d="M1 6 Q4 3.6 7 6 T13 6 T19 6" strokeOpacity="0.55" /><path d="M1 10.5 Q4 8.1 7 10.5 T13 10.5 T19 10.5" /><path d="M1 15 Q4 12.6 7 15 T13 15 T19 15" strokeOpacity="0.55" /></g>}
      {g === 'chain' && <g>{hull('#b08d57')}<path d="M3.4 8.2 Q9 5.2 16.8 8.2" fill="none" stroke="#ffa26a" strokeWidth="1.1" strokeDasharray="1.4 0.9" /></g>}
    </svg>
  )
}

export interface KeyProps {
  model: ShipModel | null
  /** The line pinned on (clicked), if any. */
  pinned: string | null
  /** A line rested on (null when the pointer leaves), and a line clicked. */
  onPreview: (k: KeyLine | null) => void
  onPin: (k: KeyLine | null) => void
  onTour: () => void
  /** The height the key may take (CSS pixels), when the selected vessel's card stands above it: its lines scroll
   *  rather than run under the card. */
  maxHeight?: number
  /** The sea's state in words ("dead calm", "a moderate swell"): the key's last line says it. */
  sea?: string
}

export function Key({ model, pinned, onPreview, onPin, onTour, maxHeight, sea }: KeyProps) {
  const [open, setOpen] = useState(() => {
    const kept = localStorage.getItem('cockpit.ship.legend')
    return kept ? kept === 'open' : window.innerWidth >= 1280 && window.innerHeight >= 860
  })
  const toggle = () => { localStorage.setItem('cockpit.ship.legend', open ? 'closed' : 'open'); setOpen(!open) }
  const lines = useMemo(() => (model ? keyLines(model) : []), [model])
  return (
    <div data-ship-ui className="ship-key-slot brass-card pointer-events-auto absolute bottom-3 left-3 flex w-[268px] flex-col !px-2.5 !py-2"
      style={maxHeight ? { maxHeight } : undefined} onMouseLeave={() => onPreview(null)}>
      <div className="flex items-center gap-2">
        <button onClick={toggle} className="ship-engraved flex flex-1 items-center justify-between text-[10px]" title="What every shape on the chart is">
          <span>The key</span><span className="text-ink-faint">{open ? '−' : '+'}</span>
        </button>
        <button onClick={onTour} title="The tour: what each shape is, on the chart (?)" className="text-ink-faint hover:text-neon"><HelpCircle size={14} /></button>
      </div>
      {open && (
        <div className="ship-key-body mt-1 min-h-0 overflow-y-auto">
          {GROUPS.map((g) => (
            <div key={g} className="ship-key-group">
              <div className="px-1 text-[9.5px] uppercase tracking-[0.14em] text-ink-faint">{g}</div>
              {lines.filter((k) => k.group === g).map((k) => {
                const can = lights(k) && k.count > 0
                return (
                  <button key={k.id} data-on={pinned === k.id ? '1' : ''} className="ship-key-row text-[11px] text-ink-dim disabled:opacity-60"
                    title={can ? 'Rest here to light them on the chart; click to keep them lit' : undefined}
                    onMouseEnter={() => onPreview(can ? k : null)} onFocus={() => onPreview(can ? k : null)} onBlur={() => onPreview(null)}
                    onClick={() => onPin(can && pinned !== k.id ? k : null)}>
                    <KeyGlyph g={k.glyph} />
                    <span className="min-w-0 truncate leading-tight"><b>{k.word}</b> <span className="text-[10px] text-ink-faint">· {k.sea}</span></span>
                    <small>{k.count}</small>
                  </button>
                )
              })}
            </div>
          ))}
          <div className="ship-key-group">
            <div className="px-1 text-[9.5px] uppercase tracking-[0.14em] text-ink-faint">The sea</div>
            <div className="ship-key-row text-[11px] text-ink-dim" title="The swell rises with tokens a minute and the turns running, and settles as they end; nothing moves when nothing happens">
              <KeyGlyph g="sea" />
              <span className="min-w-0 truncate leading-tight"><b>{SHAPES.sea.word}</b> <span className="text-[10px] text-ink-faint">· {SHAPES.sea.sea}</span></span>
              <small className="whitespace-nowrap">{sea ?? '—'}</small>
            </div>
          </div>
          <p className="mt-1 px-1 text-[10px] leading-snug text-ink-faint">Rest on a line to light it; click to keep it lit; ? the tour.</p>
        </div>
      )}
    </div>
  )
}
