// Drag and drop on the Graph and the Board (theseus-jid7t.4): the rules are `ops/drag.ts`'s `dropOf`; this is the hand.
// While a drag hovers an element marked `data-drop`, the cursor says what a drop would do or why it can't; a drop that
// asks (a re-home) opens a sheet naming the effect and the budget sentence, then sends the one call; the daemon's answer
// (its `note`) or its refusal lands in the results toast.
import { useRef, useState, type DragEvent, type ReactNode } from 'react'
import { ArrowRightLeft } from 'lucide-react'
import type { TaskRehomeResult } from '@protocol'
import { useOpsAct } from './act'
import { dropKey, dropOf, type DragItem, type DropTarget } from './drag'
import type { RunResult } from './selection'
import { Sheet } from './BulkBar'

export interface DragDrop {
  begin: (src: DragItem) => void
  /** The drag over `el`: may it drop there? The cursor's words follow the pointer. */
  hover: (el: Element | null, x: number, y: number) => boolean
  drop: (el: Element | null) => void
  end: () => void
  /** The cursor's words, the tray of homes and projects while a drag is on, and the re-home sheet. */
  layer: ReactNode
}

/** The drop's hand. `targetOf` reads a `data-drop` key (`conversation:ses_…`, `lane:<slug>`, `project:tsk_…`); `tray`
 *  is the threads' lanes and the projects, offered as chips while a drag is on (the Board has no lanes). */
export function useDragDrop(targetOf: (key: string) => DropTarget | null, tray: readonly DropTarget[], onResults: (r: RunResult[]) => void): DragDrop {
  const { send, settle, past } = useOpsAct()
  const src = useRef<DragItem | null>(null)
  const [dragging, setDragging] = useState(false)
  const [hint, setHint] = useState<{ x: number; y: number; words: string; ok: boolean } | null>(null)
  const [asking, setAsking] = useState<{ src: DragItem; t: DropTarget; method: string; params: Record<string, string>; words: string } | null>(null)
  const targetIn = (el: Element | null) => { const k = el?.closest('[data-drop]')?.getAttribute('data-drop'); return k ? targetOf(k) : null }

  const run = async (src: DragItem, method: string, params: Record<string, string>, words: string) => {
    const r = await send('drop', method, params)
    const note = r.ok && method === 'task.rehome' ? (r.result as TaskRehomeResult | undefined)?.note : undefined
    onResults([{ id: src.id, key: `${src.id}:drop`, label: words, ok: r.ok, words: r.ok ? note ?? 'done' : r.words }])
    void settle()
  }
  const begin = (s: DragItem) => { src.current = s; setDragging(true) }
  const end = () => { src.current = null; setDragging(false); setHint(null) }
  const hover = (el: Element | null, x: number, y: number) => {
    const s = src.current
    if (!s) return false
    const t = targetIn(el)
    if (!t) { setHint({ x, y, words: s.kind === 'task' ? 'drop on a conversation, a thread’s lane or a project' : 'drop on a project', ok: false }); return false }
    const d = dropOf(s, t, past)
    setHint({ x, y, words: 'refused' in d ? d.refused : d.posture === 'ask' ? `re-home to ${t.title}` : d.words, ok: !('refused' in d) })
    return !('refused' in d)
  }
  const drop = (el: Element | null) => {
    const s = src.current
    end()
    const t = targetIn(el)
    if (!s || !t) return
    dropTo(s, t)
  }
  const dropTo = (src: DragItem, t: DropTarget) => {
    const d = dropOf(src, t, past)
    if ('refused' in d) { onResults([{ id: src.id, key: `${src.id}:drop`, label: `${src.title} → ${t.title}`, ok: false, words: d.refused }]); return }
    if (d.posture === 'ask') setAsking({ src, t, method: d.method, params: d.params, words: d.words })
    else void run(src, d.method, d.params, d.words)
  }
  const chip = (t: DropTarget) => {
    const key = dropKey(t)
    const on = (e: DragEvent) => { if (hover(e.currentTarget, e.clientX, e.clientY)) { e.preventDefault(); e.dataTransfer.dropEffect = 'move' } }
    return (
      <span key={key} data-drop={key} onDragOver={on} onDrop={(e) => { e.preventDefault(); drop(e.currentTarget) }}
        className="rounded-full px-2.5 py-1 text-[11.5px] text-ink ring-1 ring-line-strong hover:bg-white/5">{t.kind === 'lane' ? `thread · ${t.title}` : `◈ ${t.title}`}</span>
    )
  }
  const layer = (
    <>
      {dragging && tray.length > 0 && (
        <div className="fixed bottom-24 left-1/2 z-40 flex max-w-[90vw] -translate-x-1/2 flex-wrap items-center gap-1.5 rounded-xl bg-hull/95 px-3 py-2 shadow-2xl ring-1 ring-gold/40" data-testid="ops-tray">
          <span className="font-display text-[10.5px] font-bold uppercase tracking-[0.08em] text-gold">drop on</span>
          {tray.map(chip)}
        </div>
      )}
      {hint && (
        <div className={`pointer-events-none fixed z-50 max-w-[300px] rounded-md px-2 py-1 text-[11.5px] shadow-xl ring-1 ${hint.ok ? 'bg-hull/95 text-ok ring-ok/40' : 'bg-hull/95 text-fault ring-fault/40'}`}
          style={{ left: hint.x + 16, top: hint.y + 14 }}>{hint.ok ? '✓ ' : '⊘ '}{hint.words}</div>
      )}
      {asking && (
        <Sheet title={`Re-home ${asking.src.title}?`} icon={<ArrowRightLeft size={14} />} go={`Re-home to ${asking.t.title}`} onClose={() => setAsking(null)}
          onGo={() => { const a = asking; setAsking(null); void run(a.src, a.method, a.params, `${a.src.title} → ${a.t.title}`) }}>
          <p className="text-[12.5px] text-ink">{asking.words}</p>
          <p className="mt-2 text-[11.5px] text-ink-faint">One call, <code>task.rehome</code>: its card follows wherever it shows. The daemon may still refuse, in its own words.</p>
        </Sheet>
      )}
    </>
  )
  return { begin, hover, drop, end, layer }
}
