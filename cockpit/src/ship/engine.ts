// The Ship's engine (theseus-logs): three.js, drawn directly (no React reconciler between the data and the GPU).
// The scene is a handful of GPU objects whose buffers the model rewrites: the sea, the stars, the hulls, the sails,
// every node's light, the oars, the currents and tethers, the wakes, and the beacons. React draws the brass
// instruments around it.
//
// Motion means something is happening now. The loop renders only while something moves: the camera, a vessel
// changing slot, a fact's flare, a vessel under sail (its wake), a lantern (waiting for the operator), a job's gear,
// a model call streaming, a running task's current. An idle daemon draws one frame and stops. Calm mode stops the
// motion and the post-processing.
import * as THREE from 'three'
import { oarReach, type Light, type ShipModel } from './model'
import { Post } from './post'
import {
  BEACON_FRAG, BEACON_VERT, FLOW_FRAG, FLOW_VERT, HULL_FRAG, HULL_VERT, LIGHT_FRAG, LIGHT_VERT, LINE_FRAG,
  LINE_VERT, SAIL_FRAG, SAIL_VERT, SEA_FRAG, SEA_VERT, STAR_FRAG, STAR_VERT, WAKE_FRAG, WAKE_VERT,
} from './shaders'

export type Hit = { kind: 'vessel'; vessel: number } | { kind: 'light'; light: number }

export interface EngineHooks {
  onHover?: (hit: Hit | null, x: number, y: number) => void
  onClick?: (hit: Hit | null) => void
  onDouble?: (hit: Hit | null) => void
  /** After a frame; `moved` when the camera or a vessel moved (labels follow). */
  onFrame?: (moved: boolean) => void
}

const RIG = { anchor: 0, sail: 1, lantern: 2, flare: 3 } as const
const WAKE_PER = 30
const FLOW_PER_TETHER = 96
const FLOW_PER_CURRENT = 30
const FOV = 36

// Each kind's colour (HDR, so the neon blooms) and size, in world units.
const LIGHT_LOOK: Record<string, { c: [number, number, number]; s: number }> = {
  // In 0..1, so the 8-bit targets keep each hue; the bloom gives the glow.
  user: { c: [0.94, 0.89, 0.78], s: 0.62 },
  model: { c: [0.66, 0.55, 1.0], s: 0.66 },
  call: { c: [0.14, 0.88, 1.0], s: 0.46 },
  ok: { c: [0.18, 0.98, 0.48], s: 0.5 },
  failed: { c: [1.0, 0.45, 0.53], s: 0.54 },
  external: { c: [1.0, 0.47, 0.74], s: 0.56 },
}

function lightLook(l: Light) {
  if (l.kind === 'result') return l.external ? LIGHT_LOOK.external : l.failed ? LIGHT_LOOK.failed : LIGHT_LOOK.ok
  return LIGHT_LOOK[l.kind]
}

const easeInOut = (t: number) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2)

interface Tween { from: [number, number, number]; to: [number, number, number]; t0: number; dur: number; arc: number }

export class ShipEngine {
  readonly renderer: THREE.WebGLRenderer
  readonly camera = new THREE.PerspectiveCamera(FOV, 1, 0.1, 40000)
  private scene = new THREE.Scene()
  /** The sea and the stars: drawn only when the camera moves (the post's cache). */
  private seaScene = new THREE.Scene()
  private post = new Post()
  /** The internal resolution, as a share of the canvas's: lowered while frames run long, raised when they're quick.
   *  `pinnedScale` fixes it (for measuring). */
  scale = 1
  pinnedScale: number | null = null
  private devicePixels = 1
  private intervals: number[] = []
  private container: HTMLElement
  private ro: ResizeObserver
  private t0 = performance.now()
  private wall0 = Date.now()
  private raf = 0
  private disposed = false
  calm: boolean
  /** Render every frame (for measuring), whatever moves. */
  bench: boolean
  private hooks: EngineHooks = {}

  // The camera rig: a point on the sea, a distance, and a pitch that follows the zoom (top-down far out, tilted close).
  tx = 0
  tz = 0
  dist = 220
  minDist = 3.2
  maxDist = 600
  private tween: Tween | null = null
  /** What the camera keeps in view as the model changes (nodes arrive after the first frame, and vessels grow and
   *  move), until the operator moves it: the fleet, a vessel, or a light. */
  follow: { kind: 'fleet' } | { kind: 'vessel'; id: string } | { kind: 'light'; id: string } | null = { kind: 'fleet' }
  /** The canvas's margins covered by the instruments and the title, in CSS pixels: a fit keeps the fleet clear. */
  insets = { top: 0, right: 0, bottom: 0, left: 0 }

  // Vessel transforms: the goal from the model, the current (animated toward it), in a float texture.
  private cap = 0
  private vdata = new Float32Array(0)
  private vtex: THREE.DataTexture | null = null
  private cur = new Float32Array(0)
  private goal = new Float32Array(0)
  private settling = false

  private u = {
    uVessels: { value: null as THREE.Texture | null },
    uVesselW: { value: 1 },
    uTime: { value: 0 },
    uCalm: { value: 0 },
    uScale: { value: 500 },
    uPixel: { value: 1 },
  }
  private seaU = { uTarget: { value: new THREE.Vector2() }, uDist: { value: 200 }, uCenter: { value: new THREE.Vector2() }, uRose: { value: 0 } }
  private starU = { uTarget: this.seaU.uTarget, uScale: this.u.uScale }

  private sea: THREE.Mesh
  private stars: THREE.Points
  private hulls: THREE.Mesh
  private sails: THREE.Mesh
  private lights: THREE.Points
  private lines: THREE.LineSegments
  private flows: THREE.Points
  private wakes: THREE.Points
  private beacons: THREE.Points

  model: ShipModel | null = null
  private selected = -1
  private hovered = -1
  private highlight = -1
  /** Until when (engine seconds) something flares, so the loop keeps drawing. */
  private flareUntil = 0
  stats = { frames: 0, cpu: [] as number[], firstFrameAt: 0, firstFleetAt: 0, scale: 1 }

  constructor(container: HTMLElement, opts: { calm: boolean; bench?: boolean; scale?: number }) {
    this.container = container
    this.calm = opts.calm
    this.bench = !!opts.bench
    if (opts.scale) { this.pinnedScale = opts.scale; this.scale = opts.scale }
    this.renderer = new THREE.WebGLRenderer({ antialias: false, alpha: false, powerPreference: 'high-performance', stencil: false })
    this.devicePixels = Math.min(window.devicePixelRatio || 1, 1.5)
    this.renderer.setPixelRatio(this.devicePixels)
    this.renderer.setClearColor(0x030912, 1)
    this.renderer.domElement.className = 'ship-canvas'
    container.appendChild(this.renderer.domElement)


    const mat = (vertexShader: string, fragmentShader: string, extra: Record<string, unknown> = {}, blending: THREE.Blending = THREE.AdditiveBlending) =>
      new THREE.ShaderMaterial({
        uniforms: { ...this.u, ...extra } as Record<string, THREE.IUniform>,
        vertexShader, fragmentShader, transparent: true, depthTest: false, depthWrite: false, blending,
      })

    const seaGeo = new THREE.PlaneGeometry(1, 1).rotateX(-Math.PI / 2)
    this.sea = new THREE.Mesh(seaGeo, new THREE.ShaderMaterial({ uniforms: this.seaU, vertexShader: SEA_VERT, fragmentShader: SEA_FRAG, depthTest: false, depthWrite: false }))
    this.sea.renderOrder = 0

    this.stars = new THREE.Points(starGeometry(), new THREE.ShaderMaterial({
      uniforms: this.starU, vertexShader: STAR_VERT, fragmentShader: STAR_FRAG, transparent: true, depthTest: false, depthWrite: false, blending: THREE.AdditiveBlending,
    }))
    this.stars.renderOrder = 1

    const quadXZ = new THREE.PlaneGeometry(1, 1).rotateX(-Math.PI / 2)
    this.hulls = new THREE.Mesh(instanced(quadXZ), mat(HULL_VERT, HULL_FRAG, {}, THREE.NormalBlending))
    this.hulls.renderOrder = 2
    this.lines = new THREE.LineSegments(new THREE.BufferGeometry(), mat(LINE_VERT, LINE_FRAG))
    this.lines.renderOrder = 3
    this.flows = new THREE.Points(new THREE.BufferGeometry(), mat(FLOW_VERT, FLOW_FRAG))
    this.flows.renderOrder = 4
    this.wakes = new THREE.Points(new THREE.BufferGeometry(), mat(WAKE_VERT, WAKE_FRAG))
    this.wakes.renderOrder = 4
    this.lights = new THREE.Points(new THREE.BufferGeometry(), mat(LIGHT_VERT, LIGHT_FRAG))
    this.lights.renderOrder = 5
    this.sails = new THREE.Mesh(instanced(new THREE.PlaneGeometry(1, 1)), mat(SAIL_VERT, SAIL_FRAG, {}, THREE.NormalBlending))
    this.sails.renderOrder = 6
    this.beacons = new THREE.Points(new THREE.BufferGeometry(), mat(BEACON_VERT, BEACON_FRAG))
    this.beacons.renderOrder = 7

    for (const o of [this.sea, this.stars]) {
      o.frustumCulled = false
      this.seaScene.add(o)
    }
    for (const o of [this.hulls, this.lines, this.flows, this.wakes, this.lights, this.sails, this.beacons]) {
      o.frustumCulled = false
      this.scene.add(o)
    }
    this.ensureCapacity(64)
    // Compile every program now, while the first reads are in flight, so the first frame with the fleet doesn't wait
    // on the driver (in parallel where the driver can).
    if (this.renderer.extensions.has('KHR_parallel_shader_compile')) {
      void this.renderer.compileAsync(this.scene, this.camera).catch(() => {})
      void this.renderer.compileAsync(this.seaScene, this.camera).catch(() => {})
    } else {
      this.renderer.compile(this.scene, this.camera)
      this.renderer.compile(this.seaScene, this.camera)
    }
    this.setCalm(this.calm)
    this.ro = new ResizeObserver(() => this.resize())
    this.ro.observe(container)
    this.resize()
    this.bindPointer()
  }

  setHooks(h: EngineHooks) {
    this.hooks = h
    this.requestRender()
  }

  // ---------------------------------------------------------------- size and time

  private resize() {
    const w = Math.max(1, this.container.clientWidth)
    const h = Math.max(1, this.container.clientHeight)
    this.renderer.domElement.style.width = `${w}px`
    this.renderer.domElement.style.height = `${h}px`
    this.camera.aspect = w / h
    this.camera.updateProjectionMatrix()
    this.sizeTargets()
    this.applyCamera()
    this.requestRender()
  }

  /** The drawing buffer at the canvas's size times the device ratio and the resolution scale (the browser stretches
   *  it to the canvas), so every pass shrinks with the scale; the post's targets and the point sizes follow. */
  private sizeTargets() {
    const w = Math.max(1, this.container.clientWidth)
    const h = Math.max(1, this.container.clientHeight)
    const ip = this.devicePixels * this.scale
    this.renderer.setPixelRatio(ip)
    this.renderer.setSize(w, h, false)
    this.post.setSize(w * ip, h * ip, ip)
    this.u.uScale.value = (h * ip) / (2 * Math.tan(THREE.MathUtils.degToRad(FOV / 2)))
    this.u.uPixel.value = ip
  }

  /** Lower the internal resolution while frames run long, and raise it back when they're quick (not when pinned). */
  private adapt(interval: number) {
    if (this.pinnedScale !== null) return
    this.intervals.push(interval)
    if (this.intervals.length < 24) return
    const avg = this.intervals.reduce((a, b) => a + b, 0) / this.intervals.length
    this.intervals = []
    const next = avg > 26 ? Math.max(0.5, this.scale * 0.85) : avg < 14 ? Math.min(1, this.scale * 1.12) : this.scale
    if (Math.abs(next - this.scale) > 0.01) {
      this.scale = next
      this.sizeTargets()
    }
  }

  /** Engine seconds for a wall-clock instant; 0 for none or one before the engine started. */
  private secs(ms: number): number {
    if (!ms || ms < this.wall0 - 60_000) return 0
    return Math.max(0.001, (ms - this.wall0) / 1000)
  }

  private now(): number {
    return (performance.now() - this.t0) / 1000
  }

  setCalm(calm: boolean) {
    this.calm = calm
    this.u.uCalm.value = calm ? 1 : 0
    this.requestRender()
  }

  // ---------------------------------------------------------------- the camera

  private pitch(): number {
    const z = Math.min(1, Math.max(0, Math.log(this.maxDist / this.dist) / Math.log(this.maxDist / this.minDist)))
    return THREE.MathUtils.degToRad(24 + 34 * Math.pow(z, 1.2))
  }

  private applyCamera() {
    const p = this.pitch()
    this.camera.position.set(this.tx, this.dist * Math.cos(p), this.tz + this.dist * Math.sin(p))
    this.camera.lookAt(this.tx, 0, this.tz)
    this.camera.updateMatrixWorld()
    this.seaU.uTarget.value.set(this.tx, this.tz)
    this.seaU.uDist.value = this.dist
    this.sea.position.set(this.tx, 0, this.tz)
    const s = this.dist * 14 + 400
    this.sea.scale.set(s, 1, s)
    this.post.seaDirty = true
  }

  /** The sea point under a pixel of the canvas (CSS pixels), or null above the horizon. */
  seaPoint(px: number, py: number): THREE.Vector3 | null {
    const w = this.container.clientWidth
    const h = this.container.clientHeight
    const ndc = new THREE.Vector3((px / w) * 2 - 1, -(py / h) * 2 + 1, 0.5).unproject(this.camera)
    const dir = ndc.sub(this.camera.position).normalize()
    if (dir.y >= -1e-4) return null
    const t = -this.camera.position.y / dir.y
    return this.camera.position.clone().add(dir.multiplyScalar(t))
  }

  /** A world point to CSS pixels on the canvas. */
  project(x: number, y: number, z: number): { x: number; y: number; on: boolean } {
    const v = new THREE.Vector3(x, y, z).project(this.camera)
    const w = this.container.clientWidth
    const h = this.container.clientHeight
    return { x: (v.x + 1) * 0.5 * w, y: (1 - v.y) * 0.5 * h, on: v.z < 1 && v.x > -1.2 && v.x < 1.2 && v.y > -1.2 && v.y < 1.2 }
  }

  /** CSS pixels per world unit at a point (for "is it big enough to label"). */
  pixelsPerUnit(x: number, z: number): number {
    const a = this.project(x, 0, z)
    const b = this.project(x + 1, 0, z)
    return Math.hypot(b.x - a.x, b.y - a.y)
  }

  /** The four sea corners of the view, for the minimap. */
  footprint(): [number, number][] {
    const w = this.container.clientWidth
    const h = this.container.clientHeight
    return ([[0, 0], [w, 0], [w, h], [0, h]] as const).map(([x, y]) => {
      const p = this.seaPoint(x, y) ?? this.seaPoint(x, Math.max(y, h * 0.05))
      return p ? [p.x, p.z] as [number, number] : [this.tx, this.tz] as [number, number]
    })
  }

  private fitDistance(minX: number, maxX: number, minZ: number, maxZ: number): number {
    const tanH = Math.tan(THREE.MathUtils.degToRad(FOV / 2))
    const w = Math.max(20, maxX - minX)
    const h = Math.max(16, maxZ - minZ)
    return Math.max(h / (2 * tanH) * 1.32, w / (2 * tanH * this.camera.aspect) * 1.22)
  }

  /** Fit the whole fleet in the canvas's clear area (inside the insets), and keep fitting it until the operator moves. */
  fit(animate = true) {
    this.follow = { kind: 'fleet' }
    const b = this.model?.bounds ?? { minX: -40, maxX: 40, minZ: -30, maxZ: 30 }
    const W = Math.max(1, this.container.clientWidth)
    const H = Math.max(1, this.container.clientHeight)
    const ins = this.insets
    const sw = Math.max(120, W - ins.left - ins.right)
    const sh = Math.max(120, H - ins.top - ins.bottom)
    const tanH = Math.tan(THREE.MathUtils.degToRad(FOV / 2))
    const bw = Math.max(24, b.maxX - b.minX)
    const bh = Math.max(18, b.maxZ - b.minZ)
    const d = Math.min(this.maxDist, Math.max(this.minDist, Math.max((bh / (2 * tanH)) * (H / sh) * 1.12, (bw / (2 * tanH * this.camera.aspect)) * (W / sw) * 1.08)))
    // Aim so the fleet's centre lands in the clear area's centre: try, measure, correct (twice).
    const saved = { tx: this.tx, tz: this.tz, dist: this.dist }
    const cx = (b.minX + b.maxX) / 2
    const cz = (b.minZ + b.maxZ) / 2
    this.tx = cx; this.tz = cz; this.dist = d
    for (let k = 0; k < 2; k++) {
      this.applyCamera()
      const p = this.project(cx, 0, cz)
      const ppu = Math.max(1e-6, this.pixelsPerUnit(cx, cz))
      this.tx -= (ins.left + sw / 2 - p.x) / ppu
      this.tz -= (ins.top + sh / 2 - p.y) / ppu
    }
    const goal = { x: this.tx, z: this.tz }
    this.tx = saved.tx; this.tz = saved.tz; this.dist = saved.dist
    this.applyCamera()
    this.flyTo(goal.x, goal.z, d, animate, true)
  }

  flyTo(x: number, z: number, d: number, animate = true, keepFollow = false) {
    if (!keepFollow) this.follow = null
    d = Math.min(this.maxDist, Math.max(this.minDist, d))
    if (!animate || this.calm) {
      this.tween = null
      this.tx = x; this.tz = z; this.dist = d
      this.applyCamera()
      this.requestRender()
      return
    }
    const travel = Math.hypot(x - this.tx, z - this.tz)
    // Already there (a refollow after a model change that moved nothing): no flight.
    const goal = this.tween?.to ?? [this.tx, this.tz, this.dist]
    if (Math.hypot(x - goal[0], z - goal[1]) < 0.01 * d && Math.abs(Math.log(d / goal[2])) < 0.01) return
    const arc = Math.max(0, Math.min(this.maxDist, travel * 0.55) - Math.max(this.dist, d))
    this.tween = { from: [this.tx, this.tz, this.dist], to: [x, z, d], t0: performance.now(), dur: 900 + Math.min(700, travel * 2), arc }
    this.requestRender()
  }

  /** Fly to a vessel (its goal slot, where it is going), and keep it in view until the operator moves. */
  flyToVessel(i: number, animate = true) {
    const v = this.model?.vessels[i]
    if (!v) return
    const tanH = Math.tan(THREE.MathUtils.degToRad(FOV / 2))
    const W = Math.max(1, this.container.clientWidth)
    const H = Math.max(1, this.container.clientHeight)
    const ins = this.insets
    const sw = Math.max(120, W - ins.left - ins.right)
    const sh = Math.max(120, H - ins.top - ins.bottom)
    const d = Math.max((v.length * 1.35) / (2 * tanH * this.camera.aspect) * (W / sw), ((v.beam + 2 * oarReach(v.beam)) * 2.6) / (2 * tanH) * (H / sh))
    this.flyTo(v.x, v.z + d * 0.04, d, animate, true)
    this.follow = { kind: 'vessel', id: v.id }
  }

  /** Fly close to a light, and keep it in view until the operator moves. */
  flyToLight(i: number, animate = true) {
    const m = this.model
    const l = m?.lights[i]
    if (!m || !l) return
    const v = m.vessels[l.vessel]
    const c = Math.cos(v.heading)
    const s = Math.sin(v.heading)
    this.flyTo(v.x + l.lx * c - l.lz * s, v.z + l.lx * s + l.lz * c, Math.max(this.minDist * 2.4, 9), animate, true)
    this.follow = { kind: 'light', id: l.id }
  }

  /** Keep what the camera follows in view after the model changed. */
  private refollow() {
    const f = this.follow
    const m = this.model
    if (!f || !m) return
    if (f.kind === 'fleet') this.fit(true)
    else if (f.kind === 'vessel') { const i = m.byId.get(f.id); if (i !== undefined) this.flyToVessel(i) }
    else { const i = m.lightById.get(f.id); if (i !== undefined) this.flyToLight(i) }
  }

  panTo(x: number, z: number) {
    this.follow = null
    this.tween = null
    this.tx = x; this.tz = z
    this.applyCamera()
    this.requestRender()
  }

  private stepCamera(now: number): boolean {
    const tw = this.tween
    if (!tw) return false
    const t = Math.min(1, (now - tw.t0) / tw.dur)
    const e = easeInOut(t)
    this.tx = tw.from[0] + (tw.to[0] - tw.from[0]) * e
    this.tz = tw.from[1] + (tw.to[1] - tw.from[1]) * e
    // Far flights rise to see where they go, as a chart is read: out, across, and in.
    const dl = Math.exp(Math.log(tw.from[2]) + (Math.log(tw.to[2]) - Math.log(tw.from[2])) * e)
    this.dist = dl + tw.arc * Math.sin(Math.PI * e)
    this.applyCamera()
    if (t >= 1) this.tween = null
    return true
  }

  // ---------------------------------------------------------------- the vessel texture

  private ensureCapacity(n: number) {
    if (n <= this.cap) return
    let cap = Math.max(64, this.cap)
    while (cap < n) cap *= 2
    if (cap > 8192) cap = 8192
    const data = new Float32Array(cap * 4 * 4)
    data.set(this.vdata.subarray(0, Math.min(this.vdata.length, data.length)))
    this.vdata = data
    this.vtex?.dispose()
    const tex = new THREE.DataTexture(data, cap, 4, THREE.RGBAFormat, THREE.FloatType)
    tex.minFilter = THREE.NearestFilter
    tex.magFilter = THREE.NearestFilter
    tex.needsUpdate = true
    this.vtex = tex
    this.cap = cap
    this.u.uVessels.value = tex
    this.u.uVesselW.value = cap
    const cur = new Float32Array(cap * 3)
    cur.set(this.cur.subarray(0, Math.min(this.cur.length, cur.length)))
    this.cur = cur
    const goal = new Float32Array(cap * 3)
    goal.set(this.goal.subarray(0, Math.min(this.goal.length, goal.length)))
    this.goal = goal
  }

  private texel(i: number, row: number): number {
    return (row * this.cap + i) * 4
  }

  private writeVesselFlags() {
    const m = this.model
    if (!m) return
    m.vessels.forEach((v, i) => {
      const flags = (v.hold ? 1 : 0) + (i === this.selected ? 2 : 0) + (i === this.hovered ? 4 : 0) + (v.kind === 'task' ? 8 : 0)
      this.vdata[this.texel(i, 1) + 2] = flags
    })
    if (this.vtex) this.vtex.needsUpdate = true
  }

  private stepVessels(dt: number): boolean {
    if (!this.settling || !this.model) return false
    const n = this.model.vessels.length
    const k = 1 - Math.exp(-dt * 7)
    let moving = false
    for (let i = 0; i < n; i++) {
      for (let j = 0; j < 3; j++) {
        const a = this.cur[i * 3 + j]
        const g = this.goal[i * 3 + j]
        const nv = Math.abs(g - a) < 0.002 ? g : a + (g - a) * k
        if (nv !== g) moving = true
        this.cur[i * 3 + j] = nv
      }
      const o = this.texel(i, 0)
      this.vdata[o] = this.cur[i * 3]
      this.vdata[o + 1] = this.cur[i * 3 + 2]
      this.vdata[o + 2] = this.cur[i * 3 + 1]
    }
    if (this.vtex) this.vtex.needsUpdate = true
    this.settling = moving
    return true
  }

  // ---------------------------------------------------------------- the model

  setModel(m: ShipModel, opts: { fit?: boolean } = {}) {
    const old = this.model
    const oldIdx = new Map<string, number>()
    old?.vessels.forEach((v, i) => oldIdx.set(v.id, i))
    const prevCur = this.cur.slice()
    const n = m.vessels.length
    this.ensureCapacity(n)
    const now = this.now()
    let flare = 0
    m.vessels.forEach((v, i) => {
      this.goal[i * 3] = v.x
      this.goal[i * 3 + 1] = v.heading
      this.goal[i * 3 + 2] = v.z
      const was = oldIdx.get(v.id)
      if (was !== undefined) {
        this.cur[i * 3] = prevCur[was * 3]
        this.cur[i * 3 + 1] = prevCur[was * 3 + 1]
        this.cur[i * 3 + 2] = prevCur[was * 3 + 2]
      } else {
        this.cur[i * 3] = v.x
        this.cur[i * 3 + 1] = v.heading
        this.cur[i * 3 + 2] = v.z
      }
      const o0 = this.texel(i, 0)
      this.vdata[o0] = this.cur[i * 3]
      this.vdata[o0 + 1] = this.cur[i * 3 + 2]
      this.vdata[o0 + 2] = this.cur[i * 3 + 1]
      this.vdata[o0 + 3] = v.length
      const o1 = this.texel(i, 1)
      this.vdata[o1] = v.beam
      this.vdata[o1 + 1] = RIG[v.rig]
      const born = this.secs(v.born)
      this.vdata[o1 + 3] = born
      const o2 = this.texel(i, 2)
      const fl = this.secs(v.flareAt)
      this.vdata[o2] = fl
      this.vdata[o2 + 1] = v.streaming ? 1 : 0
      this.vdata[o2 + 2] = v.planks > 0 ? Math.min(1, v.goldPlanks / v.planks) : 0
      this.vdata[o2 + 3] = hashSeed(v.id)
      if (born) flare = Math.max(flare, born + 2.5)
      if (fl) flare = Math.max(flare, fl + 3)
    })
    this.settling = true
    // Keep selection and hover on the same vessels.
    const sel = this.selected >= 0 ? old?.vessels[this.selected]?.id : undefined
    const hov = this.hovered >= 0 ? old?.vessels[this.hovered]?.id : undefined
    const hl = this.highlight >= 0 ? old?.lights[this.highlight]?.id : undefined
    this.model = m
    this.selected = sel ? m.byId.get(sel) ?? -1 : -1
    this.hovered = hov ? m.byId.get(hov) ?? -1 : -1
    this.highlight = hl ? m.lightById.get(hl) ?? -1 : -1
    this.writeVesselFlags()

    this.rebuildHulls(n)
    flare = Math.max(flare, this.rebuildLights(m))
    this.rebuildLines(m)
    this.rebuildFlows(m)
    this.rebuildWakes(n)
    this.rebuildBeacons(m)
    if (flare > now) this.flareUntil = Math.max(this.flareUntil, flare)

    const b = m.bounds
    const fitD = this.fitDistance(b.minX, b.maxX, b.minZ, b.maxZ)
    this.maxDist = Math.max(160, fitD * 2.4)
    this.seaU.uCenter.value.set((b.minX + b.maxX) / 2, (b.minZ + b.maxZ) / 2)
    this.seaU.uRose.value = Math.max(30, Math.hypot(b.maxX - b.minX, b.maxZ - b.minZ) * 0.42)
    this.post.seaDirty = true
    if (opts.fit) this.fit(false)
    else this.refollow()
    this.requestRender()
  }

  private rebuildHulls(n: number) {
    for (const mesh of [this.hulls, this.sails]) {
      const g = mesh.geometry as THREE.InstancedBufferGeometry
      const idx = new Float32Array(n)
      for (let i = 0; i < n; i++) idx[i] = i
      g.setAttribute('aIdx', new THREE.InstancedBufferAttribute(idx, 1))
      g.instanceCount = n
    }
  }

  private lightPos(l: Light) {
    const i = l.vessel
    const x = this.cur[i * 3]
    const h = this.cur[i * 3 + 1]
    const z = this.cur[i * 3 + 2]
    const c = Math.cos(h)
    const s = Math.sin(h)
    return { x: x + l.lx * c - l.lz * s, y: l.ly, z: z + l.lx * s + l.lz * c }
  }

  private rebuildLights(m: ShipModel): number {
    const n = m.lights.length
    const pos = new Float32Array(n * 3)
    const idx = new Float32Array(n)
    const col = new Float32Array(n * 3)
    const size = new Float32Array(n)
    const flags = new Float32Array(n)
    const times = new Float32Array(n * 2)
    let until = 0
    m.lights.forEach((l, i) => {
      pos[i * 3] = l.lx; pos[i * 3 + 1] = l.ly; pos[i * 3 + 2] = l.lz
      idx[i] = l.vessel
      const look = lightLook(l)
      col.set(look.c, i * 3)
      size[i] = look.s
      flags[i] = (l.l1 ? 1 : 0) + (l.external ? 2 : 0) + (l.running ? 4 : 0) + (l.failed ? 8 : 0) + (i === this.highlight ? 16 : 0)
      const born = this.secs(l.born)
      times[i * 2] = born
      if (born) until = Math.max(until, born + 2.2)
    })
    const g = new THREE.BufferGeometry()
    g.setAttribute('position', new THREE.BufferAttribute(pos, 3))
    g.setAttribute('aIdx', new THREE.BufferAttribute(idx, 1))
    g.setAttribute('aColor', new THREE.BufferAttribute(col, 3))
    g.setAttribute('aSize', new THREE.BufferAttribute(size, 1))
    g.setAttribute('aFlags', new THREE.BufferAttribute(flags, 1))
    g.setAttribute('aTimes', new THREE.BufferAttribute(times, 2))
    this.lights.geometry.dispose()
    this.lights.geometry = g
    return until
  }

  private rebuildLines(m: ShipModel) {
    const pos: number[] = []
    const idx: number[] = []
    const col: number[] = []
    const seg = (i: number, a: [number, number, number], b: [number, number, number], c: [number, number, number, number]) => {
      pos.push(...a, ...b); idx.push(i, i); col.push(...c, ...c)
    }
    m.vessels.forEach((v, i) => {
      if (!v.nodes) return
      const half = v.length / 2
      // The keel, in old gold, stern to bow.
      seg(i, [-half * 0.9, 0.12, 0], [half * 0.92, 0.12, 0], [0.84 * 0.55, 0.65 * 0.55, 0.28 * 0.55, 0.9])
    })
    for (const l of m.lights) {
      if (l.kind === 'call') {
        const c = l.failed ? [0.98, 0.44, 0.52] : [0.13, 0.83, 0.93]
        seg(l.vessel, [l.lx, 0.12, 0], [l.lx, l.ly, l.lz], [c[0] * 0.32, c[1] * 0.32, c[2] * 0.32, 0.9])
      } else if (l.kind === 'result' && l.ox !== undefined && l.oz !== undefined) {
        const c = l.external ? [0.96, 0.45, 0.71] : l.failed ? [0.98, 0.44, 0.52] : [0.16, 0.86, 0.42]
        seg(l.vessel, [l.ox, l.ly, l.oz], [l.lx, l.ly, l.lz], [c[0] * 0.5, c[1] * 0.5, c[2] * 0.5, 0.9])
      }
    }
    // Each formation's mooring ring, faint brass, in world coordinates.
    for (const f of m.formations) {
      const r = f.radius
      const N = 96
      for (let k = 0; k < N; k++) {
        if (k % 6 === 5) continue
        const a0 = (k / N) * Math.PI * 2
        const a1 = ((k + 1) / N) * Math.PI * 2
        seg(-1, [f.x + Math.cos(a0) * r, 0.02, f.z + Math.sin(a0) * r], [f.x + Math.cos(a1) * r, 0.02, f.z + Math.sin(a1) * r], [0.69 * 0.22, 0.55 * 0.22, 0.34 * 0.22, 0.9])
      }
    }
    const g = new THREE.BufferGeometry()
    g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3))
    g.setAttribute('aIdx', new THREE.Float32BufferAttribute(idx, 1))
    g.setAttribute('aColor', new THREE.Float32BufferAttribute(col, 4))
    this.lines.geometry.dispose()
    this.lines.geometry = g
  }

  private rebuildFlows(m: ShipModel) {
    const from: number[] = []
    const to: number[] = []
    const param: number[] = []
    const col: number[] = []
    const size: number[] = []
    for (const t of m.tethers) {
      const a = m.vessels[t.from]
      const b = m.vessels[t.to]
      const burst = this.secs(t.reportAt)
      for (let k = 0; k < FLOW_PER_TETHER; k++) {
        from.push(t.from, -a.length / 2, 0)
        to.push(t.to, b.length / 2, 0)
        param.push(k / FLOW_PER_TETHER, t.live ? 0.2 : 0, 0.06, burst)
        const done = t.state === 'complete' || t.state === 'succeeded'
        const c = t.live ? [0.13 * 1.6, 0.83 * 1.6, 0.93 * 1.6] : done ? [0.84 * 0.9, 0.65 * 0.9, 0.28 * 0.9] : [0.6, 0.55, 0.45]
        col.push(...c)
        size.push(t.live ? 0.42 : 0.3)
      }
      if (burst) this.flareUntil = Math.max(this.flareUntil, burst + 3.2)
    }
    for (const c of m.currents) {
      const la = c.fromLight !== undefined ? m.lights[c.fromLight] : undefined
      const lb = c.toLight !== undefined ? m.lights[c.toLight] : undefined
      const live = m.vessels[c.from].rig === 'sail' || m.vessels[c.to].rig === 'sail'
      for (let k = 0; k < FLOW_PER_CURRENT; k++) {
        from.push(c.from, la?.lx ?? 0, la?.lz ?? 0)
        to.push(c.to, lb?.lx ?? 0, lb?.lz ?? 0)
        param.push(k / FLOW_PER_CURRENT, live ? 0.12 : 0, -0.18, 0)
        col.push(0.96 * 1.2, 0.45 * 1.2, 0.71 * 1.2)
        size.push(0.34)
      }
    }
    const n = size.length
    const g = new THREE.BufferGeometry()
    g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(n * 3), 3))
    g.setAttribute('aFrom', new THREE.Float32BufferAttribute(from, 3))
    g.setAttribute('aTo', new THREE.Float32BufferAttribute(to, 3))
    g.setAttribute('aParam', new THREE.Float32BufferAttribute(param, 4))
    g.setAttribute('aColor', new THREE.Float32BufferAttribute(col, 3))
    g.setAttribute('aSize', new THREE.Float32BufferAttribute(size, 1))
    this.flows.geometry.dispose()
    this.flows.geometry = g
  }

  private rebuildWakes(n: number) {
    const total = n * WAKE_PER
    const idx = new Float32Array(total)
    const seed = new Float32Array(total * 2)
    for (let i = 0; i < n; i++) {
      for (let k = 0; k < WAKE_PER; k++) {
        const j = i * WAKE_PER + k
        idx[j] = i
        seed[j * 2] = (k / WAKE_PER + i * 0.137) % 1
        seed[j * 2 + 1] = k % 2 ? 1 : -1
      }
    }
    const g = new THREE.BufferGeometry()
    g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(total * 3), 3))
    g.setAttribute('aIdx', new THREE.BufferAttribute(idx, 1))
    g.setAttribute('aSeed', new THREE.BufferAttribute(seed, 2))
    this.wakes.geometry.dispose()
    this.wakes.geometry = g
  }

  private rebuildBeacons(m: ShipModel) {
    const n = m.vessels.length
    const idx = new Float32Array(n * 3)
    const kind = new Float32Array(n * 3)
    const at = new Float32Array(n * 9)
    m.vessels.forEach((v, i) => {
      for (let k = 0; k < 3; k++) { idx[i * 3 + k] = i; kind[i * 3 + k] = k }
      // A model call streaming shows at the bow, where its node will land.
      at.set([v.length / 2 - Math.min(2.2, v.length * 0.12) + 0.8, 0.5, 0], (i * 3 + 1) * 3)
    })
    const g = new THREE.BufferGeometry()
    g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(n * 9), 3))
    g.setAttribute('aIdx', new THREE.BufferAttribute(idx, 1))
    g.setAttribute('aKind', new THREE.BufferAttribute(kind, 1))
    g.setAttribute('aAt', new THREE.BufferAttribute(at, 3))
    this.beacons.geometry.dispose()
    this.beacons.geometry = g
  }

  // ---------------------------------------------------------------- selection

  setSelected(i: number) {
    if (i === this.selected) return
    this.selected = i
    this.writeVesselFlags()
    this.requestRender()
  }

  setHovered(i: number) {
    if (i === this.hovered) return
    this.hovered = i
    this.writeVesselFlags()
    this.requestRender()
  }

  setHighlight(i: number) {
    if (i === this.highlight || !this.model) return
    const attr = this.lights.geometry.getAttribute('aFlags') as THREE.BufferAttribute | undefined
    if (attr) {
      if (this.highlight >= 0 && this.highlight < attr.count) attr.setX(this.highlight, attr.getX(this.highlight) - 16)
      if (i >= 0 && i < attr.count) attr.setX(i, attr.getX(i) + 16)
      attr.needsUpdate = true
    }
    this.highlight = i
    this.requestRender()
  }

  /** What is under a pixel: a light when its vessel is drawn big enough to tell them apart, else a vessel. */
  pick(px: number, py: number): Hit | null {
    const m = this.model
    if (!m) return null
    const sea = this.seaPoint(px, py)
    // Lights, on vessels big enough on screen.
    let best = -1
    let bestD = 11
    const big = new Set<number>()
    m.vessels.forEach((v, i) => { if (this.pixelsPerUnit(this.cur[i * 3], this.cur[i * 3 + 2]) * v.length > 260) big.add(i) })
    if (big.size) {
      const vp = new THREE.Matrix4().multiplyMatrices(this.camera.projectionMatrix, this.camera.matrixWorldInverse)
      const e = vp.elements
      const w = this.container.clientWidth
      const h = this.container.clientHeight
      for (let i = 0; i < m.lights.length; i++) {
        const l = m.lights[i]
        if (!big.has(l.vessel)) continue
        const p = this.lightPos(l)
        const cw = e[3] * p.x + e[7] * p.y + e[11] * p.z + e[15]
        if (cw <= 0) continue
        const sx = ((e[0] * p.x + e[4] * p.y + e[8] * p.z + e[12]) / cw + 1) * 0.5 * w
        const sy = (1 - (e[1] * p.x + e[5] * p.y + e[9] * p.z + e[13]) / cw) * 0.5 * h
        const d = Math.hypot(sx - px, sy - py)
        if (d < bestD) { bestD = d; best = i }
      }
    }
    if (best >= 0) return { kind: 'light', light: best }
    if (!sea) return null
    let hit = -1
    let area = Infinity
    m.vessels.forEach((v, i) => {
      const dx = sea.x - this.cur[i * 3]
      const dz = sea.z - this.cur[i * 3 + 2]
      const hd = this.cur[i * 3 + 1]
      const c = Math.cos(-hd)
      const s = Math.sin(-hd)
      const lx = dx * c - dz * s
      const lz = dx * s + dz * c
      const ppu = this.pixelsPerUnit(this.cur[i * 3], this.cur[i * 3 + 2])
      const slack = 10 / Math.max(ppu, 1e-3)
      if (Math.abs(lx) <= v.length / 2 + slack && Math.abs(lz) <= v.beam / 2 + oarReach(v.beam) + slack) {
        const a = v.length * v.beam
        if (a < area) { area = a; hit = i }
      }
    })
    return hit >= 0 ? { kind: 'vessel', vessel: hit } : null
  }

  /** A light's world position now (its vessel's current slot). */
  lightWorldNow(i: number) {
    const l = this.model?.lights[i]
    return l ? this.lightPos(l) : null
  }

  vesselNow(i: number) {
    return { x: this.cur[i * 3], z: this.cur[i * 3 + 2], heading: this.cur[i * 3 + 1] }
  }

  // ---------------------------------------------------------------- the pointer

  private bindPointer() {
    const el = this.renderer.domElement
    let down: { x: number; y: number; sea: THREE.Vector3 | null; moved: boolean; id: number } | null = null
    let hoverRaf = 0
    let last = { x: 0, y: 0 }
    const local = (e: PointerEvent | WheelEvent | MouseEvent) => {
      const r = el.getBoundingClientRect()
      return { x: e.clientX - r.left, y: e.clientY - r.top }
    }
    el.addEventListener('wheel', (e) => {
      e.preventDefault()
      const p = local(e)
      const before = this.seaPoint(p.x, p.y)
      const dy = e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY
      this.tween = null
      this.follow = null
      this.dist = Math.min(this.maxDist, Math.max(this.minDist, this.dist * Math.exp(dy * 0.0016)))
      this.applyCamera()
      const after = this.seaPoint(p.x, p.y)
      if (before && after) { this.tx += before.x - after.x; this.tz += before.z - after.z; this.applyCamera() }
      this.requestRender()
    }, { passive: false })
    el.addEventListener('pointerdown', (e) => {
      if (e.button !== 0) return
      const p = local(e)
      down = { x: p.x, y: p.y, sea: this.seaPoint(p.x, p.y), moved: false, id: e.pointerId }
      el.setPointerCapture(e.pointerId)
    })
    el.addEventListener('pointermove', (e) => {
      const p = local(e)
      last = p
      if (down) {
        if (!down.moved && Math.hypot(p.x - down.x, p.y - down.y) < 4) return
        down.moved = true
        el.style.cursor = 'grabbing'
        this.tween = null
        this.follow = null
        const now = this.seaPoint(p.x, p.y)
        if (down.sea && now) { this.tx += down.sea.x - now.x; this.tz += down.sea.z - now.z; this.applyCamera(); this.requestRender() }
        return
      }
      if (!hoverRaf) {
        hoverRaf = requestAnimationFrame(() => {
          hoverRaf = 0
          const hit = this.pick(last.x, last.y)
          el.style.cursor = hit ? 'pointer' : 'grab'
          this.hooks.onHover?.(hit, last.x, last.y)
        })
      }
    })
    const up = (e: PointerEvent) => {
      if (!down) return
      const was = down
      down = null
      el.style.cursor = 'grab'
      try { el.releasePointerCapture(e.pointerId) } catch { /* released already */ }
      if (!was.moved) { const p = local(e); this.hooks.onClick?.(this.pick(p.x, p.y)) }
    }
    el.addEventListener('pointerup', up)
    el.addEventListener('pointercancel', up)
    el.addEventListener('pointerleave', () => { if (!down) this.hooks.onHover?.(null, -1, -1) })
    el.addEventListener('dblclick', (e) => { const p = local(e); this.hooks.onDouble?.(this.pick(p.x, p.y)) })
    el.style.cursor = 'grab'
  }

  // ---------------------------------------------------------------- the loop

  requestRender() {
    if (this.disposed || this.raf) return
    this.raf = requestAnimationFrame(this.frame)
  }

  private lastFrame = performance.now()

  /** Whether anything shown moves on its own: a wake, a lantern, a gear, a stream, a flowing current, a flare. */
  private alive(t: number): boolean {
    if (this.bench) return true
    if (t < this.flareUntil) return true
    if (this.calm) return false
    const m = this.model
    if (!m) return false
    if (m.vessels.some((v) => v.rig === 'sail' || v.rig === 'lantern' || v.streaming)) return true
    if (m.tethers.some((x) => x.live)) return true
    return m.lights.some((l) => l.running)
  }

  private frame = (when: number) => {
    this.raf = 0
    if (this.disposed) return
    const interval = when - this.lastFrame
    const dt = Math.min(0.1, interval / 1000)
    this.lastFrame = when
    // Only back-to-back frames say how long a frame takes.
    if (interval < 250) this.adapt(interval)
    const t = this.now()
    const camMoved = this.stepCamera(performance.now())
    const vesselsMoved = this.stepVessels(dt)
    this.u.uTime.value = t
    const c0 = performance.now()
    this.post.render(this.renderer, this.seaScene, this.scene, this.camera, this.calm)
    const c1 = performance.now()
    this.stats.frames++
    this.stats.scale = this.scale
    if (!this.stats.firstFrameAt) this.stats.firstFrameAt = performance.now()
    if (!this.stats.firstFleetAt && this.model?.vessels.length) this.stats.firstFleetAt = performance.now()
    this.stats.cpu.push(c1 - c0)
    if (this.stats.cpu.length > 240) this.stats.cpu.shift()
    this.hooks.onFrame?.(camMoved || vesselsMoved)
    if (camMoved || this.settling || this.tween || this.alive(t)) this.requestRender()
  }

  /** Dev and bench only: hide layers by name, to see what draws what. */
  debugHide(names: string[]) {
    const all = { sea: this.sea, stars: this.stars, hulls: this.hulls, lines: this.lines, flows: this.flows, wakes: this.wakes, lights: this.lights, sails: this.sails, beacons: this.beacons }
    for (const [k, o] of Object.entries(all)) o.visible = !names.includes(k)
    this.post.seaDirty = true
    this.requestRender()
  }

  dispose() {
    this.disposed = true
    if (this.raf) cancelAnimationFrame(this.raf)
    this.ro.disconnect()
    this.scene.traverse((o) => {
      const mesh = o as THREE.Mesh
      mesh.geometry?.dispose()
      const mm = mesh.material as THREE.Material | undefined
      mm?.dispose()
    })
    this.seaScene.traverse((o) => {
      const mesh = o as THREE.Mesh
      mesh.geometry?.dispose()
      ;(mesh.material as THREE.Material | undefined)?.dispose()
    })
    this.vtex?.dispose()
    this.post.dispose()
    this.renderer.dispose()
    this.renderer.domElement.remove()
  }
}

function instanced(base: THREE.BufferGeometry): THREE.InstancedBufferGeometry {
  const g = new THREE.InstancedBufferGeometry()
  g.index = base.index
  g.setAttribute('position', base.getAttribute('position'))
  g.setAttribute('uv', base.getAttribute('uv'))
  g.instanceCount = 0
  return g
}

function starGeometry(): THREE.BufferGeometry {
  // The logo's night: a few gold stars, fixed (a seeded scatter), far under the chart.
  let s = 0x5eed
  const rnd = () => { s = (s * 1664525 + 1013904223) >>> 0; return s / 4294967296 }
  const N = 1600
  const pos = new Float32Array(N * 3)
  const size = new Float32Array(N)
  const tone = new Float32Array(N)
  for (let i = 0; i < N; i++) {
    pos[i * 3] = (rnd() - 0.5) * 3600
    pos[i * 3 + 1] = -140 - rnd() * 60
    pos[i * 3 + 2] = (rnd() - 0.5) * 3600
    size[i] = 0.6 + Math.pow(rnd(), 3) * 2.6
    tone[i] = 0.25 + rnd() * 0.6
  }
  const g = new THREE.BufferGeometry()
  g.setAttribute('position', new THREE.BufferAttribute(pos, 3))
  g.setAttribute('aSize', new THREE.BufferAttribute(size, 1))
  g.setAttribute('aTone', new THREE.BufferAttribute(tone, 1))
  return g
}

function hashSeed(id: string): number {
  let h = 2166136261
  for (let i = 0; i < id.length; i++) { h ^= id.charCodeAt(i); h = Math.imul(h, 16777619) }
  return ((h >>> 0) % 1000) + 0.5
}

