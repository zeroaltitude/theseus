// The ⌘K palette's entities and verbs (theseus-jid7t.6), pure. A verb is chosen, then its scope: the focused card, all
// that are visible, or the selection (`⇥` cycles); the targets come in as input (`OpsTarget[]`), so the Graph lens's
// selection joins through one seam. A target the verb would not touch is counted, with the floor's reason, and an entry
// no target takes is greyed: the palette hides nothing the daemon would allow and says why for what it would refuse.
import type { DataMark, ProfileInfo } from '@protocol'
import type { OpsRow } from './board.ts'
import { greyOf } from './floor.ts'
import { costHint } from './menus.ts'
import type { Price } from '../lib/money.ts'
import type { Kind } from './verbs.ts'

export type Scope = 'one' | 'visible' | 'selection'
export const SCOPES: readonly Scope[] = ['one', 'visible', 'selection']
export const SCOPE_WORDS: Readonly<Record<Scope, string>> = { one: 'the focused one', visible: 'all visible', selection: 'the selection' }
export const nextScope = (s: Scope): Scope => SCOPES[(SCOPES.indexOf(s) + 1) % SCOPES.length]

/** What a verb needs to know of a card. */
export interface OpsTarget {
  id: string
  kind: Kind
  title: string
  sessionId?: string
  profile?: string
  data?: DataMark
  paused: boolean
  /** A turn runs on it. */
  running: boolean
  /** It has ended or failed: a retry or a dismiss. */
  failed: boolean
  ended: boolean
  executionId?: string
}

export function targetOf(r: OpsRow): OpsTarget {
  const kind: Kind = r.kind === 'conversation' ? 'conversation' : r.kind === 'task' ? 'task' : r.kind === 'job' ? 'job' : r.kind === 'wake' ? 'wake' : 'routine'
  const state = r.view?.state ?? r.state ?? ''
  return {
    id: r.id, kind, title: r.title, sessionId: r.sessionId, profile: r.model, data: r.data, paused: r.paused,
    running: state === 'running' || state === 'waiting' || r.session?.execution_state === 'running',
    failed: state === 'failed' || r.task?.info?.state === 'failed', ended: r.column === 'done',
    executionId: r.kind === 'conversation' ? r.view?.id ?? r.session?.execution_id : undefined,
  }
}

/** The targets a scope names. The selection arrives from the Graph lens's hook at the join. */
export function scopeTargets(scope: Scope, from: { focused: OpsTarget | undefined; visible: readonly OpsTarget[]; selection: readonly OpsTarget[] }): OpsTarget[] {
  return scope === 'one' ? (from.focused ? [from.focused] : []) : scope === 'visible' ? [...from.visible] : [...from.selection]
}

/** One call a verb makes: the same shape `useOpsAct().act` takes. */
export interface Plan { verb: string; key: string; method: string; params: unknown; ask?: string }

export interface Entry {
  id: string
  label: string
  /** What the entry would do and what it would skip, in words: "2 targets · 1 already on it". */
  detail: string
  /** Set when no target takes it: the reason, in the floor's or the daemon's words. */
  grey?: string
  plans: Plan[]
  /** Opens a sheet instead (New task). */
  opens?: 'new_task'
}

export interface Ctx { profiles: readonly ProfileInfo[]; prices: ReadonlyMap<string, Price> }

const n = (k: number, one: string, many = `${one}s`) => `${k} ${k === 1 ? one : many}`

function entry(id: string, label: string, takes: Array<{ t: OpsTarget; plan: Plan }>, skipped: Array<{ t: OpsTarget; why: string }>, extra = ''): Entry {
  const why = new Map<string, number>()
  for (const s of skipped) why.set(s.why, (why.get(s.why) ?? 0) + 1)
  const skip = [...why].map(([w, c]) => `${c} ${w}`).join(' · ')
  const detail = [takes.length ? n(takes.length, 'target') : '', skip, extra].filter(Boolean).join(' · ')
  return { id, label, detail, plans: takes.map((x) => x.plan), grey: takes.length ? undefined : skipped[0]?.why ?? 'nothing here takes it' }
}

/** Every verb the palette offers over `targets`, each entry with the plans it would make and the targets it skips. */
export function verbEntries(targets: readonly OpsTarget[], cx: Ctx): Entry[] {
  const out: Entry[] = []
  if (!targets.length) return [{ id: 'new_task', label: 'Start a task', detail: 'anywhere', plans: [], opens: 'new_task' }]
  const tasks = targets.filter((t) => t.kind === 'task')
  const pausers = tasks.filter((t) => !t.ended)
  out.push(entry('pause', 'Pause', pausers.filter((t) => !t.paused).map((t) => ({ t, plan: { verb: 'pause', key: t.id, method: 'task.pause', params: { task: t.id } } })),
    targets.map((t) => ({ t, why: t.kind !== 'task' ? 'not a task' : t.ended ? 'ended' : 'already paused' })).filter((s) => !pausers.some((t) => t.id === s.t.id && !t.paused))))
  out.push(entry('resume', 'Resume', pausers.filter((t) => t.paused).map((t) => ({ t, plan: { verb: 'pause', key: t.id, method: 'task.resume', params: { task: t.id } } })),
    targets.map((t) => ({ t, why: t.kind !== 'task' ? 'not a task' : t.paused ? '' : 'not paused' })).filter((s) => s.why)))
  out.push(entry('cancel', 'Cancel', pausers.map((t) => ({ t, plan: { verb: 'cancel', key: `${t.id}:cancel`, method: 'task.cancel', params: { task: t.id }, ask: `Cancel ${n(pausers.length, 'task')}? Their jobs stop and each reports that it was cancelled.` } })),
    targets.filter((t) => !pausers.includes(t)).map((t) => ({ t, why: t.kind !== 'task' ? 'not a task' : 'ended' }))))
  const stoppers = targets.filter((t) => t.executionId && t.running)
  out.push(entry('stop', 'Stop the running turn', stoppers.map((t) => ({ t, plan: { verb: 'stop', key: `${t.executionId}:stop`, method: 'execution.stop', params: { execution_id: t.executionId } } })),
    targets.filter((t) => !stoppers.includes(t)).map((t) => ({ t, why: 'no turn running' }))))
  const failed = targets.filter((t) => t.failed)
  out.push(entry('retry', 'Retry', failed.filter((t) => t.kind === 'task').map((t) => ({ t, plan: { verb: 'retry', key: `${t.id}:retry`, method: 'task.retry', params: { task: t.id } } })),
    targets.filter((t) => !(t.failed && t.kind === 'task')).map((t) => ({ t, why: 'not failed' }))))
  // Switch model: one entry per profile, the floor deciding each target.
  const sessions = targets.filter((t) => t.sessionId && (t.kind === 'conversation' || t.kind === 'task'))
  const cur = cx.profiles.find((p) => p.name === sessions[0]?.profile)
  for (const p of cx.profiles) {
    const takes: Array<{ t: OpsTarget; plan: Plan }> = []
    const skipped: Array<{ t: OpsTarget; why: string }> = []
    for (const t of targets) {
      if (!t.sessionId || (t.kind !== 'conversation' && t.kind !== 'task')) skipped.push({ t, why: 'not a session' })
      else if (t.profile === p.name) skipped.push({ t, why: 'already on it' })
      else if (greyOf(t.data, p.name, p)) skipped.push({ t, why: `hold ${t.data} data` })
      else takes.push({ t, plan: { verb: 'model_menu', key: `${t.sessionId}:model`, method: 'session.profile', params: { session_id: t.sessionId, profile: p.name } } })
    }
    const hint = costHint(cx.prices.get(p.model), cx.prices.get(cur?.model ?? ''))
    out.push(entry(`model:${p.name}`, `Switch model to ${p.name}`, takes, skipped, hint))
  }
  for (const m of ['ask', 'edits', 'plan', 'auto'] as const) {
    const ok = sessions.filter((t) => t.kind === 'conversation')
    out.push(entry(`mode:${m}`, `Mode: ${m}`, ok.map((t) => ({ t, plan: { verb: 'mode_menu', key: `${t.sessionId}:mode`, method: 'session.mode', params: { session_id: t.sessionId, mode: m }, ask: m === 'auto' ? 'Set Auto? It then acts without asking (the floor still asks).' : undefined } })),
      targets.filter((t) => !ok.includes(t)).map((t) => ({ t, why: 'not a conversation' }))))
  }
  out.push({ id: 'new_task', label: 'Start a task', detail: 'its home the focused conversation', plans: [], opens: 'new_task' })
  return out
}

/** The grammar: a verb then its target ("pause tides"), or a target then its verb. cmdk matches a value by its words in
 *  any order, so one value holds both. */
export const searchValue = (verb: string, target: string): string => `${verb} ${target} ${target} ${verb}`.toLowerCase()
