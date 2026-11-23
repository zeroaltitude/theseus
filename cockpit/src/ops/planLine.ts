// The plan line on a conversation anchor's Graph card: its n/m and its summary, for any conversation whose plan has
// work in it (issue 44: no longer only a thread's own one). A .ts module built with `createElement`, not JSX, so the
// tests render it as they run (`node --experimental-strip-types`, which reads no JSX).
import { createElement, type ReactElement } from 'react'
import { anchorPlan, type Progress } from './forms.ts'

/** The line, or nothing: a task, a step, or a conversation with an empty plan draws none. */
export function PlanLine({ kind, progress }: { kind: string; progress?: Progress }): ReactElement | null {
  const plan = anchorPlan(kind, progress)
  if (!plan) return null
  return createElement('div', { className: 'num mt-0.5 text-[10.5px] text-ink-dim', title: plan.summary },
    'plan ', createElement('span', { className: 'text-ok' }, `${plan.n}/${plan.m}`), ` · ${plan.summary}`)
}
