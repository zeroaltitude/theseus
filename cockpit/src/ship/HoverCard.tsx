// The Ship's hover cards (theseus-hnof): whatever the pointer rests on says what it is first, in plain words ("SESSION",
// "TURN 12 OF 17", "TOOL CALL"), then its real data, then where a click takes you. The sea's word for it rides along,
// faint, so the metaphor is learned while it is read. Every word is `words.ts`'s, as the key and the labels say it.
import { useLayoutEffect, useRef, useState } from 'react'
import type { Hit } from './engine'
import type { Bench, Light, ShipModel, Vessel } from './model'
import { authorWord, benchLine, benchState, count, LIGHT_NOUN, LIGHT_SEA, outcome, SHAPES, span, stateWord, type Tone, usdShort, vesselNoun, vesselSea } from './words'
import { placeCard } from './placement'
import { instrumentRects } from './instrumentRects'
import { ago, clock } from '@/lib/format'

const TONE: Record<Tone, string> = { live: 'text-live', wait: 'text-wait', fault: 'text-fault', ok: 'text-ok', idle: 'text-ink-dim' }

function Head({ noun, sea, right }: { noun: string; sea: string; right?: string }) {
  return (
    <div className="flex items-baseline gap-2">
      <span className="ship-engraved text-[10px]">{noun}</span>
      <span className="text-[10px] italic text-ink-faint">the {sea}</span>
      {right && <span className="num ml-auto shrink-0 text-[10.5px] text-ink-faint">{right}</span>}
    </div>
  )
}

function VesselBody({ v, model }: { v: Vessel; model: ShipModel }) {
  const st = stateWord(v)
  const tasks = model.vessels.filter((x) => x.parentId === v.id)
  const parent = v.parentId ? model.vessels[model.byId.get(v.parentId) ?? -1] : undefined
  const calls = v.benches.reduce((a, b) => a + model.benches[b].calls, 0)
  const failed = v.benches.reduce((a, b) => a + model.benches[b].failed, 0)
  return (
    <>
      <Head noun={vesselNoun(v)} sea={vesselSea(v)} right={v.kind === 'task' ? v.taskShort : undefined} />
      <div className="mt-0.5 font-display text-[13.5px] font-semibold leading-snug text-ivory">{v.title}</div>
      <div className={`num text-[11.5px] ${TONE[st.tone]}`}>{st.word} <span className="text-ink-faint">· {st.sea}</span></div>
      <div className="num mt-1 text-[11px] text-ink-dim">
        {count(v.turns, 'turn')} · {count(calls, 'tool call')}{failed ? `, ${failed} failed` : ''} · {usdShort(v.cost)}{v.limit ? ` of ${usdShort(v.limit)}` : ''}
      </div>
      <div className="num text-[11px] text-ink-faint">
        {v.place && !v.parentId ? `from ${v.label || 'the CLI'} · ` : ''}last active {ago(v.lastActive)}
      </div>
      {parent && <div className="num text-[11px] text-ink-dim">started by the session “{parent.title.slice(0, 40)}”</div>}
      {!!tasks.length && <div className="num text-[11px] text-ink-dim">towing {count(tasks.length, 'task')}</div>}
      {v.hold && <div className="num text-[11px] text-wait">read text from the web: {v.hold.tool} {v.hold.query ?? v.hold.url}</div>}
      <div className="mt-1 text-[10.5px] text-ink-faint">click to bring it alongside · double-click to fly in</div>
    </>
  )
}

function BenchBody({ b, model }: { b: Bench; model: ShipModel }) {
  const v = model.vessels[b.vessel]
  const total = v.benches.length
  const state = benchState(b)
  return (
    <>
      <Head noun={`${SHAPES.turn.noun} ${b.n} of ${total}`} sea="bench" right={Number.isFinite(b.at) ? clock(b.at) : undefined} />
      {b.preview && <div className="mt-0.5 line-clamp-2 text-[12px] leading-snug text-ink">“{b.preview}”</div>}
      <div className="num mt-0.5 text-[10.5px] text-ink-faint">asked by {authorWord(b.author)} in “{v.title.slice(0, 36)}”</div>
      <div className={`num mt-1 text-[11.5px] ${TONE[state.tone]}`}>{state.word}{b.end > b.at ? ` · took ${span(b.end - b.at)}` : ''}</div>
      <div className="num text-[11px] text-ink-dim">{benchLine(b)}{b.model ? ` · ${b.model.replace(/^claude-/, '')}` : ''}</div>
      <div className="mt-1 text-[10.5px] text-ink-faint">click to bring this turn close and onto the card; the card opens it in the session</div>
    </>
  )
}

function LightBody({ l, model }: { l: Light; model: ShipModel }) {
  const v = model.vessels[l.vessel]
  const b = l.bench >= 0 ? model.benches[l.bench] : undefined
  // An oar is one thing to the reader: the call and its result, together.
  const call = l.kind === 'result' ? model.lights.find((c) => c.kind === 'call' && c.toolUseId && c.toolUseId === l.toolUseId) ?? l : l
  const result = call.kind === 'call' ? model.lights.find((r) => r.kind === 'result' && r.toolUseId && r.toolUseId === call.toolUseId) : undefined
  if (call.kind === 'call') {
    const o = outcome({ ...call, failed: result?.failed ?? call.failed, external: result?.external, collapsedAt: result?.collapsedAt ?? call.collapsedAt }, !!result)
    return (
      <>
        <Head noun={LIGHT_NOUN.call} sea={LIGHT_SEA.call} right={clock(call.at)} />
        <div className="num mt-0.5 text-[13px] text-ivory">{call.tool ?? 'tool'} <span className={TONE[o.tone]}>· {o.word}</span></div>
        {call.preview && <div className="mt-0.5 line-clamp-2 text-[11.5px] leading-snug text-ink-dim">{call.preview}</div>}
        {result?.preview && <div className="num mt-1 line-clamp-2 text-[11px] leading-snug text-ink-faint">→ {result.preview}</div>}
        <div className="num mt-1 flex flex-wrap gap-x-2 text-[10.5px]">
          {result && <span className="text-ink-dim">took {span(result.at - call.at)}</span>}
          {call.running && <span className="text-money">⚙ a job, running since {clock(call.at)}</span>}
          {call.l1 && <span className="text-[#5eead4]">⬡ sandboxed (L1)</span>}
          {result?.external && <span className="text-think">◎ text from the web</span>}
          {b && <span className="text-ink-faint">turn {b.n} of “{v.title.slice(0, 28)}”</span>}
        </div>
        <div className="mt-1 text-[10.5px] text-ink-faint">click for the call inspector: its input, its output, its gate</div>
      </>
    )
  }
  const noun = LIGHT_NOUN[l.kind]
  const head = l.kind === 'model' ? (l.model ?? 'model').replace(/^claude-/, '') : `from ${authorWord(l.author)}`
  return (
    <>
      <Head noun={noun} sea={LIGHT_SEA[l.kind]} right={clock(l.at)} />
      <div className="num mt-0.5 text-[12.5px] text-ivory">{head}{l.cost !== undefined ? <span className="text-money"> · {usdShort(l.cost)}</span> : null}</div>
      {l.preview && <div className="mt-0.5 line-clamp-3 text-[11.5px] leading-snug text-ink-dim">{l.preview}</div>}
      {b && <div className="num mt-1 text-[10.5px] text-ink-faint">turn {b.n} of “{v.title.slice(0, 32)}”</div>}
      <div className="mt-1 text-[10.5px] text-ink-faint">{l.kind === 'model' ? 'click for the model-call inspector' : 'click to put its turn on the card'}</div>
    </>
  )
}

export function HoverCard({ hover, model }: { hover: { hit: Hit; x: number; y: number }; model: ShipModel }) {
  // Beside the pointer, inside the Ship, and off its instruments (it once slid under the console, theseus-hnof.2):
  // measured before it is painted, then placed.
  const ref = useRef<HTMLDivElement>(null)
  const [at, setAt] = useState<{ x: number; y: number } | null>(null)
  useLayoutEffect(() => {
    const el = ref.current
    const box = el?.parentElement
    if (!el || !box) return
    const p = placeCard({ x: hover.x, y: hover.y }, { w: el.offsetWidth, h: el.offsetHeight }, { w: box.clientWidth, h: box.clientHeight }, instrumentRects(box))
    setAt((q) => (q && q.x === p.x && q.y === p.y ? q : { x: p.x, y: p.y }))
  }, [hover.x, hover.y, hover.hit])
  const h = hover.hit
  return (
    <div ref={ref} className="brass-tip pointer-events-none absolute left-0 top-0 z-[34] w-max max-w-[410px]"
      style={{ transform: at ? `translate(${at.x}px, ${at.y}px)` : undefined, visibility: at ? 'visible' : 'hidden' }}>
      {h.kind === 'vessel' && <VesselBody v={model.vessels[h.vessel]} model={model} />}
      {h.kind === 'bench' && <BenchBody b={model.benches[h.bench]} model={model} />}
      {h.kind === 'light' && <LightBody l={model.lights[h.light]} model={model} />}
    </div>
  )
}
