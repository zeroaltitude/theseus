// The Ship (theseus-logs): the cockpit's hero view. One agent over one graph, on screen: every session a vessel,
// every task a boat in tow, every message and tool call a light along a keel, the reach between them as currents,
// and the bridge's brass instruments around the edge. Everything shown is the daemon's own data.
import { useEffect, useMemo, useRef, useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { Anchor, Crosshair, ExternalLink, Maximize, Search, Waves } from 'lucide-react'
import { ShipEngine, type Hit } from '@/ship/engine'
import { LabelLayer, rigWords, usdShort } from '@/ship/labels'
import { Minimap, type MinimapHandle } from '@/ship/Minimap'
import { Compass, EngineTelegraph, FuelGauge, Nixie, PressureGauge, ShipsClock } from '@/ship/instruments'
import { useShipLive, useShipSynthetic, type ShipData } from '@/ship/useShipData'
import { placeOf, type Light, type ShipModel, type Vessel } from '@/ship/model'
import { CallInspector } from '@/components/CallInspector'
import { ModelInspector } from '@/components/ModelInspector'
import { PlankStrip } from '@/components/brass'
import { useCalm } from '@/lib/calm'
import { useConn } from '@/lib/rpc'
import { ago, clock, cn, short } from '@/lib/format'

// The synthetic fleet is for measuring, in dev and bench builds only (never in a production build).
const SYNTH = (import.meta.env.DEV || import.meta.env.MODE === 'bench') && new URLSearchParams(window.location.search).has('synthetic')

export default function ShipRoute() {
  return SYNTH ? <ShipSynthetic /> : <ShipLive />
}

function ShipLive() {
  const [params] = useSearchParams()
  return <ShipView data={useShipLive(params.get('s') ?? undefined)} />
}

function ShipSynthetic() {
  return <ShipView data={useShipSynthetic()} />
}

declare global {
  interface Window { __ship?: { stats: ShipEngine['stats']; lights: number; vessels: number; mountedAt: number } }
}

const KIND_WORD: Record<Light['kind'], string> = { user: 'message', model: 'model call', call: 'tool call', result: 'result' }

function ShipView({ data }: { data: ShipData }) {
  const [params, setParams] = useSearchParams()
  const calm = useCalm((s) => s.calm)
  const setCalm = useCalm((s) => s.setCalm)
  const host = useRef<HTMLDivElement>(null)
  const labelsRoot = useRef<HTMLDivElement>(null)
  const minimap = useRef<MinimapHandle>(null)
  const [engine, setEngine] = useState<ShipEngine | null>(null)
  const labels = useRef<LabelLayer | null>(null)
  const [hover, setHover] = useState<{ hit: Hit; x: number; y: number } | null>(null)
  const fitted = useRef(false)
  const model = data.model
  const selId = params.get('s') ?? undefined
  const sel = model && selId ? model.byId.get(selId) ?? -1 : -1
  const hlId = params.get('n') ?? undefined
  const bench = params.has('bench')
  const pin = Number(params.get('scale')) || undefined

  // The engine lives as long as the view.
  useEffect(() => {
    const mountedAt = performance.now()
    const e = new ShipEngine(host.current!, { calm: useCalm.getState().calm, bench, scale: pin })
    // The title above and the instruments below cover the canvas's edges: a fit keeps the fleet between them.
    e.insets = { top: 112, right: 28, bottom: 196, left: 28 }
    labels.current = new LabelLayer(labelsRoot.current!)
    setEngine(e)
    window.__ship = { stats: e.stats, lights: 0, vessels: 0, mountedAt }
    if (import.meta.env.DEV || import.meta.env.MODE === 'bench') (window as unknown as { __shipEngine: ShipEngine }).__shipEngine = e
    return () => {
      labels.current?.dispose()
      labels.current = null
      e.dispose()
      setEngine(null)
    }
  }, [bench, pin])

  useEffect(() => { engine?.setCalm(calm) }, [engine, calm])

  // The model into the engine and the labels. The first one fits the fleet, or lands on the vessel (and the light)
  // the address names (a deep link).
  useEffect(() => {
    if (!engine || !model) return
    const first = !fitted.current
    const at = first && selId ? model.byId.get(selId) : undefined
    engine.setModel(model, { fit: first && at === undefined })
    if (at !== undefined) engine.flyToVessel(at, false)
    fitted.current = true
    labels.current?.setModel(model)
    if (window.__ship) { window.__ship.lights = model.lights.length; window.__ship.vessels = model.vessels.length }
  }, [engine, model]) // eslint-disable-line react-hooks/exhaustive-deps -- selId matters only for the first model
  // A deep link to a light lands on it once the graph is read (the first model may not have the nodes yet).
  const landed = useRef(false)
  useEffect(() => {
    if (landed.current || !engine || !model || data.progress < 1 || !hlId) return
    const li = model.lightById.get(hlId)
    if (li === undefined) return
    landed.current = true
    engine.flyToLight(li)
  }, [engine, model, data.progress, hlId])

  useEffect(() => { engine?.setSelected(sel) }, [engine, sel, model])
  useEffect(() => {
    if (!engine || !model) return
    engine.setHighlight(hlId ? model.lightById.get(hlId) ?? -1 : -1)
  }, [engine, model, hlId])

  // Pointer and frame hooks.
  const selRef = useRef(sel)
  const hoverIdx = hover?.hit.kind === 'vessel' ? hover.hit.vessel : -1
  const hoverRef = useRef(hoverIdx)
  useEffect(() => {
    selRef.current = sel
    hoverRef.current = hoverIdx
    engine?.requestRender()
  }, [sel, hoverIdx, engine])
  useEffect(() => {
    if (!engine) return
    engine.setHooks({
      onHover: (hit, x, y) => {
        setHover(hit ? { hit, x, y } : null)
        engine.setHovered(hit?.kind === 'vessel' ? hit.vessel : -1)
      },
      onClick: (hit) => {
        const m = engine.model
        if (!m) return
        if (!hit) return
        if (hit.kind === 'vessel') {
          const v = m.vessels[hit.vessel]
          setParams((p) => { p.set('s', v.id); p.delete('n'); p.delete('call'); p.delete('msg'); return p }, { replace: true })
          engine.flyToVessel(hit.vessel)
        } else {
          const l = m.lights[hit.light]
          setParams((p) => {
            p.set('s', l.sessionId)
            p.set('n', l.id)
            p.delete('call'); p.delete('msg')
            if ((l.kind === 'call' || l.kind === 'result') && (l.toolUseId || l.correlationId)) p.set('call', (l.toolUseId ?? l.correlationId)!)
            if (l.kind === 'model') p.set('msg', l.id)
            return p
          }, { replace: true })
        }
      },
      onDouble: (hit) => {
        if (hit?.kind === 'vessel') engine.flyToVessel(hit.vessel)
        else if (hit?.kind === 'light') engine.flyToLight(hit.light)
      },
      onFrame: (moved) => {
        labels.current?.update(engine, selRef.current, hoverRef.current)
        if (moved) minimap.current?.draw()
      },
    })
  }, [engine, setParams])

  // ?fly= (from the palette, ⌘K): a session or a node, once the graph is read (vessels grow as their nodes arrive).
  const fly = params.get('fly')
  const read = data.progress >= 1
  useEffect(() => {
    if (!fly || !engine || !model || !read) return
    const vi = model.byId.get(fly)
    const li = model.lightById.get(fly)
    if (vi === undefined && li === undefined) return
    setParams((p) => {
      p.delete('fly')
      if (vi !== undefined) { p.set('s', fly); p.delete('n') } else { p.set('s', model.lights[li!].sessionId); p.set('n', fly) }
      return p
    }, { replace: true })
    if (vi !== undefined) engine.flyToVessel(vi)
    else engine.flyToLight(li!)
  }, [fly, engine, model, read, setParams])

  // Keys: Home or 0 fits the fleet; Esc lets the selection go (when no inspector is open).
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null
      if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return
      if (e.metaKey || e.ctrlKey || e.altKey) return
      if (e.key === 'Home' || e.key === '0') { e.preventDefault(); engine?.fit() }
      if (e.key === 'Escape' && !params.get('call') && !params.get('msg')) setParams((p) => { p.delete('s'); p.delete('n'); return p }, { replace: true })
    }
    window.addEventListener('keydown', k)
    return () => window.removeEventListener('keydown', k)
  }, [engine, params, setParams])

  const vessel = model && sel >= 0 ? model.vessels[sel] : undefined
  const focus = vessel ?? (model ? [...model.vessels].sort((a, b) => b.lastActive - a.lastActive)[0] : undefined)
  const h = data.health
  const approvals = model?.vessels.reduce((a, v) => a + v.pendingConfirms, 0) ?? 0
  const status = useConn((s) => s.status)

  return (
    <div className="ship-root relative h-full w-full overflow-hidden" data-calm={calm ? '1' : ''}>
      <div ref={host} className="absolute inset-0" />
      <div ref={labelsRoot} className="ship-labels pointer-events-none absolute inset-0 overflow-hidden" />
      {data.progress < 1 && <div className="pointer-events-none absolute inset-x-0 top-0" title="Reading the graph"><PlankStrip progress={data.progress} height={6} /></div>}

      <Cartouche model={model} synthetic={data.synthetic} live={status === 'open'} error={data.error} />

      <div data-ship-ui className="absolute right-3 top-4 flex items-center gap-1.5">
        <BrassButton title="Fly to a session or a call (Ctrl+K)" onClick={() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true }))}>
          <Search size={13} /> Fly to <span className="kbd ml-1">⌘K</span>
        </BrassButton>
        <BrassButton title="See the whole fleet (Home)" onClick={() => engine?.fit()}><Maximize size={13} /> Fleet</BrassButton>
        {vessel && <BrassButton title="Back to the selected vessel" onClick={() => engine?.flyToVessel(sel)}><Crosshair size={13} /> Vessel</BrassButton>}
        <BrassButton title={calm ? 'Calm: no motion or glow. Click for the full hologram.' : 'Calm mode drops the motion, the glow, and the particles'} on={calm} onClick={() => setCalm(!calm)}>
          {calm ? <Anchor size={13} /> : <Waves size={13} />} {calm ? 'Calm' : 'Live'}
        </BrassButton>
      </div>

      {vessel && model && <VesselCard v={vessel} model={model} onClose={() => setParams((p) => { p.delete('s'); p.delete('n'); return p }, { replace: true })} />}
      {hover && model && <HoverCard hover={hover} model={model} />}
      <Legend />

      <div data-ship-ui className="ship-console pointer-events-auto absolute bottom-3 left-1/2 flex -translate-x-1/2 items-end gap-2.5 px-4 pb-2 pt-2.5">
        <Compass live={data.profiles?.live ?? h?.profile} profiles={data.profiles?.profiles.map((p) => p.name) ?? []} model={h?.model} />
        <PressureGauge approvals={approvals} holds={h?.external_text?.length ?? model?.stats.held ?? 0} held={h?.kernel.turns_held ?? 0} />
        <FuelGauge spent={focus?.spent ?? focus?.cost ?? 0} reserved={focus?.reserved ?? 0} limit={focus?.limit ?? h?.kernel.spend_limit_usd}
          of={focus ? focus.title.slice(0, 22) : '—'} total={h?.cost_usd_total ?? 0} />
        <EngineTelegraph accepting={h?.kernel.accepting} running={h?.kernel.executions_by_state.running ?? model?.stats.running ?? 0} ceiling={h?.kernel.admission_ceiling ?? 8} held={h?.kernel.turns_held ?? 0} />
        <ShipsClock uptime={h?.uptime_secs} version={h?.version} />
        <Nixie value={data.tpm} label="Tokens / min" title="Tokens a minute: input, cache, and output of every model call in the last sixty seconds (provider.call rows)." />
      </div>

      <div data-ship-ui className="absolute bottom-3 right-3"><Minimap ref={minimap} engine={engine} model={model} selected={sel} /></div>

      {vessel && <CallInspector sessionId={vessel.id} />}
      {vessel && <ModelInspector sessionId={vessel.id} />}
    </div>
  )
}

function BrassButton({ children, onClick, title, on }: { children: React.ReactNode; onClick: () => void; title: string; on?: boolean }) {
  return (
    <button type="button" onClick={onClick} title={title} className={cn('brass-button pointer-events-auto', on && 'brass-button-on')}>
      {children}
    </button>
  )
}

function Cartouche({ model, synthetic, live, error }: { model: ShipModel | null; synthetic: boolean; live: boolean; error?: string }) {
  const s = model?.stats
  return (
    <div data-ship-ui className="ship-cartouche pointer-events-none absolute left-4 top-4">
      <div className="flex items-baseline gap-3">
        <h1 className="ship-title">The Ship</h1>
        {synthetic && <span className="ship-synthetic">synthetic fleet · dev only</span>}
      </div>
      <div className="num mt-0.5 flex flex-wrap gap-x-3 text-[11.5px] text-ink-dim">
        <span><b className="text-ink">{s?.sessions ?? '…'}</b> vessels</span>
        <span><b className="text-ink">{s?.nodes ?? '…'}</b> lights</span>
        {!!s?.tasks && <span><b className="text-ink">{s.tasks}</b> in tow</span>}
        <span className={s?.running ? 'text-live' : ''}><b>{s?.running ?? 0}</b> under sail</span>
        {!!s?.waiting && <span className="text-wait"><b>{s.waiting}</b> lanterns</span>}
        {!!s?.held && <span className="text-think"><b>{s.held}</b> chained</span>}
        {!!s?.l1 && <span className="text-[#5eead4]"><b>{s.l1}</b> shielded</span>}
      </div>
      {!synthetic && !live && <div className="mt-1 text-[11.5px] text-wait">the link to the daemon is down: reconnecting…</div>}
      {error && <div className="mt-1 text-[11.5px] text-fault">{error}</div>}
    </div>
  )
}

function VesselCard({ v, model, onClose }: { v: Vessel; model: ShipModel; onClose: () => void }) {
  const lights = useMemo(() => model.lights.filter((l) => l.sessionId === v.id), [model, v.id])
  const kinds = { user: 0, model: 0, call: 0, result: 0 } as Record<Light['kind'], number>
  for (const l of lights) kinds[l.kind]++
  const tasks = model.vessels.filter((x) => x.parentId === v.id)
  const parent = v.parentId ? model.vessels[model.byId.get(v.parentId) ?? -1] : undefined
  const l1 = lights.filter((l) => l.l1).length
  const ext = lights.filter((l) => l.external).length
  return (
    <aside data-ship-ui className="brass-card pointer-events-auto absolute left-4 top-[92px] w-[300px]">
      <header className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <div className="ship-engraved text-[10px]">{v.kind === 'task' ? `task ${v.taskShort ?? ''}` : placeOf({ label: v.label, kind: v.kind }).label}</div>
          <h2 className="truncate font-display text-[16px] font-semibold text-ivory" title={v.title}>{v.title}</h2>
          <div className={cn('num text-[11.5px]', v.rig === 'sail' ? 'text-live' : v.rig === 'lantern' ? 'text-wait' : v.rig === 'flare' ? 'text-fault' : 'text-ink-dim')}>{rigWords(v)}</div>
        </div>
        <button onClick={onClose} title="Let the selection go (Esc)" className="rounded px-1 text-ink-faint hover:text-ink">×</button>
      </header>
      <dl className="num mt-2 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-[11.5px]">
        <dt className="text-ink-faint">lights</dt><dd className="text-ink">{v.nodes} · {kinds.user} msg · {kinds.model} model · {kinds.call} calls</dd>
        <dt className="text-ink-faint">turns</dt><dd className="text-ink">{v.turns} · last {ago(v.lastActive)}</dd>
        <dt className="text-ink-faint">money</dt><dd className="text-ink">{usdShort(v.cost)}{v.limit ? ` of ${usdShort(v.limit)}` : ''}{v.reserved ? ` · ${usdShort(v.reserved)} reserved` : ''}</dd>
        {(v.profile || v.model) && <><dt className="text-ink-faint">profile</dt><dd className="truncate text-ink">{v.profile ?? '—'} · {v.model ?? '—'}</dd></>}
        {!!l1 && <><dt className="text-ink-faint">sandbox</dt><dd className="text-[#5eead4]">{l1} lights shielded (L1)</dd></>}
        {!!ext && <><dt className="text-ink-faint">external</dt><dd className="text-think">{ext} {ext === 1 ? 'result' : 'results'} from the web</dd></>}
        {v.hold && <><dt className="text-ink-faint">chained</dt><dd className="text-wait" title={v.hold.url}>read {v.hold.tool} {v.hold.query ? `"${v.hold.query}"` : v.hold.url} · {ago(v.hold.since_ms)}</dd></>}
        {!!v.pendingConfirms && <><dt className="text-ink-faint">waiting</dt><dd className="text-wait">{v.pendingConfirms} approval{v.pendingConfirms === 1 ? '' : 's'} for you</dd></>}
        {parent && <><dt className="text-ink-faint">in tow of</dt><dd className="truncate text-ink">{parent.title}</dd></>}
        {!!tasks.length && <><dt className="text-ink-faint">towing</dt><dd className="text-ink">{tasks.length} task{tasks.length === 1 ? '' : 's'}</dd></>}
        <dt className="text-ink-faint">id</dt><dd className="text-ink-dim">{short(v.id)}</dd>
      </dl>
      <div className="mt-2.5 flex flex-wrap gap-1.5">
        <Link to={`/session/${v.id}`} className="brass-button"><ExternalLink size={12} /> Session deck</Link>
        <Link to={`/ledger?q=${v.id}`} className="brass-button">Ledger rows</Link>
      </div>
      <p className="mt-2 text-[10.5px] leading-snug text-ink-faint">Click a light for its inspector. Double-click to fly to it.</p>
    </aside>
  )
}

function HoverCard({ hover, model }: { hover: { hit: Hit; x: number; y: number }; model: ShipModel }) {
  const style = { left: Math.min(hover.x + 16, window.innerWidth - 420), top: hover.y + 14 }
  if (hover.hit.kind === 'vessel') {
    const v = model.vessels[hover.hit.vessel]
    return (
      <div className="brass-tip pointer-events-none absolute" style={style}>
        <div className="font-display text-[13px] font-semibold text-ivory">{v.title}</div>
        <div className="num text-[11px] text-ink-dim">{rigWords(v)} · {v.nodes} lights · {v.turns} turns · {usdShort(v.cost)}</div>
        {v.hold && <div className="num text-[11px] text-wait">chained: read {v.hold.tool} {v.hold.query ?? v.hold.url}</div>}
        <div className="mt-0.5 text-[10.5px] text-ink-faint">click to bring it alongside</div>
      </div>
    )
  }
  const l = model.lights[hover.hit.light]
  const v = model.vessels[l.vessel]
  const head = l.kind === 'call' ? `${l.tool ?? 'tool'}` : l.kind === 'result' ? `${l.tool ?? 'tool'} → ${l.failed ? 'failed' : 'ok'}` : l.kind === 'model' ? (l.model ?? 'model') : (l.author ?? 'the operator')
  return (
    <div className="brass-tip pointer-events-none absolute max-w-[400px]" style={style}>
      <div className="flex items-baseline gap-2">
        <span className="ship-engraved text-[9.5px]">{KIND_WORD[l.kind]}</span>
        <span className="num truncate text-[12.5px] text-ink">{head}</span>
        <span className="num ml-auto shrink-0 text-[10.5px] text-ink-faint">{clock(l.at)}</span>
      </div>
      {l.preview && <div className="mt-0.5 line-clamp-3 text-[11.5px] leading-snug text-ink-dim">{l.preview}</div>}
      <div className="num mt-1 flex flex-wrap gap-1.5 text-[10.5px]">
        {l.l1 && <span className="text-[#5eead4]">⬡ sandboxed (L1)</span>}
        {l.external && <span className="text-think">◎ external text</span>}
        {l.running && <span className="text-money">⚙ running</span>}
        {l.cost !== undefined && <span className="text-money">{usdShort(l.cost)}</span>}
        <span className="text-ink-faint">{v.title.slice(0, 40)}</span>
      </div>
      <div className="mt-0.5 text-[10.5px] text-ink-faint">{l.kind === 'model' ? 'click for the model-call inspector' : l.kind === 'user' ? 'a message' : 'click for the call inspector'}</div>
    </div>
  )
}

function Legend() {
  // Open by default on a wide screen; folded on a narrow one (it would crowd the instruments).
  const [open, setOpen] = useState(() => {
    const kept = localStorage.getItem('cockpit.ship.legend')
    return kept ? kept === 'open' : window.innerWidth >= 1500
  })
  const toggle = () => { localStorage.setItem('cockpit.ship.legend', open ? 'closed' : 'open'); setOpen(!open) }
  return (
    <div data-ship-ui className="brass-card pointer-events-auto absolute bottom-3 left-3 w-[210px] !p-2.5">
      <button onClick={toggle} className="ship-engraved flex w-full items-center justify-between text-[10px]" title="What the marks mean">
        <span>The key</span><span className="text-ink-faint">{open ? '−' : '+'}</span>
      </button>
      {open && (
        <ul className="mt-1.5 space-y-[3px] text-[11px] text-ink-dim">
          <Key c="#efe3c8">a message</Key>
          <Key c="#a78bfa">a model call</Key>
          <Key c="#22d3ee">a tool call (an oar)</Key>
          <Key c="#34d399">its result, ok</Key>
          <Key c="#fb7185">its result, failed</Key>
          <Key c="#f472b6" ring>external text (the web)</Key>
          <Key c="#5eead4" hex>sandboxed (L1)</Key>
          <Key c="#d6a548" gear>a job running</Key>
          <li className="pt-1 text-[10.5px] leading-snug text-ink-faint">Hulls: brass at anchor, cyan under sail, an amber lantern waits for you, a red flare failed. A chain on the rail: it read external text. Gold planks: its turns of the last hour.</li>
        </ul>
      )}
    </div>
  )
}

function Key({ c, children, ring, hex, gear }: { c: string; children: React.ReactNode; ring?: boolean; hex?: boolean; gear?: boolean }) {
  return (
    <li className="flex items-center gap-2">
      <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden>
        {hex && <polygon points="7,1 12.2,4 12.2,10 7,13 1.8,10 1.8,4" fill="none" stroke={c} strokeWidth="1.2" />}
        {ring && <circle cx="7" cy="7" r="5.4" fill="none" stroke={c} strokeWidth="1.2" />}
        {gear && <circle cx="7" cy="7" r="5.4" fill="none" stroke={c} strokeWidth="1.6" strokeDasharray="1.6 1.2" />}
        <circle cx="7" cy="7" r={ring || hex || gear ? 2.2 : 3.2} fill={hex || ring || gear ? '#22d3ee' : c} style={{ filter: `drop-shadow(0 0 3px ${c})` }} />
      </svg>
      <span>{children}</span>
    </li>
  )
}
