// The Ship (theseus-logs; made legible, theseus-hnof): the cockpit's hero view. One agent over one graph, on screen:
// every place a harbour, every session a ship and every task a boat in tow, every turn a bench across a ship's deck
// (newest at the bow), every tool call an oar of its turn with its result at the blade. Every shape says what it is in
// plain words (its nameplate, its hover card, the key, the tour); the watch answers the operator's questions; the depth
// gauge says how deep the camera reads. Everything shown is the daemon's own data, and moves only when it happens.
import { useEffect, useMemo, useRef, useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { Anchor, Crosshair, ExternalLink, HelpCircle, Maximize, Search, Waves } from 'lucide-react'
import { ShipEngine, type Hit } from '@/ship/engine'
import { LabelLayer, usdShort } from '@/ship/labels'
import { Minimap, type MinimapHandle } from '@/ship/Minimap'
import { Compass, EngineTelegraph, Nixie, ShipsClock } from '@/ship/instruments'
import { useShipLive, useShipSynthetic, type ShipData } from '@/ship/useShipData'
import { placeOf, type ShipModel, type Vessel } from '@/ship/model'
import { HoverCard } from '@/ship/HoverCard'
import { Key } from '@/ship/Key'
import type { KeyLine } from '@/ship/keyset'
import { Tour } from '@/ship/Tour'
import { DepthGauge } from '@/ship/Depth'
import { Coins } from '@/ship/Coins'
import { benchLine, count, depthOf, stateWord, type Depth } from '@/ship/words'
import { ShipBoundary, ShipFallback } from '@/ship/NoWebGL'
import { Watch, type WatchFocus, type WatchTarget } from '@/ship/Watch'
import { hasWebGL, NO_WEBGL, tryBuild } from '@/ship/webgl'
import { CallInspector } from '@/components/CallInspector'
import { ModelInspector } from '@/components/ModelInspector'
import { PlankStrip } from '@/components/brass'
import { useCalm } from '@/lib/calm'
import { useConn } from '@/lib/rpc'
import { useWorld } from '@/lib/world'
import { ago, cn, short, stamp } from '@/lib/format'

// The synthetic fleet is for measuring, in dev and bench builds only (never in a production build).
const SYNTH = (import.meta.env.DEV || import.meta.env.MODE === 'bench') && new URLSearchParams(window.location.search).has('synthetic')

// Probed once a page: WebGL turned off comes back only with a browser restart, and a context that fails later is
// caught where the engine is built.
let webgl: boolean | undefined
const canDraw = () => (webgl ??= hasWebGL(() => document.createElement('canvas')))

export default function ShipRoute() {
  // Without WebGL (hardware acceleration off) the Ship can't draw: its place says why and how to fix it, and the rest
  // of the cockpit works (theseus-9k53). The probe runs before the Ship reads anything.
  const [failed, setFailed] = useState<string | null>(() => (canDraw() ? null : NO_WEBGL))
  if (failed !== null) return <ShipFallback reason={failed} />
  return <ShipBoundary>{SYNTH ? <ShipSynthetic onFail={setFailed} /> : <ShipLive onFail={setFailed} />}</ShipBoundary>
}

type OnFail = (reason: string) => void

function ShipLive({ onFail }: { onFail: OnFail }) {
  const [params] = useSearchParams()
  const world = useWorld()
  return <ShipView data={useShipLive(params.get('s') ?? undefined, world)} onFail={onFail} />
}

function ShipSynthetic({ onFail }: { onFail: OnFail }) {
  return <ShipView data={useShipSynthetic()} onFail={onFail} />
}

declare global {
  interface Window { __ship?: { stats: ShipEngine['stats']; lights: number; vessels: number; mountedAt: number } }
}


function ShipView({ data, onFail }: { data: ShipData; onFail: OnFail }) {
  const [params, setParams] = useSearchParams()
  const calm = useCalm((s) => s.calm)
  const setCalm = useCalm((s) => s.setCalm)
  const [root, setRoot] = useState<HTMLDivElement | null>(null)
  const host = useRef<HTMLDivElement>(null)
  const labelsRoot = useRef<HTMLDivElement>(null)
  const minimap = useRef<MinimapHandle>(null)
  const [engine, setEngine] = useState<ShipEngine | null>(null)
  const labels = useRef<LabelLayer | null>(null)
  const [hover, setHover] = useState<{ hit: Hit; x: number; y: number } | null>(null)
  const fitted = useRef(false)
  // What is lit on the chart: a key line kept on (clicked) or rested on, else the watch's plate.
  const [overlay, setOverlay] = useState<WatchFocus | null>(null)
  const [keyPreview, setKeyPreview] = useState<KeyLine | null>(null)
  const [keyPinned, setKeyPinned] = useState<KeyLine | null>(null)
  const [tour, setTour] = useState(false)
  const [depth, setDepth] = useState<Depth>('fleet')
  const model = data.model
  const selId = params.get('s') ?? undefined
  const sel = model && selId ? model.byId.get(selId) ?? -1 : -1
  const hlId = params.get('n') ?? undefined
  const benchId = params.get('b') ?? undefined
  const bench = model && sel >= 0 && benchId ? model.vessels[sel].benches.find((b) => model.benches[b].turnId === benchId) ?? -1 : -1
  const bench_ = bench >= 0 && model ? model.benches[bench] : undefined
  const inspector = !!params.get('call') || !!params.get('msg')
  const benchMode = params.has('bench')
  const pin = Number(params.get('scale')) || undefined
  // In Live mode the sea rolls, always; `?swell=0` stills it for one page (screenshots, benches), beside `?calm=1`.
  const swell = params.get('swell') !== '0'

  // The engine lives as long as the view.
  useEffect(() => {
    const mountedAt = performance.now()
    // A probe can pass and the renderer's context still fail: then the Ship's place says so too.
    const made = tryBuild(() => new ShipEngine(host.current!, { calm: useCalm.getState().calm, bench: benchMode, scale: pin, swell }))
    if ('failed' in made) { onFail(made.failed); return }
    const e = made.engine
    // The title above, the watch at the right, and the instruments below cover the canvas's edges: a fit keeps the
    // fleet between them.
    e.insets = { top: 112, right: 310, bottom: 196, left: 28 }
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
  }, [benchMode, pin, swell, onFail])

  useEffect(() => { engine?.setCalm(calm) }, [engine, calm])
  const lit = keyPinned ?? keyPreview
  useEffect(() => {
    const f = lit ? { vessels: lit.vessels, lights: lit.lights } : overlay ? { vessels: overlay.vessels, lights: overlay.lights } : null
    engine?.setFocus(f)
  }, [engine, lit, overlay, model])

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

  // The first visit: the tour, once the fleet is read (until it is done or skipped, or `?notour`).
  const [toured, setToured] = useState(() => localStorage.getItem('cockpit.ship.tour') === 'done' || new URLSearchParams(window.location.search).has('notour'))
  const firstVisit = !toured && !!engine && !!model && data.progress >= 1 && !data.synthetic && model.vessels.length > 0
  const touring = tour || firstVisit
  const closeTour = () => { localStorage.setItem('cockpit.ship.tour', 'done'); setToured(true); setTour(false) }

  const flyTo = (t: WatchTarget) => {
    const m = engine?.model
    if (!engine || !m) return
    const li = t.node ? m.lightById.get(t.node) : undefined
    const vi = t.session ? m.byId.get(t.session) : li !== undefined ? m.lights[li].vessel : undefined
    if (vi === undefined) return
    setParams((p) => { p.set('s', m.vessels[vi].id); if (t.node && li !== undefined) p.set('n', t.node); else p.delete('n'); p.delete('b'); return p }, { replace: true })
    if (li !== undefined) engine.flyToLight(li)
    else engine.flyToVessel(vi)
  }

  // Pointer and frame hooks.
  const selRef = useRef(sel)
  const hoverIdx = hover?.hit.kind === 'vessel' ? hover.hit.vessel : -1
  const hoverRef = useRef(hoverIdx)
  const inspectorRef = useRef(inspector)
  useEffect(() => {
    selRef.current = sel
    hoverRef.current = hoverIdx
    inspectorRef.current = inspector
    engine?.requestRender()
  }, [sel, hoverIdx, inspector, engine])
  useEffect(() => {
    if (!engine) return
    engine.setHooks({
      onHover: (hit, x, y) => {
        setHover(hit ? { hit, x, y } : null)
        const m = engine.model
        engine.setHovered(hit?.kind === 'vessel' ? hit.vessel : hit?.kind === 'bench' && m ? m.benches[hit.bench].vessel : -1)
      },
      onClick: (hit) => {
        const m = engine.model
        if (!m) return
        if (!hit) return
        if (hit.kind === 'vessel') {
          const v = m.vessels[hit.vessel]
          setParams((p) => { p.set('s', v.id); p.delete('n'); p.delete('b'); p.delete('call'); p.delete('msg'); return p }, { replace: true })
          engine.flyToVessel(hit.vessel)
        } else if (hit.kind === 'bench') {
          const b = m.benches[hit.bench]
          setParams((p) => { p.set('s', m.vessels[b.vessel].id); p.set('b', b.turnId); p.delete('n'); p.delete('call'); p.delete('msg'); return p }, { replace: true })
          engine.flyToBench(hit.bench)
        } else {
          const l = m.lights[hit.light]
          setParams((p) => {
            p.set('s', l.sessionId)
            p.set('n', l.id)
            p.delete('call'); p.delete('msg')
            if (l.turnId) p.set('b', l.turnId)
            if ((l.kind === 'call' || l.kind === 'result') && (l.toolUseId || l.correlationId)) p.set('call', (l.toolUseId ?? l.correlationId)!)
            if (l.kind === 'model') p.set('msg', l.id)
            return p
          }, { replace: true })
        }
      },
      onDouble: (hit) => {
        if (hit?.kind === 'vessel') engine.flyToVessel(hit.vessel)
        else if (hit?.kind === 'bench') engine.flyToBench(hit.bench)
        else if (hit?.kind === 'light') engine.flyToLight(hit.light)
      },
      onFrame: (moved) => {
        labels.current?.update(engine, selRef.current, hoverRef.current)
        if (moved) minimap.current?.draw()
        // The depth gauge: how big the biggest vessel near the middle of the view is. A vessel is near it when the
        // nearest point of its keel is (close in on a long ship's bow, its centre is off the screen).
        const m = engine.model
        if (!m || !host.current) return
        const W = host.current.clientWidth
        const H = host.current.clientHeight
        let px = 0
        m.vessels.forEach((v, i) => {
          const now = engine.vesselNow(i)
          const hx = Math.cos(now.heading) * v.length * 0.5
          const hz = Math.sin(now.heading) * v.length * 0.5
          const a = engine.project(now.x - hx, 0, now.z - hz)
          const b = engine.project(now.x + hx, 0, now.z + hz)
          if (!a.on && !b.on) return
          const dx = b.x - a.x
          const dy = b.y - a.y
          const t = Math.max(0, Math.min(1, ((W / 2 - a.x) * dx + (H / 2 - a.y) * dy) / Math.max(1e-6, dx * dx + dy * dy)))
          const near = { x: a.x + dx * t, y: a.y + dy * t }
          if (Math.abs(near.x - W / 2) > W * 0.35 || Math.abs(near.y - H / 2) > H * 0.4) return
          px = Math.max(px, v.length * engine.pixelsPerUnit(now.x - hx + 2 * hx * t, now.z - hz + 2 * hz * t))
        })
        const d = depthOf(px, inspectorRef.current)
        setDepth((x) => (x === d ? x : d))
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

  /** Go to a depth: the fleet; the selected ship (or the biggest near the middle); its turn (the selected, the running,
   *  or the newest); that turn's newest call's inspector. */
  const goDepth = (d: Depth) => {
    const m = engine?.model
    if (!engine || !m) return
    if (d === 'fleet') { engine.fit(); setParams((p) => { p.delete('call'); p.delete('msg'); return p }, { replace: true }); return }
    const vi = sel >= 0 ? sel : m.vessels.reduce((best, v, i) => (best < 0 || v.benches.length > m.vessels[best].benches.length ? i : best), -1)
    if (vi < 0) return
    const v = m.vessels[vi]
    const bi = bench >= 0 ? bench : v.activeBench >= 0 ? v.activeBench : v.benches[v.benches.length - 1] ?? -1
    if (d === 'ship') {
      setParams((p) => { p.set('s', v.id); p.delete('call'); p.delete('msg'); return p }, { replace: true })
      engine.flyToVessel(vi)
      return
    }
    if (bi < 0) return
    const b = m.benches[bi]
    if (d === 'turn') {
      setParams((p) => { p.set('s', v.id); p.set('b', b.turnId); p.delete('call'); p.delete('msg'); return p }, { replace: true })
      engine.flyToBench(bi)
      return
    }
    const call = [...b.lights].reverse().map((i) => m.lights[i]).find((l) => l.kind === 'call' && (l.toolUseId || l.correlationId))
    const msg = [...b.lights].reverse().map((i) => m.lights[i]).find((l) => l.kind === 'model')
    setParams((p) => {
      p.set('s', v.id); p.set('b', b.turnId)
      if (call) p.set('call', (call.toolUseId ?? call.correlationId)!)
      else if (msg) p.set('msg', msg.id)
      return p
    }, { replace: true })
    engine.flyToBench(bi)
  }

  // Keys: Home or 0 fits the fleet; Esc lets an overlay, then the selection, go (when no inspector is open); ? is the
  // tour; - and + step out and in a depth.
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null
      if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return
      if (e.metaKey || e.ctrlKey || e.altKey || touring) return
      if (e.key === 'Home' || e.key === '0') { e.preventDefault(); engine?.fit() }
      if (e.key === '?') { e.preventDefault(); setTour(true) }
      if (e.key === '-' || e.key === '_') goDepth(depth === 'call' ? 'turn' : depth === 'turn' ? 'ship' : 'fleet')
      if (e.key === '+' || e.key === '=') goDepth(depth === 'fleet' ? 'ship' : depth === 'ship' ? 'turn' : 'call')
      if (e.key === 'Escape' && !params.get('call') && !params.get('msg')) {
        if (keyPinned || overlay) { setKeyPinned(null); setOverlay(null); return }
        setParams((p) => { p.delete('s'); p.delete('n'); p.delete('b'); return p }, { replace: true })
      }
    }
    window.addEventListener('keydown', k)
    return () => window.removeEventListener('keydown', k)
  })

  const vessel = model && sel >= 0 ? model.vessels[sel] : undefined
  const h = data.health
  const status = useConn((s) => s.status)
  // The time machine: the gauges read the fold at its moment, not today's health.
  const g = data.past?.gauges

  return (
    <div ref={setRoot} className="ship-root relative h-full w-full overflow-hidden" data-calm={calm ? '1' : ''}>
      <div ref={host} className="absolute inset-0" />
      <div ref={labelsRoot} className="ship-labels pointer-events-none absolute inset-0 overflow-hidden" />
      {data.progress < 1 && <div className="pointer-events-none absolute inset-x-0 top-0" title="Reading the graph"><PlankStrip progress={data.progress} height={6} /></div>}

      <Cartouche model={model} synthetic={data.synthetic} live={status === 'open'} error={data.error} asOf={data.past?.t} />

      <div data-ship-ui className="absolute right-3 top-4 flex items-center gap-1.5">
        <BrassButton title="Fly to a session or a call (Ctrl+K)" onClick={() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true }))}>
          <Search size={13} /> Fly to <span className="kbd ml-1">⌘K</span>
        </BrassButton>
        <BrassButton title="See the whole fleet (Home)" onClick={() => engine?.fit()}><Maximize size={13} /> Fleet</BrassButton>
        {vessel && <BrassButton title="Back to the selected session" onClick={() => engine?.flyToVessel(sel)}><Crosshair size={13} /> Ship</BrassButton>}
        <BrassButton title={calm ? 'Calm: no motion or glow. Click for the full hologram.' : 'Calm mode stills the sea and drops the motion, the glow, and the particles'} on={calm} onClick={() => setCalm(!calm)}>
          {calm ? <Anchor size={13} /> : <Waves size={13} />} {calm ? 'Calm' : 'Live'}
        </BrassButton>
        <BrassButton title="The tour: what each shape is, on the chart (?)" onClick={() => setTour(true)}><HelpCircle size={13} /> Tour</BrassButton>
      </div>

      {vessel && model && <VesselCard v={vessel} model={model} bench={bench_} now={data.past?.t} reachCap={data.reachCap} onClose={() => setParams((p) => { p.delete('s'); p.delete('n'); p.delete('b'); return p }, { replace: true })} />}
      {hover && model && !touring && <HoverCard hover={hover} model={model} />}
      <Key model={model} pinned={keyPinned?.id ?? null} onPreview={setKeyPreview} onPin={setKeyPinned} onTour={() => setTour(true)} />

      <div data-ship-ui className="ship-console pointer-events-auto absolute bottom-3 left-1/2 flex -translate-x-1/2 items-end gap-2.5 px-4 pb-2 pt-2.5">
        {g ? (
          <Compass live={g.profile ?? data.profiles?.live} profiles={data.profiles?.profiles.map((p) => p.name) ?? []}
            model={data.profiles?.profiles.find((p) => p.name === g.profile)?.model} />
        ) : (
          <Compass live={data.profiles?.live ?? h?.profile} profiles={data.profiles?.profiles.map((p) => p.name) ?? []} model={h?.model} />
        )}
        <EngineTelegraph accepting={g ? g.accepting : h?.kernel.accepting} running={g ? g.running : h?.kernel.executions_by_state.running ?? model?.stats.running ?? 0}
          ceiling={h?.kernel.admission_ceiling ?? 8} held={g ? 0 : h?.kernel.turns_held ?? 0} />
        <ShipsClock uptime={g ? g.uptimeSecs ?? undefined : h?.uptime_secs} version={h?.version} down={!!g && g.uptimeSecs === null} />
        <Nixie value={data.tpm} label="Tokens / min" title="Tokens a minute: input, cache, and output of every model call in the last sixty seconds (provider.call rows)." />
      </div>

      <div data-ship-ui className="ship-watch-slot pointer-events-auto absolute right-3 top-[64px]">
        <Watch model={model} data={data} focus={overlay} onFocus={(f) => { setKeyPinned(null); setOverlay(f) }} onFly={flyTo} />
      </div>

      {/* The porthole and the depth gauge stand left of the watch's column, in the foot's row; when the watch folds to
          its strip at the top (under 1400 px wide or 900 px tall), they move to the right edge, clear of the console. */}
      <div className="ship-depth-slot absolute bottom-[200px] right-3 [@media(min-width:1400px)_and_(min-height:900px)]:right-[306px]"><DepthGauge depth={depth} onGo={goDepth} /></div>
      <div data-ship-ui className="ship-porthole-slot absolute bottom-3 right-3 [@media(min-width:1400px)_and_(min-height:900px)]:right-[306px]"><Minimap ref={minimap} engine={engine} model={model} selected={sel} /></div>

      <Coins engine={engine} host={root} />
      {touring && engine && model && root && <Tour engine={engine} model={model} host={root} onClose={closeTour} />}

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

function Cartouche({ model, synthetic, live, error, asOf }: { model: ShipModel | null; synthetic: boolean; live: boolean; error?: string; asOf?: number }) {
  const s = model?.stats
  const failed = model?.vessels.filter((v) => v.rig === 'flare').length ?? 0
  return (
    <div data-ship-ui className="ship-cartouche pointer-events-none absolute left-4 top-4">
      <div className="flex items-baseline gap-3">
        <h1 className="ship-title">The Ship</h1>
        {synthetic && <span className="ship-synthetic">synthetic fleet · dev only</span>}
        {asOf !== undefined && <span className="ship-engraved text-[11px] !text-wait">as of {stamp(asOf)}</span>}
      </div>
      <div className="mt-0.5 flex flex-wrap gap-x-3 text-[12px] text-ink-dim">
        <span><b className="num text-ink">{s ? s.sessions - s.tasks : '…'}</b> {s && s.sessions - s.tasks === 1 ? 'session' : 'sessions'}</span>
        {!!s?.tasks && <span><b className="num text-ink">{s.tasks}</b> {s.tasks === 1 ? 'task' : 'tasks'}</span>}
        <span><b className="num text-ink">{model ? model.benches.length : '…'}</b> turns</span>
        <span className={s?.running ? 'text-live' : ''}><b className="num">{s?.running ?? 0}</b> working</span>
        {!!s?.waiting && <span className="text-wait"><b className="num">{s.waiting}</b> waiting for you</span>}
        {!!failed && <span className="text-fault"><b className="num">{failed}</b> failed</span>}
      </div>
      {!synthetic && !live && <div className="mt-1 text-[11.5px] text-wait">the link to the daemon is down: reconnecting…</div>}
      {error && <div className="mt-1 text-[11.5px] text-fault">{error}</div>}
    </div>
  )
}

function VesselCard({ v, model, bench, onClose, now, reachCap }: { v: Vessel; model: ShipModel; bench?: ShipModel['benches'][number]; onClose: () => void; now?: number; reachCap?: ShipData['reachCap'] }) {
  const lights = useMemo(() => model.lights.filter((l) => l.sessionId === v.id), [model, v.id])
  const kinds = { user: 0, model: 0, call: 0, result: 0 } as Record<string, number>
  for (const l of lights) kinds[l.kind]++
  const tasks = model.vessels.filter((x) => x.parentId === v.id)
  const parent = v.parentId ? model.vessels[model.byId.get(v.parentId) ?? -1] : undefined
  const l1 = lights.filter((l) => l.l1 && l.kind === 'call').length
  const ext = lights.filter((l) => l.external).length
  const failed = v.benches.reduce((a, b) => a + model.benches[b].failed, 0)
  const st = stateWord(v)
  const tone = st.tone === 'live' ? 'text-live' : st.tone === 'wait' ? 'text-wait' : st.tone === 'fault' ? 'text-fault' : 'text-ink-dim'
  return (
    <aside data-ship-ui className="brass-card pointer-events-auto absolute left-4 top-[100px] w-[310px]">
      <header className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <div className="ship-engraved text-[10px]">{v.kind === 'task' ? `task ${v.taskShort ?? ''}` : `session · ${placeOf({ label: v.label, kind: v.kind }).label}`}</div>
          <h2 className="truncate font-display text-[16px] font-semibold text-ivory" title={v.title}>{v.title}</h2>
          <div className={cn('num text-[11.5px]', tone)}>{st.word} <span className="text-ink-faint">· {st.sea}</span></div>
        </div>
        <button onClick={onClose} title="Let the selection go (Esc)" className="rounded px-1 text-ink-faint hover:text-ink">×</button>
      </header>
      <dl className="num mt-2 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-[11.5px]">
        <dt className="text-ink-faint">turns</dt><dd className="text-ink">{v.turns} · last {ago(v.lastActive, now)}</dd>
        <dt className="text-ink-faint">calls</dt><dd className="text-ink">{count(kinds.model, 'model call')} · {count(kinds.call, 'tool call')}{failed ? <span className="text-fault">, {failed} failed</span> : null}</dd>
        <dt className="text-ink-faint">money</dt><dd className="text-ink">{usdShort(v.cost)}{v.limit ? ` of ${usdShort(v.limit)}` : ''}{v.reserved ? ` · ${usdShort(v.reserved)} held for tasks and calls` : ''}</dd>
        {(v.profile || v.model) && <><dt className="text-ink-faint">model</dt><dd className="truncate text-ink">{v.profile ?? '—'} · {v.model ?? '—'}</dd></>}
        {!!l1 && <><dt className="text-ink-faint">sandbox</dt><dd className="text-[#5eead4]">{count(l1, 'call')} sandboxed (L1)</dd></>}
        {!!ext && <><dt className="text-ink-faint">web</dt><dd className="text-think">{count(ext, 'result')} with text from the web</dd></>}
        {v.hold && <><dt className="text-ink-faint">holds</dt><dd className="text-wait" title={v.hold.url}>read {v.hold.tool} {v.hold.query ? `"${v.hold.query}"` : v.hold.url} · {ago(v.hold.since_ms, now)}</dd></>}
        {!!v.pendingConfirms && <><dt className="text-ink-faint">waiting</dt><dd className="text-wait">{count(v.pendingConfirms, 'approval')} for you</dd></>}
        {parent && <><dt className="text-ink-faint">started by</dt><dd className="truncate text-ink">{parent.title}</dd></>}
        {!!tasks.length && <><dt className="text-ink-faint">tasks</dt><dd className="text-ink">{count(tasks.length, 'task')} in tow</dd></>}
        {reachCap && <><dt className="text-ink-faint">currents</dt><dd className="text-wait" title="node.reach is one call a node, so the currents between vessels are read for this vessel's newest nodes only">for the newest {reachCap.read} of {reachCap.total} nodes</dd></>}
        <dt className="text-ink-faint">id</dt><dd className="text-ink-dim">{short(v.id)}</dd>
      </dl>
      {bench && (
        <div className="mt-2 rounded-md border border-line px-2 py-1.5">
          <div className="ship-engraved text-[9.5px]">turn {bench.n} of {v.benches.length}</div>
          {bench.preview && <div className="line-clamp-2 text-[11.5px] text-ink">“{bench.preview}”</div>}
          <div className="num text-[10.5px] text-ink-dim">{benchLine(bench, usdShort)}</div>
        </div>
      )}
      <div className="mt-2.5 flex flex-wrap gap-1.5">
        <Link to={`/session/${v.id}${bench ? `?turn=${encodeURIComponent(bench.turnId)}` : ''}`} className="brass-button"><ExternalLink size={12} /> {bench ? `Turn ${bench.n} in the session` : 'Session deck'}</Link>
        <Link to={`/ledger?q=${v.id}`} className="brass-button">Ledger rows</Link>
      </div>
      <p className="mt-2 text-[10.5px] leading-snug text-ink-faint">Click a bench for its turn, an oar for its call. Double-click to fly in.</p>
    </aside>
  )
}
