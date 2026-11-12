// The Ops page's Tasks tab (theseus-nuqut): five counts, a click on one lists its tasks with why each waits, and a
// click on a task reads its live narrative (`narrative.line`, filtered to its session by `ops/taskTab.ts`). Acting on
// the task is the Inspector's, one click away ("Open on the Board"): its gated verbs are not reimplemented here.
import { useMemo, useState } from 'react'
import { usePush } from '@/lib/rpc'
import { clock, cn } from '@/lib/format'
import type { OpsRow } from './board'
import { needsEntries } from './needsReason'
import type { QueueItem } from './queue'
import { narrativeOf, tabCounts, tabRows, TAB_COUNTS, TAB_WORDS, type TabCount } from './taskTab'

export function TasksTab({ rows, queue, now, onOpen }: { rows: readonly OpsRow[]; queue: readonly QueueItem[]; now: number; onOpen: (id: string) => void }) {
  const [count, setCount] = useState<TabCount>('needs')
  const [task, setTask] = useState<string | null>(null)
  const counts = useMemo(() => tabCounts(rows, now), [rows, now])
  const listed = useMemo(() => tabRows(rows, now, count), [rows, now, count])
  const why = useMemo(() => new Map(needsEntries(rows, queue).map((e) => [e.task, e.reason])), [rows, queue])
  const chosen = listed.find((r) => r.id === task) ?? null
  const narrative = usePush((s) => s.narrative)
  const on = usePush((s) => s.narrativeOn)
  const lines = useMemo(() => (chosen ? narrativeOf(narrative, chosen) : []), [narrative, chosen])
  return (
    <div className="flex h-full min-h-0 flex-col gap-2" data-testid="tasks-tab">
      <div role="tablist" aria-label="Task counts" className="flex flex-wrap gap-2">
        {TAB_COUNTS.map((c) => (
          <button key={c} role="tab" aria-selected={c === count} onClick={() => { setCount(c); setTask(null) }}
            className={cn('rounded-lg px-3 py-2 text-left ring-1 ring-line', c === count ? 'bg-gold/15 text-gold' : 'text-ink-dim hover:text-ink')}>
            <span className="num block text-[22px] leading-none">{counts[c]}</span>
            <span className="text-[11px] uppercase tracking-[0.08em]">{TAB_WORDS[c]}</span>
          </button>
        ))}
      </div>
      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)] gap-2.5">
        <ul aria-label={TAB_WORDS[count]} className="min-h-0 overflow-auto rounded-xl border border-line bg-hull/60 p-1">
          {listed.length === 0 && <li className="px-3 py-2 text-[12px] text-ink-faint">nothing here</li>}
          {listed.map((r) => (
            <li key={r.id}>
              <button onClick={() => setTask(r.id)} aria-current={r.id === task} className={cn('w-full rounded-md px-3 py-1.5 text-left text-[12px] hover:bg-white/5', r.id === task && 'bg-white/10')}>
                <span className="block truncate text-ink">{r.title}</span>
                {count === 'needs' && <span className="block truncate text-wait">waiting on you: {why.get(r.id)}</span>}
              </button>
            </li>
          ))}
        </ul>
        <section aria-label="Narrative" className="flex min-h-0 flex-col rounded-xl border border-line bg-hull/60">
          <header className="flex items-center gap-2 border-b border-line px-3 py-1 text-[11px] text-ink-faint">
            <span className="min-w-0 flex-1 truncate">{chosen ? chosen.title : 'pick a task'}</span>
            {chosen && <button onClick={() => onOpen(chosen.id)} className="text-gold hover:underline">Open on the Board</button>}
          </header>
          <ol className="min-h-0 flex-1 overflow-auto px-3 py-1 font-mono text-[11px]">
            {chosen && on === false && <li className="text-ink-faint">the narrative is off ([narrative] in the config)</li>}
            {chosen && on !== false && lines.length === 0 && <li className="text-ink-faint">no sentences yet: they arrive as it works</li>}
            {lines.map((l) => <li key={l.seq} className="flex gap-2"><span className="num shrink-0 text-ink-faint">{clock(l.at_unix_ms)}</span><span className="min-w-0 text-ink-dim">{l.text}</span></li>)}
          </ol>
        </section>
      </div>
    </div>
  )
}
