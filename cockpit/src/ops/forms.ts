// The nouns' forms on the Graph and the Board (theseus-hl0r1): what each kind of row looks like, derived from the one
// work set (`boardRows`) alone. Pure: `npm test` runs it (test/ops-forms.test.ts). The rows are never re-derived here:
// a row's bucket, its plan flag and its parent are `board.ts`'s; this module only reads them.
//
//   thread        the lane; its conversation is the lane's head card (`graph.ts` names it, a conversation in its lane)
//   conversation  the navy anchor
//   plan          a folder-tab card with an n/m bar; a step with a child listed under it is one, in place
//   step          the work card with a checkbox glyph in its state colour
//   task          attached to a step or plan: a badge strip along its bottom, never a card of its own
//   job, ask, wake, routine, watch, project   as before
import type { OpsRow } from './board.ts'
import { progressLine } from '../lib/progressLine.ts'
import { bucketTone, type StateBucket } from './state.ts'

export type Form = 'anchor' | 'plan' | 'step' | 'work' | 'job' | 'ask' | 'future'

/** The eight states a step is drawn in. */
export type StepWord = 'todo' | 'doing' | 'blocked' | 'needs' | 'paused' | 'done' | 'abandoned' | 'failed'

export const STEP_GLYPH: Readonly<Record<StepWord, string>> = { todo: '☐', doing: '◐', blocked: '■', needs: '●', paused: '⏸', done: '☑', abandoned: '⊘', failed: '✗' }
export const STEP_WORDS: Readonly<Record<StepWord, string>> = { todo: 'to do', doing: 'doing', blocked: 'blocked', needs: 'needs you', paused: 'paused', done: 'done', abandoned: 'abandoned', failed: 'failed' }

/** A step's state word from its bucket (the one model) and its own state (a record's, or a view's). */
export function stepWordOf(bucket: StateBucket, state?: string): StepWord {
  if (state === 'failed') return 'failed'
  if (state === 'abandoned' || state === 'cancelled' || state === 'expired') return 'abandoned'
  if (state === 'done' || state === 'complete') return 'done'
  if (state === 'blocked') return 'blocked'
  switch (bucket) {
    case 'needs': return 'needs'
    case 'working': return 'doing'
    case 'paused': case 'scheduled': return 'paused'
    case 'ended': return 'done'
    default: return 'todo'
  }
}

/** A tone token: a state tone, or `ink-dim` for the quiet ones. Never a faded look. */
export type StepTone = 'wait' | 'live' | 'ok' | 'fault' | 'ink-dim'

/** A tone token's CSS colour (the page's tokens follow the mode). */
export const toneVar = (t: StepTone): string => `var(--color-${t})`

export interface StepLook { word: StepWord; glyph: string; tone: StepTone; faded: boolean }

/** A step's look: its glyph and its colour (`bucketTone`, else the word's own). A to-do step is ink-dim and not faded:
 *  only `ended` work is drawn faded. */
export function stepLook(bucket: StateBucket, state?: string): StepLook {
  const word = stepWordOf(bucket, state)
  const tone: StepTone = bucketTone(bucket, state ?? '') ?? (word === 'done' ? 'ok' : word === 'failed' ? 'fault' : word === 'blocked' ? 'wait' : 'ink-dim')
  return { word, glyph: STEP_GLYPH[word], tone, faded: bucket === 'ended' }
}

/** A plan's progress: `n` of `m` done, and the buckets of the rest. */
export interface Progress {
  n: number
  m: number
  /** The open work units by bucket, and the ended ones that did not finish (failed). */
  counts: Readonly<Record<StateBucket, number>>
  failed: number
  /** "1 working · 3 to do · 2 done": the folded plan's one line. */
  summary: string
  needs: number
}

const BUCKETS: readonly StateBucket[] = ['needs', 'working', 'scheduled', 'paused', 'todo', 'rest', 'ended']

/** The rows by every key a parent may name them by, as the Graph resolves a parent. */
interface Index { byRow: Map<string, OpsRow>; kids: Map<string, OpsRow[]> }

function indexOf(rows: readonly OpsRow[]): Index {
  const alias = new Map<string, string>()
  for (const r of rows) for (const k of [r.id, r.view?.id, r.recordId, r.task?.info?.execution_id]) if (k && !alias.has(k)) alias.set(k, r.id)
  const byRow = new Map(rows.map((r) => [r.id, r]))
  const kids = new Map<string, OpsRow[]>()
  for (const r of rows) {
    const p = r.parent ? alias.get(r.parent) : undefined
    if (p && p !== r.id) kids.set(p, [...(kids.get(p) ?? []), r])
  }
  return { byRow, kids }
}

/** A unit of work a plan counts: a step, or a task that carries out a record. A task with no record is not one. */
const isUnit = (r: OpsRow): boolean => r.kind === 'step' || (r.kind === 'task' && r.record != null)

function descendants(ix: Index, id: string): OpsRow[] {
  const seen = new Set<string>([id])
  const out: OpsRow[] = []
  const walk = (x: string) => {
    for (const k of ix.kids.get(x) ?? []) {
      if (seen.has(k.id)) continue
      seen.add(k.id)
      out.push(k)
      walk(k.id)
    }
  }
  walk(id)
  return out
}

const SUMMARY: ReadonlyArray<readonly [StateBucket, string]> = [['needs', 'needs you'], ['working', 'working'], ['paused', 'paused'], ['scheduled', 'scheduled'], ['todo', 'to do']]

function progressIn(ix: Index, id: string): Progress {
  const subtree = descendants(ix, id)
  const counts = Object.fromEntries(BUCKETS.map((b) => [b, 0])) as Record<StateBucket, number>
  let n = 0
  let failed = 0
  let abandoned = 0
  // Only the leaves count: a nested plan is its children's, once, never a unit of its own.
  for (const r of subtree) {
    if (!isUnit(r) || (ix.kids.get(r.id) ?? []).some(isUnit)) continue
    if (r.bucket === 'ended') {
      const w = stepWordOf(r.bucket, r.state)
      if (w === 'done') n++
      else if (w === 'failed') failed++
      else abandoned++
    } else counts[r.bucket]++
  }
  counts.ended = n + failed + abandoned
  // Abandoned work is no longer owed: it is out of the m.
  const m = n + failed + BUCKETS.filter((b) => b !== 'ended').reduce((s, b) => s + counts[b], 0)
  const parts = SUMMARY.filter(([b]) => counts[b] > 0).map(([b, w]) => `${counts[b]} ${w}`)
  if (failed) parts.push(`${failed} failed`)
  if (n) parts.push(`${n} done`)
  return { n, m, counts, failed, summary: parts.join(' · '), needs: counts.needs }
}

/** A plan row's progress, from the work set's rows alone: its *n of m* and the buckets of what is open, over its whole
 *  subtree, each leaf once. */
export function progressOf(rows: readonly OpsRow[], planId: string): Progress {
  return progressIn(indexOf(rows), planId)
}

/** The strip along the bottom of the step or plan a task carries out. */
export interface Badge {
  kind: 'task' | 'claim'
  /** The task's state glyph; a claim has none. */
  glyph: string
  word: string
  model?: string
  spent?: number
  limit?: number
  /** The task's live progress line (`progressLine`). */
  line?: string
  /** A claim's holder, "worked by ‹title›". */
  by?: string
  /** It runs: the card glows cyan. */
  running: boolean
  /** It asks: the card glows amber. */
  asks: boolean
}

const TASK_GLYPH: Readonly<Record<StateBucket, string>> = { needs: '●', working: '◐', paused: '⏸', scheduled: '◷', todo: '☐', rest: '○', ended: '✓' }

export interface RowForm {
  form: Form
  /** A plan's progress. */
  progress?: Progress
  /** A step or plan: the checkbox it wears. */
  look?: StepLook
  badge?: Badge
  /** A plan's direct children, for the Board's open card. */
  kids?: ReadonlyArray<{ id: string; title: string; look: StepLook }>
}

/** Every row's form. A row with a step listed under it is a plan: a step that gains a child is drawn as one at once,
 *  the same row, the same id (the owner's Q5: derived in place, nothing stored). */
export function formsOf(rows: readonly OpsRow[], now: number): Map<string, RowForm> {
  const ix = indexOf(rows)
  const out = new Map<string, RowForm>()
  for (const r of rows) {
    const units = (ix.kids.get(r.id) ?? []).filter(isUnit)
    switch (r.kind) {
      case 'conversation': out.set(r.id, { form: 'anchor', ...(units.length ? { progress: progressIn(ix, r.id) } : {}) }); break
      case 'job': out.set(r.id, { form: 'job' }); break
      case 'ask': out.set(r.id, { form: 'ask' }); break
      case 'wake': case 'routine': case 'watch': out.set(r.id, { form: 'future' }); break
      case 'task':
        if (!r.record) { out.set(r.id, { form: 'work' }); break }
        out.set(r.id, carried(r, units.length > 0 || r.plan === true, ix, now))
        break
      default: // a step
        out.set(r.id, carried(r, units.length > 0 || r.plan === true, ix, now))
    }
  }
  return out
}

function carried(r: OpsRow, plan: boolean, ix: Index, now: number): RowForm {
  const look = stepLook(r.bucket, r.record?.state ?? r.state)
  const form: RowForm = { form: plan ? 'plan' : 'step', look }
  if (plan) {
    form.progress = progressIn(ix, r.id)
    form.kids = (ix.kids.get(r.id) ?? []).filter(isUnit).map((k) => ({ id: k.id, title: k.title, look: stepLook(k.bucket, k.record?.state ?? k.state) }))
  }
  if (r.kind === 'task') {
    const info = r.task?.info
    const v = r.view
    const cost = v?.cost ?? (info ? { spent_usd: info.spent_usd, limit_usd: info.limit_usd } : undefined)
    form.badge = {
      kind: 'task', glyph: TASK_GLYPH[r.bucket], word: 'task', model: r.model, spent: cost?.spent_usd, limit: cost?.limit_usd,
      line: v && r.bucket !== 'ended' ? progressLine(v, now) : undefined, running: v?.state === 'running', asks: r.bucket === 'needs',
    }
  } else if (r.record?.claim) {
    const by = r.carrier ? ix.byRow.get(r.carrier) : undefined
    if (by) form.badge = { kind: 'claim', glyph: '', word: 'claim', by: by.title, running: by.view?.state === 'running', asks: false }
  }
  return form
}
