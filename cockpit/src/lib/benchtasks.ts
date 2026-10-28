// The per-test pop-out's pure half (theseus-1de57): a set's per-task points as table rows, and the one sentence that
// says where Theseus stands on a task. An auto-generated judgement that overstates is worse than a table, so the words
// are a fixed ladder, the raw counts always sit beside them, and no per-task row ever carries a percentage or a
// superlative. A comparison is always within ONE report (the same tasks and attempts), never across reports.
import { family, harness, property, type Point } from './bench.ts'

/** What one arm did on one task: solved k of n attempts. */
export interface Side { arm: string; name: string; k: number; n: number }

export type Assessment = { word: string; counts: string; against: string }

/** The ladder's words, pinned. */
export const WORDS = {
  few: 'too few attempts to say',
  none: 'no arm solved it',
  only: 'only Theseus solved it',
  level: 'level with the best arm here',
  ahead: 'ahead here',
  behind: 'behind here',
} as const

const of = (s: Side) => `${s.k} of ${s.n}`

/** Where Theseus stands on a task against the other arms' best, by the ladder: fewer than 2 attempts on either side;
 *  no arm solved it; only Theseus solved it; level; ahead; behind. `best` is the other arm with the highest rate. */
export function assess(theseus: Side | undefined, best: Side | undefined): Assessment {
  const against = best?.name ?? 'no other arm'
  const counts = theseus && best ? `${of(theseus)}, against ${of(best)}` : theseus ? `${of(theseus)}, against no other arm` : best ? `no Theseus attempts, against ${of(best)}` : 'no attempts'
  if (!theseus || !best || theseus.n < 2 || best.n < 2) return { word: WORDS.few, counts, against }
  if (theseus.k === 0 && best.k === 0) return { word: WORDS.none, counts, against }
  if (theseus.k > 0 && best.k === 0) return { word: WORDS.only, counts, against }
  // The same report gives every arm the same attempts; rates are compared by cross-multiplying, so unequal n stays honest.
  const d = theseus.k * best.n - best.k * theseus.n
  return { word: d === 0 ? WORDS.level : d > 0 ? WORDS.ahead : WORDS.behind, counts, against }
}

export interface TaskRow {
  task: string
  /** Every arm that ran the task, Theseus first. */
  sides: (Side & { usd?: number; wall?: number })[]
  theseus: (Side & { usd?: number; wall?: number }) | null
  best: Side | null
  assessment: Assessment
}

/** The arm that stands for Theseus in a report: the arm keyed `theseus`, else the first of its family.
 *
 *  The family, not `sameArm`, on purpose (theseus-yh48a): the table puts Theseus beside its rivals, so a listed ablation
 *  (`theseus-batching`) stays a side of its own, shown in its own cell, but is not a rival to rank against: "Theseus
 *  against Theseus + paragraph" is the question the arms chart asks, not this one. Under `sameArm` a set holding only the
 *  ablation and a rival would have no Theseus side at all. `benchtasks.test.ts` pins both halves. */
const pickTheseus = <T extends { arm: string }>(sides: T[]): T | undefined =>
  sides.find((s) => s.arm === 'theseus') ?? sides.find((s) => family(s.arm) === 'theseus')

/** The rows of a set's per-task points (`points(…, 'task')`), by task name. */
export function taskRows(ps: readonly Point[]): TaskRow[] {
  const by = new Map<string, Point[]>()
  for (const p of ps) if (p.task) by.set(p.task, [...(by.get(p.task) ?? []), p])
  return [...by].sort(([a], [b]) => a.localeCompare(b)).map(([task, group]) => {
    const sides = group.map((p) => {
      const v = p.values.pass
      return { arm: p.harness, name: harness(p.harness).name, k: v ? Math.round(v.v * v.n) : 0, n: v ? v.n : p.n, usd: p.values.usd?.v, wall: p.values['agent-s']?.v }
    })
    const theseus = pickTheseus(sides) ?? null
    // Rivals are the other harnesses: a listed ablation is a side, not a rival (see `pickTheseus`).
    const others = sides.filter((s) => s !== theseus && family(s.arm) !== 'theseus')
    const best = others.reduce<Side | null>((b, s) => (!b || s.k * b.n > b.k * s.n || (s.k * b.n === b.k * s.n && s.k > b.k) ? s : b), null)
    const ordered = theseus ? [theseus, ...sides.filter((s) => s !== theseus)] : sides
    return { task, sides: ordered, theseus, best, assessment: assess(theseus ?? undefined, best ?? undefined) }
  })
}

/** A cell's money and time in the properties' own words. */
export const usdWords = (v: number | undefined): string => (v === undefined ? 'not measured' : property('usd')!.format(v))
export const wallWords = (v: number | undefined): string => (v === undefined ? 'not measured' : property('agent-s')!.format(v))

/** Every string `assess` can return, for the test that holds the words to the rules. */
export function everyAssessment(): Assessment[] {
  const s = (k: number, n: number): Side => ({ arm: 'x', name: 'Arm', k, n })
  return [[undefined, s(1, 3)], [s(1, 1), s(1, 3)], [s(0, 3), s(0, 3)], [s(2, 3), s(0, 3)], [s(2, 3), s(2, 3)], [s(3, 3), s(2, 3)], [s(1, 3), s(2, 3)]]
    .map(([a, b]) => assess(a, b))
}

// ---------------------------------------------------------------- the task catalogue (theseus-hbh4m)

/** What the catalogue (`docs/benchmarks/tasks.json`, `bench/report/tasks.py`) says of one task. Every field is optional. */
export interface TaskInfo {
  title?: string; difficulty?: string; category?: string; tags?: string[]; expertMin?: number; juniorMin?: number
  timeoutS?: number; cpus?: number | string; memory?: string
  /** Upstream's prose: an excerpt, shown as one and attributed. */
  summary?: string
  /** Where the task lives upstream; written only from an explicit base, linked only when present. */
  source?: string
}
export type Catalogue = { tasks: Map<string, TaskInfo>; suite: string | null; reason: string | null }

const NONE = (reason: string): Catalogue => ({ tasks: new Map(), suite: null, reason })
const obj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)
const str = (v: unknown): string | undefined => (typeof v === 'string' && v !== '' ? v : undefined)
const nbr = (v: unknown): number | undefined => (typeof v === 'number' && Number.isFinite(v) ? v : undefined)

/** Read the catalogue's text the way the manifest is read: a file it cannot trust gives no descriptions and the reason,
 *  never a throw. A field of the wrong type is left out; the rest of its task stays. */
export function readCatalogue(text: string | null | undefined): Catalogue {
  if (!text) return NONE('the build embeds no task catalogue')
  let doc: unknown
  try { doc = JSON.parse(text) } catch { return NONE('the task catalogue is not valid JSON') }
  if (!obj(doc)) return NONE('the task catalogue is not an object')
  if (doc.version !== 1) return NONE(`the task catalogue is version ${String(doc.version)}; this build reads version 1`)
  if (!obj(doc.tasks)) return NONE('the task catalogue has no `tasks` table')
  const tasks = new Map<string, TaskInfo>()
  for (const [id, raw] of Object.entries(doc.tasks)) {
    if (!obj(raw)) continue
    const tags = Array.isArray(raw.tags) && raw.tags.every((t) => typeof t === 'string') ? (raw.tags as string[]) : undefined
    const source = str(raw.source)
    tasks.set(id, {
      title: str(raw.title), difficulty: str(raw.difficulty), category: str(raw.category), tags, expertMin: nbr(raw.expert_min),
      juniorMin: nbr(raw.junior_min), timeoutS: nbr(raw.timeout_s), cpus: nbr(raw.cpus) ?? str(raw.cpus), memory: str(raw.memory),
      summary: str(raw.summary)?.slice(0, 200), source: source && /^https?:\/\//.test(source) ? source : undefined,
    })
  }
  return { tasks, suite: str(doc.suite) ?? null, reason: null }
}
