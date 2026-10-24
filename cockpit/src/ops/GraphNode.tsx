// The Graph lens's nodes (theseus-jid7t.4): a work node (a conversation, the anchor of its lane; a task; a step) with
// its state header, glyph and word, short id, title, progress line, spend bar, model chip, place and a lock for private
// or sensitive data; a job as a pill beside its session; the folds ("✓ N done", "+N folded"); the "+ future" nodes;
// and a lane's band with its label. Colour is for state and always goes with a glyph or a word; glows only for what
// needs the owner (amber), what is picked or focused (gold) and what is live (cyan).
import { memo, type CSSProperties } from 'react'
import { Handle, Position, type Node, type NodeProps } from '@xyflow/react'
import { Lock } from 'lucide-react'
import type { DataMark } from '@protocol'
import { progressLine } from '@/lib/progressLine'
import { taskShort } from '@/lib/taskgraph'
import { stateTone, toneHex } from '@/lib/taxonomy'
import { clock, cn, usd } from '@/lib/format'
import type { OpsRow } from './board'
import { bucketGlyph, bucketOfNode, bucketTone, FADED, FILTERED_OUT, look } from './state'
import { masked, NODE_W, shownTitle, type GLane, type GNode } from './graph'

export interface NodeData extends Record<string, unknown> {
  n: GNode
  row?: OpsRow
  now: number
  focused: boolean
  picked: boolean
  /** Outside the filters and the search: drawn faint, kept in place. */
  dim: boolean
  /** A drag hovers it: `ok` takes the drop, `no` refuses it. */
  hover?: 'ok' | 'no'
  onFold?: () => void
  /** `?redact=1`: a private or sensitive node shows its kind and short id for its title. */
  redact?: boolean
}

export interface LaneData extends Record<string, unknown> { lane: GLane; width: number; hover?: 'ok' | 'no'; redact?: boolean }

/** Each state's glyph: shape always goes with colour. */
const GLYPH: Readonly<Record<string, string>> = {
  running: '◐', waiting: '◔', needs_you: '●', queued: '◷', paused: '⏸', ready: '○', done: '✓', failed: '✗', cancelled: '⊘', expired: '⌛',
  complete: '✓', in_progress: '◐', blocked: '■', waiting_human: '●', suspended: '⏸', abandoned: '⊘', proposed: '○', accepted: '○', open: '○', unknown: '·',
}
const FUTURE_GLYPH: Readonly<Record<string, string>> = { wake: '◷', routine: '↻', watch: '◎', project: '◈' }

const locked = (d: DataMark | undefined) => d === 'private' || d === 'sensitive'

/** The ring and glow a node wears: amber for what needs the owner, gold for the picked or focused, the drop's answer. */
function ring(d: NodeData, tone: string): CSSProperties {
  const gold = 'var(--color-gold)'
  if (d.hover === 'ok') return { boxShadow: `0 0 0 2px ${toneHex.ok}, 0 0 22px -4px ${toneHex.ok}` }
  if (d.hover === 'no') return { boxShadow: `0 0 0 2px ${toneHex.fault}` }
  const glow = d.n.needs ? `0 0 26px -6px ${toneHex.wait}` : d.n.state === 'running' ? `0 0 18px -8px ${toneHex.live}` : ''
  const edge = d.picked || d.focused ? `0 0 0 ${d.focused ? 2 : 1.5}px ${gold}, 0 0 20px -4px color-mix(in srgb, ${gold} 66%, transparent)` : `0 0 0 1px ${d.n.needs ? toneHex.wait : tone}55`
  return { boxShadow: [edge, glow].filter(Boolean).join(', ') }
}

/** The keys a needs-you node prints on itself: its answers, as the page's keys take them once it is focused. */
function keysOf(d: NodeData): string {
  const v = d.n.view
  if (v?.state === 'failed') return 'r retry · d dismiss'
  const choices = v?.question?.choices?.length ?? 0
  if (choices) return `1–${Math.min(6, choices)} choose`
  return 'y approve · n decline'
}

const Pick = ({ on }: { on: boolean }) => (
  <span aria-label={on ? 'picked' : 'not picked'} className={cn('flex h-3 w-3 shrink-0 items-center justify-center rounded-[3px] text-[8px] ring-1', on ? 'bg-gold text-deck ring-gold' : 'ring-ink-faint/60')}>{on ? '✓' : ''}</span>
)

function Work({ data: d }: NodeProps<Node<NodeData>>) {
  const { n, row, now } = d
  const bucket = bucketOfNode(n, now)
  const { faded } = look(bucket, d.dim)
  const tone = toneHex[bucketTone(bucket, n.state) ?? stateTone(n.state === 'paused' ? 'queued' : n.state)] ?? toneHex.idle
  const conv = n.kind === 'conversation'
  const v = n.view
  const cost = v?.cost ?? (row?.task?.info ? { spent_usd: row.task.info.spent_usd, limit_usd: row.task.info.limit_usd } : undefined)
  const line = v && !masked(n, !!d.redact) && !['done', 'failed', 'cancelled', 'expired'].includes(v.state) ? progressLine(v, now) : undefined
  const title = shownTitle(n, !!d.redact)
  const shortId = n.kind === 'step' ? taskShort(n.id) : row?.short ?? n.id.slice(-6)
  return (
    <div data-drop={conv && row?.sessionId ? `conversation:${row.sessionId}` : undefined} data-node={n.id}
      className={cn('rounded-lg bg-hull/95 px-2.5 py-2 backdrop-blur transition-opacity', n.fresh && 'ops-fade-in', d.dim && FILTERED_OUT, faded && FADED, conv && 'bg-[#0d2036]/95')}
      style={{ width: NODE_W, ...ring(d, tone) }}>
      <Handle type="target" position={Position.Left} className="!h-1.5 !w-1.5 !border-0 !bg-line-strong" />
      <div className="flex items-center gap-1.5 text-[10.5px]" style={{ color: n.needs ? toneHex.wait : tone }}>
        <Pick on={d.picked} />
        <span aria-hidden>{bucketGlyph(bucket, n.state) ?? GLYPH[n.state] ?? '·'}</span>
        <span className="font-semibold uppercase tracking-wider">{n.state.replace('_', ' ')}</span>
        {locked(n.data) && <span title={`${n.data} data: US-origin models only${n.data === 'sensitive' ? ', never a shared place' : ''}`} className="flex items-center gap-0.5 text-ink-dim"><Lock size={10} aria-label={n.data} />{n.data}</span>}
        {n.asks > 0 && <span className="rounded-full bg-wait/15 px-1.5 text-wait" title="questions it asks: answer them in the queue">? {n.asks}</span>}
        <span className="num ml-auto text-ink-faint" title={n.id}>{conv ? 'conversation' : n.kind === 'step' ? `step · ${shortId}` : shortId}</span>
      </div>
      <div className={cn('mt-1 line-clamp-2 leading-snug text-ink', conv ? 'font-display text-[13px] font-bold tracking-[0.02em]' : 'text-[12px] font-medium')} title={title}>{title}</div>
      {line && <div className="num mt-0.5 truncate text-[10.5px] text-ink-dim" title={line}>{line}</div>}
      {cost && cost.limit_usd > 0 && (
        <div className="mt-1.5" title={`${usd(cost.spent_usd)} of ${usd(cost.limit_usd)}`}>
          <div className="h-1 overflow-hidden rounded-full bg-white/5"><div className="h-full rounded-full bg-money/80" style={{ width: `${Math.min(100, (cost.spent_usd / cost.limit_usd) * 100)}%` }} /></div>
          <div className="num mt-0.5 text-[9.5px] text-ink-faint"><span className="text-money">{usd(cost.spent_usd)}</span> of {usd(cost.limit_usd)}</div>
        </div>
      )}
      {(row?.model || row?.place) && (
        <div className="mt-1 flex flex-wrap gap-1 text-[9.5px]">
          {row.model && <span className="rounded bg-model/10 px-1 text-model ring-1 ring-model/30" title={v?.moved_to ? 'a checkpoint moved it to this profile' : 'the profile it runs on'}>◇ {row.model}</span>}
          {row.place && <span className="rounded bg-white/5 px-1 text-ink-dim ring-1 ring-line">{row.place}</span>}
        </div>
      )}
      {n.needs && <div className="num mt-1.5 rounded bg-wait/10 px-1.5 py-0.5 text-[10px] text-wait ring-1 ring-wait/30">needs you · {keysOf(d)}</div>}
      <Handle type="source" position={Position.Right} className="!h-1.5 !w-1.5 !border-0 !bg-line-strong" />
    </div>
  )
}

function Job({ data: d }: NodeProps<Node<NodeData>>) {
  const { n } = d
  const tone = toneHex[stateTone(n.state)] ?? toneHex.idle
  const { faded } = look(bucketOfNode(n, d.now), d.dim)
  return (
    <div className={cn('flex w-[210px] items-center gap-1.5 rounded-full bg-hull/95 px-2.5 py-1 text-[10.5px]', n.fresh && 'ops-fade-in', d.dim && FILTERED_OUT, faded && FADED)} style={ring(d, tone)} title={`job · ${shownTitle(n, !!d.redact)}`}>
      <Handle type="target" position={Position.Left} className="!h-1 !w-1 !border-0 !bg-line-strong" />
      <Pick on={d.picked} />
      <span style={{ color: tone }} aria-hidden>{GLYPH[n.state] ?? '·'}</span>
      <span className="num min-w-0 flex-1 truncate text-ink-dim">⚙ {shownTitle(n, !!d.redact)}</span>
      <span className="text-[9.5px]" style={{ color: tone }}>{n.state}</span>
    </div>
  )
}

function Fold({ data: d }: NodeProps<Node<NodeData>>) {
  const { n } = d
  const done = n.kind === 'done'
  return (
    <button type="button" onClick={d.onFold} disabled={!d.onFold} className={cn('w-[180px] rounded-full px-3 py-1.5 text-left text-[11.5px] ring-1', n.fresh && 'ops-fade-in', done ? 'bg-ok/[0.07] text-ok ring-ok/40 hover:bg-ok/15' : 'bg-white/[0.03] text-ink-dim ring-line hover:bg-white/[0.07]')}
      title={done ? `${n.members!.length} ended over an hour ago: open them` : `${n.members!.length} nodes folded to keep the graph under its cap: open them (other subtrees fold instead), or narrow it with the filters or the search`}>
      <Handle type="target" position={Position.Left} className="!h-1 !w-1 !border-0 !bg-line-strong" />
      {n.title}
    </button>
  )
}

function Future({ data: d }: NodeProps<Node<NodeData>>) {
  const { n } = d
  const v = n.view
  const project = n.kind === 'project'
  return (
    <div data-drop={project ? `project:${n.id}` : undefined} className={cn('w-[210px] rounded-lg border border-dashed border-think/50 bg-hull/90 px-2.5 py-1.5', d.dim && FILTERED_OUT)} style={d.hover ? ring(d, toneHex.think) : undefined}>
      <Handle type="target" position={Position.Left} className="!h-1 !w-1 !border-0 !bg-line-strong" />
      <div className="flex items-center gap-1.5 text-[10px] text-think"><span aria-hidden>{FUTURE_GLYPH[n.kind]}</span><span className="uppercase tracking-wider">{n.kind}</span>
        {v?.next_at_ms != null && <span className="num ml-auto text-ink-faint">{clock(v.next_at_ms)}</span>}</div>
      <div className="mt-0.5 line-clamp-2 text-[11.5px] text-ink">{shownTitle(n, !!d.redact)}</div>
      {project && <div className="mt-0.5 text-[9.5px] text-ink-faint">drop a task or a conversation here to attach it</div>}
    </div>
  )
}

/** A lane's band and its engraved label in the gutter: the thread's title, warmth, context, today's dollars, seal. */
function Lane({ data: d }: NodeProps<Node<LaneData>>) {
  const { lane } = d
  const home = lane.key === 'home'
  return (
    <div data-drop={home ? undefined : `lane:${lane.key}`} className="relative rounded-xl"
      style={{ width: d.width, height: lane.height, background: 'linear-gradient(90deg, rgb(214 165 72 / 0.06), rgb(10 24 40 / 0.35) 220px, rgb(10 24 40 / 0.18))',
        boxShadow: d.hover === 'ok' ? `inset 0 0 0 2px ${toneHex.ok}` : d.hover === 'no' ? `inset 0 0 0 2px ${toneHex.fault}` : lane.needs ? `inset 0 0 0 1px ${toneHex.wait}44` : 'inset 0 0 0 1px rgb(176 141 87 / 0.14)' }}>
      <div className="absolute left-4 top-4 w-[180px]">
        <div className="font-display text-[15px] font-bold uppercase leading-tight tracking-[0.12em] text-gold" style={{ textShadow: '0 1px 0 rgb(0 0 0 / 0.6), 0 -1px 0 rgb(255 230 180 / 0.12)' }}>
          {lane.sealed && <Lock size={11} className="mr-1 inline text-ink-faint" aria-label="sealed" />}{shownTitle(lane, !!d.redact)}
        </div>
        {lane.words && !masked(lane, !!d.redact) && <div className="num mt-1 text-[10.5px] leading-snug text-ink-dim">{lane.words}</div>}
        {lane.needs > 0 && <div className="mt-1.5 inline-block rounded-full bg-wait/15 px-2 text-[10.5px] text-wait">● {lane.needs} need{lane.needs === 1 ? 's' : ''} you</div>}
      </div>
    </div>
  )
}

export const NODE_TYPES = { work: memo(Work), job: memo(Job), fold: memo(Fold), future: memo(Future), lane: memo(Lane) }

/** The node type that draws a kind. */
export const typeOf = (k: GNode['kind']): keyof typeof NODE_TYPES =>
  k === 'job' ? 'job' : k === 'done' || k === 'folded' ? 'fold' : k === 'wake' || k === 'routine' || k === 'watch' || k === 'project' ? 'future' : 'work'
