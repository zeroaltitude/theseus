// The Ship's model (theseus-logs): the whole graph as a fleet, built from what the protocol says and nothing else.
// Pure: no three.js and no React, so the layout is the same whoever draws it, and a test or a bench can build it.
//
// - Each session is a vessel, sized by its node count and rigged by its state. Tasks are boats that trail their
//   parent, tethered to it.
// - Sessions from one place (a Discord DM, a channel, the web UI, the CLI) sail together, as a formation.
// - Every node is a light along its vessel's keel, in order: messages and model calls on the keel, each tool call an
//   oar out to its side, with its result at the blade.
// - Each turn is a bench (a thwart across the deck), stern to bow, oldest to newest (theseus-hnof): a ship grows a
//   bench for every turn, and a turn's tool calls are the oars of its bench.
//
// Positions are deterministic and stable: a vessel keeps its slot when others arrive, and (given `placesSeen`) a
// harbour keeps its place as its ships grow, so the map never reshuffles under the operator's eye. `asOf` is the seam for the time machine (a later round): it shows the fleet as it was.
import type { Attention, ConfirmRequest, ExecutionInfo, ExternalText, NodeInfo, SessionInfo, SessionLink, SessionRetired, SessionState, TaskInfo } from '@protocol'

export type LightKind = 'user' | 'model' | 'call' | 'result'
/** How a vessel is rigged: at anchor (nothing running), under sail (working), a lantern (waiting for the
 *  operator), or a flare (failed). */
export type Rig = 'anchor' | 'sail' | 'lantern' | 'flare'

export interface Light {
  id: string
  sessionId: string
  /** The vessel's index in `ShipModel.vessels`. */
  vessel: number
  kind: LightKind
  at: number
  turnId?: string
  tool?: string
  toolUseId?: string
  correlationId?: string
  /** A result that failed (or a call whose result failed). */
  failed?: boolean
  /** A result that carried external text (web): it glows the warning colour. */
  external?: boolean
  /** A call or result that ran in the sandbox (17b's L1): drawn inside a hexagonal shield. */
  l1?: boolean
  /** A job that runs now: a turning gear. */
  running?: boolean
  /** A call that waits for the operator's answer (confirm.list names its correlation id): its blade is amber. */
  waiting?: boolean
  /** Its turn's bench, an index into `ShipModel.benches`; -1 for a node with no turn. */
  bench: number
  /** A sandboxed job a cancel or a stop verified gone (18a): its shield collapses. The time the job settled; the
   *  engine animates a collapse that comes while the page is open and draws an older one collapsed. */
  collapsedAt?: number
  model?: string
  cost?: number
  author?: string
  preview: string
  /** Local position on the vessel: x along the keel (stern -, bow +), z across (port +), y up. */
  lx: number
  lz: number
  ly: number
  /** For a result: its call's local position, so the oar is drawn from it. */
  ox?: number
  oz?: number
  /** Client time it arrived live (ms), for its flare; 0 for what was there when the page loaded. */
  born: number
}

export interface Vessel {
  id: string
  kind: 'conversation' | 'task'
  title: string
  label: string | null
  place: string
  parentId?: string
  taskShort?: string
  depth: number
  state: string
  attention?: Attention
  rig: Rig
  nodes: number
  turns: number
  toolCalls: number
  cost: number
  limit?: number
  spent?: number
  reserved: number
  hold?: ExternalText
  pendingConfirms: number
  created: number
  lastActive: number
  profile?: string
  model?: string
  executionId?: string
  /** A model call is streaming now (model.delta seen in the last seconds). */
  streaming: boolean
  /** Client time of its last failure flare (turn.failed), for the burst. */
  flareAt: number
  /** Client time it opened while the page watched (ms), for its flare; 0 for what was there before. */
  born: number
  /** Its benches (turns), stern to bow: indices into `ShipModel.benches`. */
  benches: number[]
  /** The bench of the turn running now, or -1. */
  activeBench: number
  /** Planks: one per turn (up to 36 drawn), gold for the turns of the last hour. */
  planks: number
  goldPlanks: number
  /** Live, quiet or retired (theseus-emqx): at sea, at anchor in the roads, or laid up in harbour. */
  life: SessionState
  retired?: SessionRetired
  /** The session its place moved to, and the one it replaced: a superseded ship flies a signal toward its successor. */
  supersededBy?: SessionLink
  supersedes?: SessionLink
  /** Its titles before the re-title. */
  titleWas?: string[]
  // layout, world units
  x: number
  z: number
  heading: number
  length: number
  beam: number
}

/** A turn, drawn as a bench across its vessel's deck: the message that started it, its model calls on the keel, and its
 *  tool calls as the bench's oars. */
export interface Bench {
  vessel: number
  turnId: string
  /** 1-based, in the session's order. */
  n: number
  /** Local x of its middle, and its half-width along the keel. */
  x: number
  half: number
  /** Its first node's time and its last's. */
  at: number
  end: number
  /** Model calls, tool calls, failed results. */
  models: number
  calls: number
  failed: number
  /** The model calls' cost in dollars. */
  cost: number
  model?: string
  /** The message that started it, and who wrote it. */
  preview: string
  author?: string
  /** The turn runs now (its session works on it), or one of its jobs does. */
  running: boolean
  /** One of its calls waits for the operator. */
  waiting: boolean
  /** Its lights, indices into `ShipModel.lights`. */
  lights: number[]
}

export interface Tether {
  from: number
  to: number
  /** The task's state: running, waiting, complete, failed, cancelled… */
  state: string
  /** Client time its report landed, for the burst along the line. */
  reportAt: number
  live: boolean
}

export interface Current {
  from: number
  to: number
  fromLight?: number
  toLight?: number
  via: string
}

export interface Formation {
  key: string
  label: string
  x: number
  z: number
  radius: number
  members: number[]
}

export interface ShipModel {
  vessels: Vessel[]
  lights: Light[]
  benches: Bench[]
  tethers: Tether[]
  currents: Current[]
  formations: Formation[]
  /** Half extents of the whole fleet, for "fit". */
  bounds: { minX: number; maxX: number; minZ: number; maxZ: number }
  byId: Map<string, number>
  lightById: Map<string, number>
  stats: { sessions: number; nodes: number; tasks: number; running: number; waiting: number; held: number; l1: number; external: number }
}

/** A cross-session link from `node.reach`: a node, and its copy in another session. */
export interface ReachLink { fromSession: string; fromNode: string; toSession: string; toNode: string; via: string }

export interface ShipInput {
  sessions: SessionInfo[]
  executions: ExecutionInfo[]
  tasks: TaskInfo[]
  nodes: NodeInfo[]
  confirms: ConfirmRequest[]
  /** Correlation ids of jobs that ran under L1 (`tool.job_started` rows and `tool.started` with `class: "l1"`): the
   *  fallback for a call node with no class in its gate decision. */
  l1: Set<string>
  /** Correlation ids of jobs running now. */
  jobsRunning: Set<string>
  /** Session id → client time of its last model.delta. */
  streaming: Map<string, number>
  /** Session id → client time of its last turn.failed. */
  failedAt: Map<string, number>
  /** Task session id → client time its report landed. */
  reports: Map<string, number>
  /** Node id → client time it arrived live. */
  born: Map<string, number>
  /** Session id → client time it appeared live. */
  bornSessions: Map<string, number>
  /** Reserved money of each execution's actions in flight. */
  reserved: Map<string, number>
  /** Correlation ids of jobs a cancel verified gone (18a's verdicts) → the time each settled. */
  cancelled?: Map<string, number>
  reach: ReachLink[]
  /** Session id → the turn it runs now (turn.started, until turn.ended or turn.failed). A session that works with none
   *  known runs its newest turn. */
  active?: Map<string, string>
  now: number
  /** The time machine's seam: show only what existed at this instant (unix ms). Undefined is live. */
  asOf?: number
  /** Where each place's formation was last placed, kept by the caller across builds: a formation stays where it was
   *  while it is still clear of those placed before it, so a ship that grows a bench doesn't send its harbour, and the
   *  camera following it, across the map (theseus-hnof). The layout writes it back. */
  placesSeen?: Map<string, { x: number; z: number }>
}

// ---------------------------------------------------------------- places

/** Where a session sails from: its label's place, as the operator says it. */
export function placeOf(s: Pick<SessionInfo, 'label' | 'kind'>): { key: string; label: string } {
  const raw = (s.label ?? '').trim()
  if (!raw) return { key: s.kind === 'task' ? 'tasks' : 'cli', label: s.kind === 'task' ? 'Tasks' : 'CLI' }
  const lower = raw.toLowerCase()
  if (lower.startsWith('discord ')) {
    const rest = raw.slice(8).trim()
    if (rest.startsWith('#')) return { key: `discord:${rest}`, label: rest }
    return { key: `discord:${rest}`, label: `Discord ${rest}` }
  }
  if (lower === 'web' || lower.startsWith('web ')) return { key: 'web', label: 'Web' }
  if (lower === 'cli' || lower.startsWith('cli ')) return { key: 'cli', label: 'CLI' }
  if (lower === 'tui' || lower.startsWith('tui ')) return { key: 'tui', label: 'Terminal' }
  return { key: `place:${lower}`, label: raw }
}

export function rigOf(state: string | undefined, attention: Attention | undefined, pending: number): Rig {
  if (state === 'failed' || state === 'budget_exhausted' || state === 'interrupted') return 'flare'
  if (attention?.level === 'needs_you' || pending > 0) return 'lantern'
  if (attention?.level === 'working' || state === 'running' || state === 'queued') return 'sail'
  return 'anchor'
}

const kindOf = (k: string): LightKind | null =>
  k === 'user_message' ? 'user' : k === 'assistant_message' ? 'model' : k === 'tool_call' ? 'call' : k === 'tool_result' ? 'result' : null

type D = Record<string, unknown>
const str = (v: unknown): string | undefined => (typeof v === 'string' ? v : undefined)

function previewOf(n: NodeInfo, kind: LightKind): string {
  const d = (n.detail ?? {}) as D
  if (kind === 'call') {
    const plan = d.plan as D | undefined
    return str(plan?.summary) ?? `${str(d.tool) ?? 'tool'} ${JSON.stringify(d.input ?? {}).slice(0, 120)}`
  }
  const t = (n.text || n.thinking || '').replace(/\s+/g, ' ').trim()
  return t.length > 160 ? `${t.slice(0, 157)}…` : t
}

// ---------------------------------------------------------------- sizes

/** Hull length from the node count: a session with no nodes is a dinghy; a long one a quinquereme. */
export const hullLength = (nodes: number, task: boolean) => (task ? 0.72 : 1) * (7 + 2.3 * Math.sqrt(nodes))
const hullBeam = (len: number) => Math.max(1.9, len * 0.165)
/** How far a result's oar reaches past the hull's side. */
export const oarReach = (beam: number) => Math.max(1.1, beam * 0.55)

// ---------------------------------------------------------------- build

export function buildModel(input: ShipInput): ShipModel {
  const { now, asOf } = input
  const cut = asOf ?? Infinity
  const sessions = input.sessions.filter((s) => s.created_at_unix_ms <= cut)
  const execById = new Map(input.executions.map((e) => [e.execution_id, e]))
  const sessionOfExec = new Map(input.executions.map((e) => [e.execution_id, e.session_id]))
  const taskBySession = new Map<string, TaskInfo>()
  for (const t of input.tasks) {
    const sid = sessionOfExec.get(t.execution_id)
    if (sid) taskBySession.set(sid, t)
  }
  const pendingBySession = new Map<string, number>()
  for (const c of input.confirms) pendingBySession.set(c.session_id, (pendingBySession.get(c.session_id) ?? 0) + 1)

  // Nodes by session, in order.
  const nodesBy = new Map<string, NodeInfo[]>()
  for (const n of input.nodes) {
    if (n.at_unix_ms > cut) continue
    let a = nodesBy.get(n.session_id)
    if (!a) { a = []; nodesBy.set(n.session_id, a) }
    a.push(n)
  }
  for (const a of nodesBy.values()) a.sort((x, y) => x.position - y.position)

  // Vessels.
  const vessels: Vessel[] = []
  const byId = new Map<string, number>()
  const ids = new Set(sessions.map((s) => s.session_id))
  const sorted = [...sessions].sort((a, b) => a.created_at_unix_ms - b.created_at_unix_ms || a.session_id.localeCompare(b.session_id))
  for (const s of sorted) {
    const e = s.execution_id ? execById.get(s.execution_id) : undefined
    const task = taskBySession.get(s.session_id)
    const parentId = s.parent_session_id ?? task?.parent_session_id
    const nodes = nodesBy.get(s.session_id) ?? []
    const isTask = s.kind === 'task'
    const len = hullLength(nodes.length, isTask)
    const pending = Math.max(s.pending_confirms ?? 0, pendingBySession.get(s.session_id) ?? 0)
    const state = e?.state ?? s.execution_state ?? 'idle'
    const attention = s.attention ?? e?.attention
    const recentTurns = new Set(nodes.filter((n) => n.turn_id && n.at_unix_ms > now - 3_600_000).map((n) => n.turn_id))
    const place = placeOf(s)
    byId.set(s.session_id, vessels.length)
    vessels.push({
      id: s.session_id,
      kind: s.kind,
      title: s.title || task?.title || s.label || 'untitled',
      label: s.label,
      place: isTask && parentId && ids.has(parentId) ? '' : place.key,
      parentId: parentId && ids.has(parentId) ? parentId : undefined,
      taskShort: task?.short,
      depth: 0,
      state,
      attention,
      rig: rigOf(state, attention, pending),
      nodes: nodes.length,
      turns: s.turns,
      toolCalls: s.tool_calls,
      cost: s.cost_usd,
      limit: e?.budget.limit_usd ?? s.limit_usd,
      spent: e?.budget.spent_usd,
      reserved: (e ? input.reserved.get(e.execution_id) : undefined) ?? e?.budget.reserved_usd ?? 0,
      hold: s.external_text,
      pendingConfirms: pending,
      created: s.created_at_unix_ms,
      lastActive: s.last_active_ms,
      profile: s.profile,
      model: s.model,
      executionId: s.execution_id,
      streaming: (input.streaming.get(s.session_id) ?? 0) > now - 4000,
      flareAt: input.failedAt.get(s.session_id) ?? 0,
      born: input.bornSessions.get(s.session_id) ?? 0,
      benches: [],
      activeBench: -1,
      planks: Math.max(1, s.turns),
      goldPlanks: recentTurns.size,
      life: s.state ?? 'live',
      ...(s.retired ? { retired: s.retired } : {}),
      ...(s.superseded_by ? { supersededBy: s.superseded_by } : {}),
      ...(s.supersedes ? { supersedes: s.supersedes } : {}),
      ...(s.title_was?.length ? { titleWas: s.title_was } : {}),
      x: 0, z: 0, heading: 0, length: len, beam: hullBeam(len),
    })
  }
  // Depth: a task of a task sails behind its parent's boat.
  for (const v of vessels) {
    let d = 0
    let p = v.parentId
    const seen = new Set<string>()
    while (p && !seen.has(p) && d < 8) { seen.add(p); d++; p = vessels[byId.get(p)!]?.parentId }
    v.depth = d
  }

  // Lights, by turn: each turn a bench, stern to bow. A bench is as wide as its stations (every node but a result
  // that rides its call's oar), with a gap between benches, so a turn reads as one group.
  const lights: Light[] = []
  const benches: Bench[] = []
  const lightById = new Map<string, number>()
  const waitingCalls = new Set(input.confirms.map((c) => c.correlation_id))
  let l1Count = 0
  let extCount = 0
  for (const v of vessels) {
    const nodes = nodesBy.get(v.id) ?? []
    const vi = byId.get(v.id)!
    const callByUse = new Map<string, NodeInfo>()
    for (const n of nodes) {
      if (n.kind !== 'tool_call') continue
      const u = str((n.detail as D | null)?.tool_use_id)
      if (u) callByUse.set(u, n)
    }
    const rides = (n: NodeInfo) => {
      if (n.kind !== 'tool_result') return false
      const u = str((n.detail as D | null)?.tool_use_id)
      return !!u && callByUse.has(u)
    }
    // Turns in order of their first node; a node with no turn rides the bench before it.
    const turnKey: string[] = []
    let prev = ''
    for (const n of nodes) { prev = n.turn_id ?? prev; turnKey.push(prev) }
    const half = v.length / 2
    const margin = Math.min(2.2, v.length * 0.12)
    const span = v.length - 2 * margin
    const GAP = 1.1
    // Units along the keel: a station each, and a gap where the turn changes.
    const unitOf = new Map<string, number>()
    let u = 0
    let lastTurn: string | null = null
    nodes.forEach((n, i) => {
      if (rides(n)) return
      if (lastTurn !== null && turnKey[i] !== lastTurn) u += GAP
      lastTurn = turnKey[i]
      unitOf.set(n.node_id, u)
      u += 1
    })
    const total = Math.max(1, u - 1)
    const xAt = (unit: number) => (u <= 1 ? 0 : -half + margin + (span * unit) / total)
    const resultFailed = new Map<string, boolean>()
    for (const n of nodes) {
      if (n.kind !== 'tool_result') continue
      const d = (n.detail ?? {}) as D
      const ru = str(d.tool_use_id)
      if (ru) resultFailed.set(ru, d.is_error === true || (str(d.status) !== undefined && str(d.status) !== 'ok'))
    }
    // The benches.
    const benchOf = new Map<string, number>()
    const firstUnit = new Map<string, number>()
    const lastUnit = new Map<string, number>()
    nodes.forEach((n, i) => {
      const k = turnKey[i]
      const un = unitOf.get(n.node_id)
      if (un === undefined) return
      if (!firstUnit.has(k)) firstUnit.set(k, un)
      lastUnit.set(k, un)
    })
    const vb: number[] = []
    for (const [k, f] of firstUnit) {
      const l = lastUnit.get(k) ?? f
      const x0 = xAt(f - 0.5)
      const x1 = xAt(l + 0.5)
      benchOf.set(k, benches.length)
      vb.push(benches.length)
      benches.push({
        vessel: vi, turnId: k, n: vb.length, x: (x0 + x1) / 2, half: Math.max(0.3, (x1 - x0) / 2),
        at: Infinity, end: 0, models: 0, calls: 0, failed: 0, cost: 0, preview: '', running: false, waiting: false, lights: [],
      })
    }
    v.benches = vb
    let oar = 0
    const callSide = new Map<string, number>()
    const callPos = new Map<string, { x: number; z: number }>()
    nodes.forEach((n, i) => {
      const kind = kindOf(n.kind)
      if (!kind) return
      const d = (n.detail ?? {}) as D
      const cid = str(d.correlation_id)
      const use = str(d.tool_use_id)
      // A call's node records its class in the gate's decision (NODE schema 4); the job rows and pushes are the fallback
      // for a node from before, so a call older than the newest 2,000 job rows keeps its shield.
      const l1 = (kind === 'call' && str((d.decision as D | undefined)?.class) === 'l1') || (!!cid && input.l1.has(cid))
      let lx = xAt(unitOf.get(n.node_id) ?? 0)
      let lz = 0
      let ox: number | undefined
      let oz: number | undefined
      if (kind === 'call') {
        const side = oar++ % 2 === 0 ? 1 : -1
        lz = side * v.beam * 0.3
        if (use) { callSide.set(use, side); callPos.set(use, { x: lx, z: lz }) }
      } else if (kind === 'result' && use && callPos.has(use)) {
        const c = callPos.get(use)!
        const side = callSide.get(use) ?? 1
        ox = c.x
        oz = c.z
        lx = c.x - oarReach(v.beam) * 0.42
        lz = side * (v.beam * 0.5 + oarReach(v.beam))
      }
      const external = kind === 'result' && d.external != null && d.external !== false
      if (l1) l1Count++
      if (external) extCount++
      const bi = benchOf.get(turnKey[i]) ?? -1
      const failed = kind === 'result' ? resultFailed.get(use ?? '') ?? false : kind === 'call' && use ? resultFailed.get(use) : undefined
      const running = !!cid && input.jobsRunning.has(cid)
      const waiting = kind === 'call' && !!cid && waitingCalls.has(cid)
      const li = lights.length
      lightById.set(n.node_id, li)
      lights.push({
        id: n.node_id,
        sessionId: v.id,
        vessel: vi,
        kind,
        at: n.at_unix_ms,
        turnId: n.turn_id,
        tool: str(d.tool),
        toolUseId: use,
        correlationId: cid,
        failed,
        external,
        l1,
        running,
        ...(waiting ? { waiting } : {}),
        ...(l1 && cid && input.cancelled?.has(cid) ? { collapsedAt: input.cancelled.get(cid) } : {}),
        model: str(d.model),
        cost: typeof d.cost_usd === 'number' ? d.cost_usd : undefined,
        author: n.author,
        preview: previewOf(n, kind),
        lx, lz, ly: kind === 'model' ? 0.35 : kind === 'user' ? 0.3 : 0.2,
        ox, oz,
        born: input.born.get(n.node_id) ?? 0,
        bench: bi,
      })
      if (bi >= 0) {
        const b = benches[bi]
        b.lights.push(li)
        b.at = Math.min(b.at, n.at_unix_ms)
        b.end = Math.max(b.end, n.at_unix_ms)
        if (kind === 'model') { b.models++; b.cost += typeof d.cost_usd === 'number' ? d.cost_usd : 0; b.model ??= str(d.model) }
        if (kind === 'call') b.calls++
        if (kind === 'result' && failed) b.failed++
        if (kind === 'user' && !b.preview) { b.preview = previewOf(n, kind); b.author = n.author }
        if (running) b.running = true
        if (waiting) b.waiting = true
      }
    })
    // The turn running now: the one the pushes named; else, for a vessel that works, the newest turn with a job running
    // (its oar's gear turns), else its newest turn.
    const act = input.active?.get(v.id)
    const jobBench = [...vb].reverse().find((b) => benches[b].running)
    const ab = act !== undefined ? benchOf.get(act) : v.rig === 'sail' && vb.length ? jobBench ?? vb[vb.length - 1] : undefined
    v.activeBench = ab ?? -1
    if (ab !== undefined) benches[ab].running = true
  }

  // Tethers: each task to its parent.
  const tethers: Tether[] = []
  for (const v of vessels) {
    if (!v.parentId) continue
    const p = byId.get(v.parentId)
    if (p === undefined) continue
    const state = taskBySession.get(v.id)?.state ?? v.state
    tethers.push({
      from: p, to: byId.get(v.id)!, state,
      reportAt: input.reports.get(v.id) ?? 0,
      live: v.rig === 'sail' || state === 'running' || state === 'queued',
    })
  }

  // Currents: reach links between vessels (a node and its copy elsewhere).
  const currents: Current[] = []
  const seenCurrent = new Set<string>()
  for (const r of input.reach) {
    const a = byId.get(r.fromSession)
    const b = byId.get(r.toSession)
    if (a === undefined || b === undefined || a === b) continue
    const key = `${r.fromNode}>${r.toNode}`
    if (seenCurrent.has(key)) continue
    seenCurrent.add(key)
    currents.push({ from: a, to: b, fromLight: lightById.get(r.fromNode), toLight: lightById.get(r.toNode), via: r.via })
  }

  const formations = layout(vessels, byId, input.placesSeen)
  const bounds = { minX: Infinity, maxX: -Infinity, minZ: Infinity, maxZ: -Infinity }
  for (const v of vessels) {
    const r = v.length / 2 + 4
    bounds.minX = Math.min(bounds.minX, v.x - r); bounds.maxX = Math.max(bounds.maxX, v.x + r)
    bounds.minZ = Math.min(bounds.minZ, v.z - r); bounds.maxZ = Math.max(bounds.maxZ, v.z + r)
  }
  // The places' rings, and room above each for its name.
  for (const f of formations) {
    bounds.minX = Math.min(bounds.minX, f.x - f.radius); bounds.maxX = Math.max(bounds.maxX, f.x + f.radius)
    bounds.minZ = Math.min(bounds.minZ, f.z - f.radius * 1.12 - 3); bounds.maxZ = Math.max(bounds.maxZ, f.z + f.radius)
  }
  if (!vessels.length) Object.assign(bounds, { minX: -40, maxX: 40, minZ: -30, maxZ: 30 })

  return {
    vessels, lights, benches, tethers, currents, formations, bounds, byId, lightById,
    stats: {
      sessions: vessels.length,
      nodes: lights.length,
      tasks: vessels.filter((v) => v.kind === 'task').length,
      running: vessels.filter((v) => v.rig === 'sail').length,
      waiting: vessels.filter((v) => v.rig === 'lantern').length,
      held: vessels.filter((v) => v.hold).length,
      l1: l1Count,
      external: extCount,
    },
  }
}

// ---------------------------------------------------------------- layout

/** A deterministic hash in [0, 1), for small stable jitter. */
export function hash01(s: string): number {
  let h = 2166136261
  for (let i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 16777619) }
  return ((h >>> 0) % 100000) / 100000
}

interface Footprint { len: number; beam: number }

/** Lay out the fleet: tasks behind their parents, members of a place in ranks, places around the centre. */
function layout(vessels: Vessel[], byId: Map<string, number>, seen?: Map<string, { x: number; z: number }>): Formation[] {
  const children = new Map<number, number[]>()
  for (let i = 0; i < vessels.length; i++) {
    const p = vessels[i].parentId
    if (!p) continue
    const pi = byId.get(p)
    if (pi === undefined) continue
    const a = children.get(pi) ?? []
    a.push(i)
    children.set(pi, a)
  }
  const GAP = 3.5
  // A vessel's footprint, with the boats that trail it (relative to its own centre: they go astern).
  const foot = new Map<number, Footprint>()
  const measure = (i: number, guard: number): Footprint => {
    const v = vessels[i]
    const own = { len: v.length, beam: v.beam + 2 * oarReach(v.beam) }
    const kids = guard < 8 ? (children.get(i) ?? []) : []
    if (!kids.length) { foot.set(i, own); return own }
    const fs = kids.map((k) => measure(k, guard + 1))
    const perRow = Math.min(4, Math.max(1, Math.ceil(Math.sqrt(kids.length))))
    const rows = Math.ceil(kids.length / perRow)
    const rowLen = Math.max(...fs.map((f) => f.len)) + GAP
    const rowBeam = Math.max(...fs.map((f) => f.beam)) + GAP * 0.6
    const f = { len: own.len + GAP * 1.6 + rows * rowLen, beam: Math.max(own.beam, perRow * rowBeam) }
    foot.set(i, f)
    return f
  }
  const roots: number[] = []
  for (let i = 0; i < vessels.length; i++) if (!vessels[i].parentId) roots.push(i)
  for (const r of roots) measure(r, 0)

  // Places: in order of their first vessel.
  const places = new Map<string, number[]>()
  for (const r of roots) {
    const k = vessels[r].place || 'tasks'
    const a = places.get(k) ?? []
    a.push(r)
    places.set(k, a)
  }
  const formations: Formation[] = []
  const placed: { x: number; z: number; r: number }[] = []
  for (const [key, members] of places) {
    // Ranks: line abreast, staggered rank by rank, the newest ahead.
    const fs = members.map((m) => foot.get(m)!)
    const perRank = Math.max(1, Math.round(Math.sqrt(members.length * 1.4)))
    const rankLen = Math.max(...fs.map((f) => f.len)) + GAP * 1.8
    const file = Math.max(...fs.map((f) => f.beam)) + GAP * 2.2
    const ranks = Math.ceil(members.length / perRank)
    const slots: { x: number; z: number }[] = members.map((_, m) => {
      const rank = Math.floor(m / perRank)
      const inRank = Math.min(perRank, members.length - rank * perRank)
      const f = m % perRank
      return { x: -rank * rankLen + (ranks - 1) * rankLen * 0.5, z: (f - (inRank - 1) / 2) * file + (rank % 2 ? file * 0.25 : 0) }
    })
    // The vessel's own centre sits ahead of its trailing boats.
    let radius = 0
    members.forEach((m, j) => {
      const fp = fs[j]
      const v = vessels[m]
      slots[j].x += (fp.len - v.length) / 2
      radius = Math.max(radius, Math.hypot(Math.abs(slots[j].x) + fp.len / 2, Math.abs(slots[j].z) + fp.beam / 2))
    })
    radius += GAP * 1.4
    // Place the formation: where it was last time, while that is still clear of every formation already placed;
    // otherwise the first spiral point clear of them.
    let fx = 0
    let fz = 0
    const was = seen?.get(key)
    const clear = (cx: number, cz: number) => placed.every((p) => Math.hypot(p.x - cx, p.z - cz) > p.r + radius + GAP * 1.6)
    if (was && clear(was.x, was.z)) { fx = was.x; fz = was.z }
    else if (placed.length) {
      const golden = Math.PI * (3 - Math.sqrt(5))
      for (let k = 1; k < 4000; k++) {
        const rr = 4 * Math.sqrt(k) * 2
        const th = k * golden
        const cx = Math.cos(th) * rr * 1.35
        const cz = Math.sin(th) * rr
        if (clear(cx, cz)) { fx = cx; fz = cz; break }
      }
    }
    placed.push({ x: fx, z: fz, r: radius })
    seen?.set(key, { x: fx, z: fz })
    const heading = (hash01(key) - 0.5) * 0.12
    members.forEach((m, j) => {
      const v = vessels[m]
      const jitter = (hash01(v.id) - 0.5) * 0.06
      const c = Math.cos(heading)
      const s = Math.sin(heading)
      v.x = fx + slots[j].x * c - slots[j].z * s
      v.z = fz + slots[j].x * s + slots[j].z * c
      v.heading = heading + jitter
      placeChildren(m, vessels, children, foot, GAP)
    })
    const first = vessels[members[0]]
    formations.push({ key, label: labelOf(key, first), x: fx, z: fz, radius, members })
  }
  return formations
}

function labelOf(key: string, v: Vessel): string {
  if (key === 'tasks') return 'Tasks'
  return placeOf({ label: v.label, kind: v.kind }).label
}

/** A vessel's tasks, astern of it in rows, each tethered to it; theirs astern of them. */
function placeChildren(i: number, vessels: Vessel[], children: Map<number, number[]>, foot: Map<number, Footprint>, GAP: number, guard = 0) {
  const kids = children.get(i)
  if (!kids?.length || guard > 8) return
  const v = vessels[i]
  const fs = kids.map((k) => foot.get(k)!)
  const perRow = Math.min(4, Math.max(1, Math.ceil(Math.sqrt(kids.length))))
  const rowLen = Math.max(...fs.map((f) => f.len)) + GAP
  const rowBeam = Math.max(...fs.map((f) => f.beam)) + GAP * 0.6
  const c = Math.cos(v.heading)
  const s = Math.sin(v.heading)
  kids.forEach((k, j) => {
    const row = Math.floor(j / perRow)
    const inRow = Math.min(perRow, kids.length - row * perRow)
    const f = j % perRow
    const kv = vessels[k]
    const back = v.length / 2 + GAP * 1.6 + row * rowLen + kv.length / 2
    const lat = (f - (inRow - 1) / 2) * rowBeam
    kv.x = v.x - back * c - lat * s
    kv.z = v.z - back * s + lat * c
    kv.heading = v.heading + (hash01(kv.id) - 0.5) * 0.05
    placeChildren(k, vessels, children, foot, GAP, guard + 1)
  })
}

/** World position of a light (its vessel's transform applied to its local position). */
export function lightWorld(m: ShipModel, l: Light): { x: number; y: number; z: number } {
  const v = m.vessels[l.vessel]
  const c = Math.cos(v.heading)
  const s = Math.sin(v.heading)
  return { x: v.x + l.lx * c - l.lz * s, y: l.ly, z: v.z + l.lx * s + l.lz * c }
}
