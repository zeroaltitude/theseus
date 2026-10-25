// The Board lens (theseus-jid7t.3): five columns, Needs you, Working, Paused·queued, Scheduled and Done (today, then
// older, fifty at a time), each card every field of the Tasks page's card and a model chip, drawn from `cardFacts`.
// A card's questions are answered in place in the queue and the inspector; the card says how many wait. Each card has
// a box that picks it for a bulk act (⇧/⌘-click too, theseus-jid7t.4), a task or a conversation drags onto a
// conversation card or the tray's homes and projects (`ops/DropLayer.tsx`), and a conversation card takes a drop.
import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type DragEvent } from 'react'
import { Link } from 'react-router'
import { Lock, OctagonX, Pause, Play } from 'lucide-react'
import type { ConfirmRequest } from '@protocol'
import { cn, short } from '@/lib/format'
import { Pill, StatePill } from '@/components/ui'
import { COLUMN_TITLES, laneRows, locked, type BoardColumns, type Column, type OpsRow } from './board'
import { cardFacts } from './facts'
import { useOpsAct } from './act'
import { ActBtn } from './ActBtn'
import { dragOf } from './rehome'
import { formsOf, STEP_WORDS, toneVar, type Progress, type RowForm } from './forms'
import { toneHex } from '@/lib/taxonomy'
import type { DragDrop } from './DropLayer'
import type { Selection } from './selection'

/** Each column's glyph and tone: shape always goes with colour. */
const LOOK: Record<Column, { glyph: string; tone: string; edge: string }> = {
  needs: { glyph: '●', tone: 'text-wait', edge: 'border-wait' },
  working: { glyph: '◐', tone: 'text-live', edge: 'border-live' },
  paused: { glyph: '⏸', tone: 'text-ink-dim', edge: 'border-ink-faint/50' },
  scheduled: { glyph: '◷', tone: 'text-think', edge: 'border-think/60' },
  done: { glyph: '✓', tone: 'text-ok', edge: 'border-line' },
}

export function BoardLens({ cols, rows: all = [], older, onMore, focus, onFocus, onContext, confirms, now, picked, onPick, drag, titleOf }: {
  cols: BoardColumns;
  /** The one work set, for a plan's progress and a step's glyph: the columns hold only the rows that have one. */
  rows?: readonly OpsRow[]; older: number; onMore: () => void; focus: string | null; onFocus: (id: string) => void
  /** A right-click on a card: its verbs (`VerbMenu`). */
  onContext?: (id: string, x: number, y: number) => void
  confirms: readonly ConfirmRequest[]; now: number; picked: Selection; onPick: (id: string) => void; drag: DragDrop; titleOf: (sessionId: string) => string
}) {
  const lanes = laneRows(cols, older)
  const forms = useMemo(() => formsOf(all, now), [all, now])
  // The owner's plan folds, kept here as the Graph keeps its own (true open, false folded; a plan opens while it is open).
  const [folds, setFolds] = useState<ReadonlyMap<string, boolean>>(new Map())
  // The drag's hand reads the latest rows and titles; the cards keep one callback, so their memo holds.
  const live = useRef({ lanes, drag, titleOf })
  useLayoutEffect(() => { live.current = { lanes, drag, titleOf } })
  const onDrag = useCallback((id: string) => {
    const r = live.current.lanes.flat().find((x) => x.id === id)
    if (r) live.current.drag.begin(dragOf(r, live.current.titleOf))
  }, [])
  const over = (e: DragEvent) => { if (drag.hover(e.target as Element, e.clientX, e.clientY)) { e.preventDefault(); e.dataTransfer.dropEffect = 'move' } }
  return (
    <div className="grid h-full min-h-0 auto-cols-[minmax(196px,1fr)] grid-flow-col gap-2.5 overflow-x-auto pb-1" data-testid="ops-board"
      onDragOver={over} onDrop={(e) => { e.preventDefault(); drag.drop(e.target as Element) }} onDragEnd={() => drag.end()}>
      {(['needs', 'working', 'paused', 'scheduled', 'done'] as const).map((col, i) => {
        const rows = lanes[i]
        const count = col === 'done' ? cols.done.length : rows.length
        return (
          <section key={col} aria-label={COLUMN_TITLES[col]} className="flex min-h-0 flex-col rounded-xl border border-line bg-hull/60">
            <header className="flex items-baseline gap-2 border-b border-line px-3 py-2">
              <span className={cn('text-[12px]', LOOK[col].tone)} aria-hidden>{LOOK[col].glyph}</span>
              <h3 className="font-display text-[11.5px] font-bold uppercase tracking-[0.08em] text-gold">{col === 'done' ? 'Done today' : COLUMN_TITLES[col]}</h3>
              <span className="num ml-auto text-[11px] text-ink-dim">{count}</span>
            </header>
            <div className="flex min-h-0 flex-1 flex-col gap-1.5 overflow-y-auto p-1.5">
              {!rows.length && <div className="px-2 py-6 text-center text-[11.5px] text-ink-faint">{EMPTY[col]}</div>}
              {rows.map((r, j) => (
                <div key={r.id}>
                  {col === 'done' && j === cols.done.length && <div className="px-1 pb-1 pt-2 font-display text-[10px] font-bold uppercase tracking-[0.08em] text-ink-faint">older</div>}
                  <Card r={r} now={now} focused={focus === r.id} onFocus={onFocus} onContext={onContext} asks={confirms.filter((c) => c.session_id === r.sessionId).length} picked={picked.has(r.id)} onPick={onPick} onDrag={onDrag}
                    form={forms.get(r.id)} open={folds.get(r.id) ?? r.bucket !== 'ended'} onPlan={() => setFolds((m) => new Map(m).set(r.id, !(m.get(r.id) ?? r.bucket !== 'ended')))} />
                </div>
              ))}
              {col === 'done' && cols.older.length > older && (
                <button onClick={onMore} className="mx-2 my-1.5 text-left text-[12px] text-live hover:underline">more ({cols.older.length - older} older)</button>
              )}
            </div>
          </section>
        )
      })}
    </div>
  )
}

const EMPTY: Record<Column, string> = {
  needs: 'nothing waits for you', working: 'nothing is working', paused: 'nothing paused or queued', scheduled: 'nothing more due today', done: 'nothing ended today',
}

/** One card: every field of the Tasks page's card, and the model chip. */
export const Card = memo(function Card({ r, now, focused, onFocus, onContext, asks, picked, onPick, onDrag, form, open = true, onPlan }: {
  r: OpsRow; now: number; focused: boolean; onFocus: (id: string) => void; onContext?: (id: string, x: number, y: number) => void; asks: number; picked: boolean; onPick: (id: string) => void; onDrag: (id: string) => void
  /** Its form (`ops/forms.ts`): a step's glyph, a plan's bar and fold. */
  form?: RowForm; open?: boolean; onPlan?: () => void
}) {
  const ref = useRef<HTMLDivElement>(null)
  const { busy, act, on } = useOpsAct()
  const f = cardFacts(r, now)
  useEffect(() => { if (focused) ref.current?.scrollIntoView({ block: 'nearest', behavior: 'smooth' }) }, [focused])
  return (
    <div ref={ref} role="button" tabIndex={-1} aria-selected={focused} data-row={r.id} onClick={(e) => (e.shiftKey || e.metaKey || e.ctrlKey ? onPick(r.id) : onFocus(r.id))}
      onContextMenu={(e) => { if (onContext) { e.preventDefault(); onFocus(r.id); onContext(r.id, e.clientX, e.clientY) } }}
      data-drop={r.kind === 'conversation' && r.sessionId ? `conversation:${r.sessionId}` : undefined}
      draggable={r.kind === 'task' || r.kind === 'conversation'} onDragStart={(e) => { e.dataTransfer.effectAllowed = 'move'; e.dataTransfer.setData('text/plain', r.id); onDrag(r.id) }}
      className={cn('cursor-pointer rounded-lg border-l-2 bg-deck/70 px-2.5 py-2 ring-1 ring-transparent transition-shadow hover:ring-line', LOOK[r.column ?? 'paused'].edge,
        r.column === 'needs' && 'bg-wait/[0.05] shadow-[0_0_14px_-4px_var(--color-wait)]',
        picked && 'bg-gold/[0.06] ring-gold/50',
        focused && 'ring-gold shadow-[0_0_16px_-2px_rgb(214_165_72_/_0.55)]')}>
      <div className="flex items-center gap-1.5">
        <input type="checkbox" checked={picked} onChange={() => onPick(r.id)} data-pick={r.id} onClick={(e) => e.stopPropagation()} aria-label={`pick ${f.title}`} title="pick it for a bulk act (␣ on the focused card)"
          className="h-3 w-3 shrink-0 cursor-pointer accent-gold" />
        {form?.form === 'plan' && <button type="button" onClick={(e) => { e.stopPropagation(); onPlan?.() }} aria-expanded={open} aria-label={open ? 'fold the plan' : 'open the plan'} className="text-[12px] leading-none text-ink-dim hover:text-ink">{open ? '▾' : '▸'}</button>}
        {form?.look && <span aria-hidden className="text-[12px] leading-none" style={{ color: toneVar(form.look.tone) }} title={STEP_WORDS[form.look.word]}>{form.look.glyph}</span>}
        {f.paused ? <Pill tone="wait" title="held by the owner: resume lets it go on">{f.pill}</Pill> : <StatePill state={f.pill} />}
        {locked(r) && <Lock size={11} className="text-ink-faint" aria-label={`${r.data} data`} />}
        <span className="num ml-auto text-[10px] text-ink-faint" title={r.id}>{form?.form === 'plan' ? `PLAN · ${short(r.recordId ?? r.id)}` : r.kind === 'step' ? `STEP · ${short(r.id)}` : r.kind === 'task' ? f.short : `${r.kind} · ${short(r.id)}`}</span>
      </div>
      <div className="mt-1 flex items-start gap-1.5">
        {f.href
          ? <Link to={f.href} onClick={(e) => e.stopPropagation()} className={cn('min-w-0 flex-1 text-[12.5px] font-medium leading-snug text-ink hover:text-live', r.kind === 'conversation' && 'font-display tracking-[0.02em]')} title={f.title}>{f.title}</Link>
          : <span className="min-w-0 flex-1 text-[12.5px] font-medium leading-snug text-ink">{f.title}</span>}
      </div>
      {form?.form === 'plan' && form.progress && (
        <div data-testid="plan-progress">
          <PlanBar p={form.progress} />
          {(!open || form.progress.needs > 0) && <div className="num text-[10.5px] text-ink-dim">{form.progress.summary}{!open && form.progress.needs > 0 && <span className="ml-1.5 text-wait">● {form.progress.needs} needs you</span>}</div>}
          {open && form.kids && form.kids.length > 0 && (
            <ul className="mt-1 flex flex-col gap-0.5 text-[11px]">
              {form.kids.slice(0, 8).map((k) => <li key={k.id} className="flex items-center gap-1.5 truncate" title={`${STEP_WORDS[k.look.word]}: ${k.title}`}><span aria-hidden style={{ color: toneVar(k.look.tone) }}>{k.look.glyph}</span><span className={cn('truncate', k.look.faded ? 'text-ink-faint' : 'text-ink-dim')}>{k.title}</span></li>)}
              {form.kids.length > 8 && <li className="text-ink-faint">+{form.kids.length - 8} more</li>}
            </ul>
          )}
        </div>
      )}
      {(f.place || f.model) && (
        <div className="mt-1 flex flex-wrap gap-1">
          {f.place && <Pill tone="idle" title={f.placeTip}>{f.place}</Pill>}
          {f.model && <Pill tone="model" title={r.view?.moved_to ? 'a checkpoint moved it to this profile' : 'the profile it runs on'}>◇ {f.model}</Pill>}
        </div>
      )}
      {(f.progress || f.doing) && (
        <div className="num mt-1 flex flex-col text-[11px]">
          {f.progress && <span className="text-ink" data-testid="progress-line">{f.progress}</span>}
          {f.doing && <span className="truncate text-ink-dim">{f.doing}</span>}
        </div>
      )}
      {f.due && <div className="num mt-1 text-[11px] text-think">◷ {f.due}</div>}
      {(f.outcome || f.report) && (
        <div className="mt-1 text-[11.5px] text-ink-dim">
          {f.outcome && <span className="mr-1.5 text-fault">✗ {f.outcome}</span>}
          {f.report === 'no report line' ? <span className="text-ink-faint">{f.report}</span> : f.report}
        </div>
      )}
      {f.question && <div className="mt-1.5 line-clamp-3 rounded-md bg-black/25 px-2 py-1 text-[11.5px] text-ink ring-1 ring-wait/30" title={f.question}>? {f.question} <span className="text-ink-faint">· answer it in the queue</span></div>}
      {f.approvals && asks > 0 && <div className="mt-1.5 text-[11px] text-wait">◆ {asks} approval{asks === 1 ? '' : 's'} waiting · in the queue</div>}
      <div className="num mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-[10.5px] text-ink-faint">
        {f.cost && <span className="text-money">{f.cost}</span>}
        <span title="when it last changed">{f.activity}</span>
        {f.waitsOn && <span>{f.waitsOn}</span>}
      </div>
      <div className="mt-1 flex flex-wrap items-center justify-end gap-1.5 text-[10.5px]" onClick={(e) => e.stopPropagation()}>
          {f.open && f.href && <Link to={f.href} className="mr-auto whitespace-nowrap text-live hover:underline" title="its page: the transcript, and a composer that messages it">{f.open}</Link>}
          {f.pause && (
            <ActBtn verb="pause" on={on} tone="wait" busy={busy === r.id} onClick={() => act('pause', r.id, r.paused ? 'task.resume' : 'task.pause', { task: r.id })} title={r.paused ? 'Let it go on' : 'Hold it: it takes no new step until resumed'}>
              {r.paused ? <><Play size={11} /> Resume</> : <><Pause size={11} /> Pause</>}
            </ActBtn>
          )}
          {f.stop && <ActBtn verb="cancel" on={on} tone="fault" busy={busy === `${r.id}:stop`} onClick={() => act('cancel', `${r.id}:stop`, 'task.cancel', { task: r.id }, `Stop task ${f.short} (${f.title})? Its jobs stop, and it reports that it was cancelled.`)}><OctagonX size={11} /> Stop</ActBtn>}
      </div>
    </div>
  )
})

/** A plan's n/m bar: done in the ok tone, the open segments by bucket. */
const SEGMENTS: ReadonlyArray<readonly [keyof Progress['counts'], string]> = [['needs', toneHex.wait], ['working', toneHex.live], ['paused', toneHex.wait], ['scheduled', toneHex.think], ['todo', 'var(--color-ink-faint)']]
export function PlanBar({ p }: { p: Progress }) {
  const m = Math.max(1, p.m)
  return (
    <div className="mt-1.5" title={p.summary || 'nothing listed yet'}>
      <div className="flex h-1.5 overflow-hidden rounded-full bg-white/5" role="img" aria-label={`${p.n} of ${p.m} done`}>
        <div style={{ width: `${(p.n / m) * 100}%`, background: toneHex.ok }} />
        {SEGMENTS.map(([b, c]) => p.counts[b] > 0 && <div key={b} style={{ width: `${(p.counts[b] / m) * 100}%`, background: c, opacity: b === 'paused' ? 0.55 : 0.85 }} />)}
      </div>
      <div className="num mt-0.5 flex text-[9.5px] text-ink-faint"><span className="text-ok">{p.n}/{p.m}</span></div>
    </div>
  )
}
