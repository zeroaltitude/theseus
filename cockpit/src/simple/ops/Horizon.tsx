// A long-horizon task's plan, its next steps and what it is waiting on, with the two changes the daemon offers: add a
// step, close a step (`./horizon.ts`). It is the Inspector's own section for a project, so the simple Ops page and the
// Ship's Operations page draw the one component and cannot differ. It reads `project.show` once and again when the
// push says a task changed (never on a timer); each change is one call through `useOpsAct`, so it is off under the
// time machine as every act is, and the list it answers with is read again.
import { useEffect, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { Plus } from 'lucide-react'
import type { ProjectShowResult } from '@protocol'
import { usePush, useRpc } from '@/lib/rpc'
import { cn } from '@/lib/format'
import { when } from '@/lib/figures'
import { HelpButton } from '@/components/Help'
import { useOpsAct } from '@/ops/act'
import { ActBtn } from '@/ops/ActBtn'
import { Section } from '@/ops/InspectorSections'
import { useOpsUi } from '@/ops/ui'
import { addItemCall, closeItemCall, horizonOf, itemRefusal, ITEM_MAX, type HorizonStep } from './horizon'

export function Horizon({ project, now, onPick }: { project: string; now: number; onPick?: (id: string) => void }) {
  const qc = useQueryClient()
  const { post, on } = useOpsAct()
  const { data, error } = useRpc<ProjectShowResult>('project.show', { id: project }, 0)
  const last = usePush((s) => s.events[s.events.length - 1])
  useEffect(() => {
    if (last && ['task.changed', 'work.changed'].includes(last.method)) void qc.invalidateQueries({ queryKey: ['project.show'] })
  }, [last, qc])
  const [text, setText] = useState('')
  const [busy, setBusy] = useState<string | null>(null)
  const [refused, setRefused] = useState<string | null>(null)
  if (error) return <div className="text-[12px] text-ink-faint">The project could not be read: {String((error as Error).message ?? error)}</div>
  if (!data) return <div className="text-[12px] text-ink-faint">Reading the project…</div>
  const h = horizonOf(data.project, data.records, now)

  const change = async (key: string, call: { method: string; params: unknown }, verb: 'project_item_add' | 'project_item_close', done?: string) => {
    setBusy(key)
    setRefused(null)
    try {
      await post(verb, call.method, call.params)
      await qc.invalidateQueries({ queryKey: ['project.show'] })
      if (done) useOpsUi.getState().say(done)
      if (verb === 'project_item_add') setText('')
    } catch (e) { setRefused((e as Error).message ?? String(e)) } finally { setBusy(null) }
  }
  const add = () => {
    const why = itemRefusal(text)
    if (why) { setRefused(why); return }
    void change('add', addItemCall(h.id, text), 'project_item_add', `Added to ${h.title}: ${text.trim()}`)
  }
  const close = (s: HorizonStep) => void change(s.id, closeItemCall(s.id), 'project_item_close', `Closed: ${s.title}`)

  return (
    <Section title="Horizon" hint={<><span>{h.total > 0 ? `${h.done} of ${h.total} done` : 'nothing counted yet'}</span><HelpButton id="simple.ops.horizon" title="Horizon" /></>}>
      <div className="flex flex-col gap-3" data-testid="horizon">
        {h.objective && <p className="text-[12px] text-ink"><span className="text-ink-faint">for </span>{h.objective}</p>}
        <Group title="Plan" empty="No milestones are set. The plan is the steps below." steps={h.plan} now={now} onPick={onPick} />
        <Group title="Doing now" empty="Nothing is being worked." steps={h.doing} now={now} onPick={onPick} close={close} busy={busy} on={on} />
        <Group title="Next steps" empty="No step is waiting to start." steps={h.next} now={now} onPick={onPick} close={close} busy={busy} on={on} />
        <div>
          <div className="simple-eyebrow mb-1">Waiting on</div>
          {h.waiting.length === 0
            ? <p className="text-[12px] text-ink-faint">Nothing. No step waits on a person, on another step, or on a date that has passed.</p>
            : <ul className="flex flex-col gap-0.5 text-[12px]">{h.waiting.map((w) => (
              <li key={w.id} className="flex items-baseline gap-2"><span aria-hidden className="text-wait">●</span>
                {onPick ? <button type="button" onClick={() => onPick(w.id)} className="text-left text-ink hover:underline">{w.title}</button> : <span className="text-ink">{w.title}</span>}
                <span className="text-ink-faint">{w.why}</span></li>
            ))}</ul>}
        </div>
        <form className="flex flex-wrap items-center gap-1.5" onSubmit={(e) => { e.preventDefault(); add() }}>
          <input value={text} onChange={(e) => { setText(e.target.value); setRefused(null) }} maxLength={ITEM_MAX + 50} aria-label="A new step for this project" placeholder="Add a step, in one line"
            className="min-w-0 flex-1 rounded-md bg-white/5 px-2 py-1 text-[12px] text-ink outline-none ring-1 ring-line placeholder:text-ink-faint focus:ring-live/40" />
          <ActBtn verb="project_item_add" on={on} tone="live" busy={busy === 'add'} onClick={add} title="Add this step to the project’s list"><Plus size={11} aria-hidden /> Add</ActBtn>
        </form>
        {refused && <p role="alert" className="rounded-md bg-fault/10 px-2 py-1 text-[12px] text-fault ring-1 ring-fault/30">{refused}</p>}
      </div>
    </Section>
  )
}

function Group({ title, empty, steps, now, onPick, close, busy, on }: {
  title: string; empty: string; steps: readonly HorizonStep[]; now: number; onPick?: (id: string) => void
  close?: (s: HorizonStep) => void; busy?: string | null; on?: (v: 'project_item_close') => boolean
}) {
  return (
    <div>
      <div className="simple-eyebrow mb-1">{title}{steps.length > 0 && <span className="ml-1.5 text-ink-faint">{steps.length}</span>}</div>
      {steps.length === 0 ? <p className="text-[12px] text-ink-faint">{empty}</p> : (
        <ul className="flex flex-col gap-0.5 text-[12px]">
          {steps.map((s) => (
            <li key={s.id} className="flex items-baseline gap-2">
              {onPick ? <button type="button" onClick={() => onPick(s.id)} className={cn('min-w-0 flex-1 truncate text-left hover:underline', s.state === 'done' ? 'text-ink-faint' : 'text-ink')}>{s.title}</button> : <span className="min-w-0 flex-1 truncate text-ink">{s.title}</span>}
              <span className={cn('num shrink-0 text-[11px]', s.overdue ? 'text-wait' : 'text-ink-faint')}>{s.due != null ? `${s.overdue ? 'was due ' : 'due '}${when(s.due, now)} · ` : ''}{s.word}</span>
              {close && on && s.item && <ActBtn verb="project_item_close" on={on as never} tone="ok" busy={busy === s.id} onClick={() => close(s)} title={`Mark “${s.title}” done`}>Done</ActBtn>}
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
