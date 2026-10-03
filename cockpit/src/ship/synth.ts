// A synthetic fleet for measuring the Ship (theseus-logs): 200 sessions and 10,000 nodes, seeded, so every run draws
// the same graph. Dev and bench builds only (`?synthetic=1`): a production build never imports it, and it is never
// shown as data. Every name in it is invented.
import type { ConfirmRequest, ExecutionInfo, NodeInfo, SessionInfo, TaskInfo } from '@protocol'
import type { ShipInput } from './model'

function mulberry32(seed: number) {
  return () => {
    seed |= 0
    seed = (seed + 0x6d2b79f5) | 0
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

const PLACES = ['discord DM @ada', 'discord #harbour', 'discord #engine-room', 'web', 'cli', 'tui', 'discord DM @grace', 'discord #lighthouse']
const TOOLS = ['fs.read', 'fs.grep', 'proc.run', 'git.diff', 'fs.edit', 'web.search', 'http.fetch', 'fs.list', 'git.log', 'task.create']
const TITLES = ['Chart the reef', 'Mend the sail', 'Count the oars', 'Read the tide table', 'Sound the harbour', 'Trim the ballast', 'Log the stars', 'Caulk the hull', 'Splice the line', 'Polish the brass']

export function synthInput(now: number, sessionsN = 200, nodesN = 10_000): ShipInput {
  const rnd = mulberry32(0x7e5e05)
  const pick = <T,>(a: T[]) => a[Math.floor(rnd() * a.length)]
  const sessions: SessionInfo[] = []
  const executions: ExecutionInfo[] = []
  const tasks: TaskInfo[] = []
  const nodes: NodeInfo[] = []
  const confirms: ConfirmRequest[] = []
  const l1 = new Set<string>()
  const running = new Set<string>()
  const streaming = new Map<string, number>()
  const reach: ShipInput['reach'] = []
  // Heavy-tailed sizes that sum to nodesN.
  const w = Array.from({ length: sessionsN }, () => Math.pow(rnd(), 2.2) + 0.02)
  const sum = w.reduce((a, b) => a + b, 0)
  const sizes = w.map((x) => Math.max(2, Math.round((x / sum) * nodesN)))
  let drift = sizes.reduce((a, b) => a + b, 0) - nodesN
  for (let i = 0; drift !== 0; i = (i + 1) % sessionsN) {
    if (drift > 0 && sizes[i] > 3) { sizes[i]--; drift-- } else if (drift < 0) { sizes[i]++; drift++ }
  }
  let pos = 1
  const conv: number[] = []
  for (let i = 0; i < sessionsN; i++) {
    const isTask = i > 20 && rnd() < 0.25 && conv.length > 0
    const sid = `ses_synth${String(i).padStart(4, '0')}`
    const eid = `exe_synth${String(i).padStart(4, '0')}`
    const created = now - (sessionsN - i) * 600_000 - Math.floor(rnd() * 300_000)
    const parentIdx = isTask ? (rnd() < 0.8 ? pick(conv) : i - 1) : undefined
    const parent = parentIdx !== undefined ? sessions[parentIdx] : undefined
    const r = rnd()
    const state = r < 0.06 ? 'running' : r < 0.08 ? 'failed' : 'waiting'
    const level: 'working' | 'needs_you' | 'ready' | 'idle' = state === 'running' ? 'working' : r < 0.105 ? 'needs_you' : r < 0.6 ? 'ready' : 'idle'
    const held = rnd() < 0.05
    // Nodes: turns of a user message, then loops of a model call and its tool calls and results.
    let left = sizes[i]
    let turn = 0
    let calls = 0
    let at = created
    let lastCall = ''
    while (left > 0) {
      const tid = `turn_${sid}_${turn}`
      nodes.push(node(`msg_${sid}_${pos}`, 'user_message', sid, pos++, at, tid, { text: pick(TITLES) }))
      left--
      for (let loop = 0; left > 0 && loop < 4; loop++) {
        at += 4000
        nodes.push(node(`ast_${sid}_${pos}`, 'assistant_message', sid, pos++, at, tid, { detail: { model: 'glm-5.3-flash', cost_usd: 0.0004 } }))
        left--
        const k = Math.min(left, Math.floor(rnd() * 4))
        const uses: string[] = []
        for (let c = 0; c < k && left > 1; c++) {
          const tool = pick(TOOLS)
          const use = `toolu_${sid}_${pos}`
          const cid = `act_${sid}_${pos}`
          nodes.push(node(`tcl_${sid}_${pos}`, 'tool_call', sid, pos++, at, tid, { detail: { tool, tool_use_id: use, correlation_id: cid, plan: { summary: `${tool} on the ${pick(['bow', 'keel', 'stern', 'mast'])}` } } }))
          uses.push(`${tool}|${use}|${cid}`)
          if (tool === 'proc.run' && rnd() < 0.5) l1.add(cid)
          lastCall = cid
          left--
          calls++
        }
        for (const u of uses) {
          if (left <= 0) break
          const [tool, use, cid] = u.split('|')
          const ext = tool === 'web.search' || tool === 'http.fetch'
          nodes.push(node(`trs_${sid}_${pos}`, 'tool_result', sid, pos++, at + 900, tid, {
            text: 'ok', detail: { tool, tool_use_id: use, correlation_id: cid, status: rnd() < 0.06 ? 'error' : 'ok', is_error: false, external: ext ? { url: 'example' } : null },
          }))
          left--
        }
      }
      turn++
      at += 60_000
    }
    if (state === 'running' && lastCall) running.add(lastCall)
    if (state === 'running' && rnd() < 0.5) streaming.set(sid, now)
    const s = {
      session_id: sid, kind: isTask ? 'task' : 'conversation', label: isTask ? null : pick(PLACES), created_at_unix_ms: created,
      turns: turn, usage: { input_tokens: 0, output_tokens: 0, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 },
      execution_id: eid, execution_state: state, last_active_ms: at, cost_usd: sizes[i] * 0.0003, tool_calls: calls,
      title: pick(TITLES), pending_confirms: level === 'needs_you' ? 1 : 0, parent_session_id: parent?.session_id,
      limit_usd: 5, attention: { level, label: level.replace('_', ' '), since_ms: at },
      external_text: held ? { since_ms: at, tool: 'web.search', url: 'example', node_id: '', query: 'tides' } : undefined,
    } as unknown as SessionInfo
    sessions.push(s)
    if (!isTask) conv.push(i)
    executions.push({
      execution_id: eid, session_id: sid, kind: s.kind, state, turns: turn, interrupted: 0, outstanding: 0, queued_results: 0,
      budget: { limit_usd: 5, spent_usd: s.cost_usd, reserved_usd: state === 'running' ? 0.02 : 0, held_unknown_usd: 0, available_usd: 5 - s.cost_usd, resets: 0 },
      created_at_ms: created, updated_at_ms: at, attention: s.attention,
    } as ExecutionInfo)
    if (isTask && parent) {
      tasks.push({
        task_id: `tsk_${sid}`, short: sid.slice(-4), execution_id: eid, parent_session_id: parent.session_id,
        parent_execution_id: parent.execution_id ?? '', title: s.title, state: state === 'running' ? 'running' : 'complete',
        spent_usd: s.cost_usd, limit_usd: 5, cost_usd: s.cost_usd, turns: turn, pending_confirms: 0, created_at_ms: created, updated_at_ms: at,
      } as TaskInfo)
      const first = nodes.find((n) => n.session_id === sid)
      const brief = [...nodes].reverse().find((n) => n.session_id === parent.session_id && n.kind === 'tool_call')
      if (first && brief) reach.push({ fromSession: parent.session_id, fromNode: brief.node_id, toSession: sid, toNode: first.node_id, via: 'brief' })
    }
    if (level === 'needs_you') {
      confirms.push({ correlation_id: `act_wait_${sid}`, session_id: sid, execution_id: eid, tool: 'proc.run', input: {}, reason: 'asks first', by: 'synth', requested_at_ms: now, expires_at_ms: now + 600_000, floor: false } as ConfirmRequest)
    }
  }
  return {
    sessions, executions, tasks, nodes, confirms, l1, jobsRunning: running, streaming, failedAt: new Map(), reports: new Map(),
    born: new Map(), bornSessions: new Map(), reserved: new Map(), reach, now,
  }
}

function node(id: string, kind: string, sid: string, position: number, at: number, turn: string, o: { text?: string; detail?: Record<string, unknown> }): NodeInfo {
  return { node_id: id, kind, session_id: sid, position, at_unix_ms: at, turn_id: turn, text: o.text ?? '', thinking: '', detail: o.detail ?? null, bytes: 0 } as NodeInfo
}
