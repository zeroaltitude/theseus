// The New task sheet's rules (theseus-jid7t.6), pure: the form's fields as `task.start`'s params, the budget's check,
// and the profile choices with the floor's greying by the home's data mark. A caller (the Timeline, for a sensitive
// calendar event) may pass a "US only" reason: then every profile of another origin is greyed with it.
import type { DataMark, ProfileInfo, TaskStartParams } from '@protocol'
import { greyOf } from './floor.ts'

export interface NewTaskForm {
  brief: string; title: string; budget: string; home: string; profile: string; startWhen: string; quotes: string
}
export const EMPTY_FORM: NewTaskForm = { brief: '', title: '', budget: '', home: '', profile: '', startWhen: '', quotes: '' }

/** The budget as dollars: blank is none (a quarter of what's left); anything but a positive number is bad. */
export function dollarsOf(budget: string): { usd?: number; bad: boolean } {
  const t = budget.trim()
  if (t === '') return { bad: false }
  const usd = Number(t.replace(/^\$/, ''))
  return usd > 0 ? { usd, bad: false } : { bad: true }
}

/** The quotes: one per blank-line-separated block. */
export const quotesOf = (text: string): string[] => text.split(/\n\s*\n/).map((q) => q.trim()).filter(Boolean)

/** `task.start`'s params from the form, or null while the brief is empty or the budget bad. */
export function startParams(f: NewTaskForm): TaskStartParams | null {
  const { usd, bad } = dollarsOf(f.budget)
  if (!f.brief.trim() || bad) return null
  const quotes = quotesOf(f.quotes)
  return {
    brief: f.brief.trim(),
    ...(f.title.trim() && { title: f.title.trim() }),
    ...(f.home && { home: f.home }),
    ...(usd !== undefined && { budget_usd: usd }),
    ...(f.profile && { profile: f.profile }),
    ...(f.startWhen.trim() && { start_when: f.startWhen.trim() }),
    ...(quotes.length && { quotes }),
  }
}

export interface Choice { name: string; label: string; grey: string | null }

/** The profiles a task may run on: each with the floor's reason when the home's data refuses it, or the caller's "US
 *  only" reason for a profile the daemon does not clear (ops-timeline: a sensitive calendar's event). Under "US only" the sheet
 *  picks one that is not greyed (the live one if it may, else the first), and none when none may: it fails closed.
 *  Otherwise it picks none (the parent's model). */
export function profileChoices(profiles: readonly ProfileInfo[], mark: DataMark | undefined, usOnly?: string, live?: string): { choices: Choice[]; pick: string } {
  const choices = profiles.map((p) => ({ name: p.name, label: `${p.name} · ${p.model}${p.origin ? ` · ${p.origin}` : ''}`, grey: (usOnly && p.cleared !== true ? usOnly : null) ?? greyOf(mark, p.name, p) }))
  if (!usOnly) return { choices, pick: '' }
  const ok = choices.filter((c) => !c.grey)
  return { choices, pick: (ok.find((c) => c.name === live) ?? ok[0])?.name ?? '' }
}

/** Under "US only" the sheet starts nothing unless the profile is one of the choices and not greyed: "the parent's
 *  model" (no profile) may be of any origin. */
export const usClosed = (choices: readonly Choice[], profile: string, usOnly?: string): boolean =>
  !!usOnly && !choices.some((c) => c.name === profile && !c.grey)
