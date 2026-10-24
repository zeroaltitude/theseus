// The ontology view's pure parts (theseus-8kk.2): the category tree's order and depth, and what a session's memberships
// and guidance say that its newest compilation's manifest does not yet. Pure, with no import but the protocol's types,
// so `node --test` runs its test (`cockpit/test/ontology.test.ts`) as it is.
import type { LedgerEntry, OntologyCategory, OntologyKind, OntologyMembership, PersonaAddParams, PersonaInfo, PersonaListResult } from '@protocol'
import { localDay } from './learning.ts'

export interface TreeRow { category: OntologyCategory; depth: number }

/** The categories depth first, as the daemon lists them: the roots by name, each followed by its children by name. The
 *  depth is counted here, from the parents, so a list in any order reads the same. A category whose parent is not in
 *  the list stands as a root. */
export function treeOrder(categories: readonly OntologyCategory[]): TreeRow[] {
  const ids = new Set(categories.map((c) => c.id))
  const kids = new Map<string | null, OntologyCategory[]>()
  for (const c of categories) {
    const key = c.parent && ids.has(c.parent) ? c.parent : null
    const list = kids.get(key)
    if (list) list.push(c)
    else kids.set(key, [c])
  }
  const byName = (a: OntologyCategory, b: OntologyCategory) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id)
  const out: TreeRow[] = []
  const seen = new Set<string>()
  const walk = (parent: string | null, depth: number) => {
    for (const c of (kids.get(parent) ?? []).sort(byName)) {
      if (seen.has(c.id)) continue
      seen.add(c.id)
      out.push({ category: c, depth })
      walk(c.id, depth + 1)
    }
  }
  walk(null, 1)
  return out
}

/** The label of the group that holds a category whose kind the kinds table does not name (or names not at all). */
export const UNKNOWN_GROUP = '<unknown>'

export interface KindGroup {
  /** A stable key: `kind:<name>`, or `kind:<unknown>`. */
  key: string
  /** What its header says: the kind's name, or `<unknown>`. */
  label: string
  /** The group's categories as the tree reads them (`treeOrder`), depths counted within the group. */
  rows: TreeRow[]
  /** How many categories it holds. */
  count: number
}

/** The categories as closed groups, one a kind (theseus-7mow): the kinds in the kinds table's order, then `<unknown>`
 *  for those whose kind it does not name; a group with nothing in it is left out. Every category is in exactly one
 *  group, so nothing is lost by folding them. */
export function groupsOf(categories: readonly OntologyCategory[], kinds: readonly { name: string }[]): KindGroup[] {
  const known = new Set(kinds.map((k) => k.name))
  const by = new Map<string, OntologyCategory[]>()
  for (const c of categories) {
    const key = c.kind && known.has(c.kind) ? c.kind : UNKNOWN_GROUP
    const list = by.get(key)
    if (list) list.push(c)
    else by.set(key, [c])
  }
  const order = [...kinds.map((k) => k.name), UNKNOWN_GROUP]
  return order.filter((k, i) => order.indexOf(k) === i && by.has(k)).map((k) => ({
    key: `kind:${k}`, label: k, rows: treeOrder(by.get(k)!), count: by.get(k)!.length,
  }))
}

/** The key of the group that holds a category, to open it when the address selects one. */
export const groupKeyOf = (groups: readonly KindGroup[], id: string | null): string | null =>
  (id && groups.find((g) => g.rows.some((r) => r.category.id === id))?.key) || null

/** Past this many categories the tree starts folded at its roots: an import's topics (theseus-anh3) are well over a
 *  hundred, and a reader opens the roots they want. */
export const FOLD_AT = 40

/** Each category's sessions with its descendants': its own `members` and every descendant's, summed (a session in a
 *  topic and in its child counts twice, as the two memberships it holds). */
export function withinCounts(rows: readonly TreeRow[]): Map<string, number> {
  const out = new Map<string, number>()
  // Depth first, so each row's descendants follow it: add each row to itself and to every open ancestor on the path.
  const path: TreeRow[] = []
  for (const r of rows) {
    while (path.length && path[path.length - 1].depth >= r.depth) path.pop()
    const n = r.category.members ?? 0
    out.set(r.category.id, n)
    for (const a of path) out.set(a.category.id, (out.get(a.category.id) ?? 0) + n)
    path.push(r)
  }
  return out
}

/** The ids of the rows that have children. */
export function parents(rows: readonly TreeRow[]): Set<string> {
  const out = new Set<string>()
  rows.forEach((r, i) => { if (rows[i + 1] && rows[i + 1].depth > r.depth) out.add(r.category.id) })
  return out
}

/** The rows a reader sees: a row shows when every ancestor of it is open. `open` holds the open rows' ids. */
export function shown(rows: readonly TreeRow[], open: ReadonlySet<string>): TreeRow[] {
  const out: TreeRow[] = []
  let hideBelow = Infinity
  for (const r of rows) {
    if (r.depth > hideBelow) continue
    hideBelow = Infinity
    out.push(r)
    if (!open.has(r.category.id)) hideBelow = r.depth
  }
  return out
}

/** The ids a category's ancestors have, the root first: opened so a selected category shows. */
export function ancestorsOf(rows: readonly TreeRow[], id: string | null): string[] {
  if (!id) return []
  const by = new Map(rows.map((r) => [r.category.id, r.category]))
  const out: string[] = []
  let c = by.get(id)
  for (let hops = 0; c?.parent && by.has(c.parent) && hops < 64; hops++) {
    out.unshift(c.parent)
    c = by.get(c.parent)
  }
  return out
}

/** What a compilation's manifest recorded of one membership (`MembershipUsed`). Read defensively: the manifest is
 *  untyped on the wire, and an older compilation has neither field. */
export interface UsedMembership { category: string; origin: string }
/** What it recorded of one category's guidance (`GuidanceUsed`). */
export interface UsedGuidance { category: string; version: number; digest: string }

export interface Recorded { memberships: UsedMembership[]; guidance: UsedGuidance[]; known: boolean }

/** The `memberships` and `guidance` of a manifest, whatever it holds. The daemon leaves an empty list out of the
 *  manifest, so a field that is absent reads as none recorded; `known` says whether either was there at all. */
export function recordedOf(manifest: unknown): Recorded {
  const m = (manifest && typeof manifest === 'object' ? manifest : {}) as Record<string, unknown>
  const arr = (v: unknown): Record<string, unknown>[] => (Array.isArray(v) ? v.filter((x) => x && typeof x === 'object') : [])
  const memberships = arr(m.memberships)
    .filter((x) => typeof x.category === 'string')
    .map((x) => ({ category: String(x.category), origin: typeof x.origin === 'string' ? x.origin : '' }))
  const guidance = arr(m.guidance)
    .filter((x) => typeof x.category === 'string')
    .map((x) => ({ category: String(x.category), version: Number(x.version) || 0, digest: typeof x.digest === 'string' ? x.digest : '' }))
  return { memberships, guidance, known: Array.isArray(m.memberships) || Array.isArray(m.guidance) }
}

export interface GuidanceChange { category: string; from: UsedGuidance | null; to: UsedGuidance | null }

export interface Pending {
  added: string[]
  removed: string[]
  guidance: GuidanceChange[]
}

export const nothingPending = (p: Pending) => !p.added.length && !p.removed.length && !p.guidance.length

/** The guidance a compile would admit for a list of memberships: each membership's category and, under a kind whose
 *  rule is `chain`, every ancestor of it, where the category has guidance. The ids sorted. */
export function guidanceInPlay(
  memberships: readonly Pick<OntologyMembership, 'category'>[],
  categories: readonly OntologyCategory[],
  kinds: readonly OntologyKind[],
): UsedGuidance[] {
  const by = new Map(categories.map((c) => [c.id, c]))
  const rule = new Map(kinds.map((k) => [k.name, k.rule]))
  const play = new Set<string>()
  for (const m of memberships) {
    let c = by.get(m.category)
    if (!c) continue
    play.add(c.id)
    if (rule.get(c.kind) !== 'chain') continue
    for (let hops = 0; c?.parent && hops < 64; hops++) {
      c = by.get(c.parent)
      if (c) play.add(c.id)
    }
  }
  const out: UsedGuidance[] = []
  for (const id of [...play].sort()) {
    const g = by.get(id)?.guidance
    if (g && g.text !== '') out.push({ category: id, version: g.version, digest: g.digest })
  }
  return out
}

/** What changed since the newest compilation, for one session: the memberships it holds now (the list) against the ones
 *  its manifest recorded, and the guidance those would admit now against the guidance it recorded. A membership is
 *  its category and its origin; its as-of time is not compared (a place's given ones are read afresh at each
 *  compile). A manifest that records neither field recorded none: the daemon leaves empty lists out. */
export function pendingOf(
  memberships: readonly OntologyMembership[],
  categories: readonly OntologyCategory[],
  kinds: readonly OntologyKind[],
  manifest: unknown,
): Pending {
  const rec = recordedOf(manifest)
  const key = (c: string, o: string) => `${c}\u0000${o}`
  const had = new Set(rec.memberships.map((m) => key(m.category, m.origin)))
  const has = new Set(memberships.map((m) => key(m.category, m.origin)))
  const added = memberships.filter((m) => !had.has(key(m.category, m.origin))).map((m) => m.category)
  const removed = rec.memberships.filter((m) => !has.has(key(m.category, m.origin))).map((m) => m.category)
  const now = new Map(guidanceInPlay(memberships, categories, kinds).map((g) => [g.category, g]))
  const was = new Map(rec.guidance.map((g) => [g.category, g]))
  const guidance: GuidanceChange[] = []
  for (const id of [...new Set([...now.keys(), ...was.keys()])].sort()) {
    const a = was.get(id) ?? null
    const b = now.get(id) ?? null
    if (a?.version !== b?.version || a?.digest !== b?.digest) guidance.push({ category: id, from: a, to: b })
  }
  return { added: [...new Set(added)].sort(), removed: [...new Set(removed)].sort(), guidance }
}

/** One line for a guidance change: `topic:harbor guidance v1 → v2`, `… added (v1)`, `… taken away`. */
export function guidanceWords(g: GuidanceChange): string {
  if (!g.from && g.to) return `${g.category} guidance added (v${g.to.version})`
  if (g.from && !g.to) return `${g.category} guidance no longer applies (was v${g.from.version})`
  return `${g.category} guidance v${g.from?.version} → v${g.to?.version}`
}

/** What a refused write says: the daemon's own words, whatever the error is. */
export function refusalWords(e: unknown): string {
  const m = (e as { message?: unknown } | null)?.message
  return typeof m === 'string' && m ? m : String(e)
}

// ---------------------------------------------------------------- growth (theseus-dd3kw, R7)

/** The origins that write through an undoable record: Jev, the sweep, and the dream. Their ledger rows carry `undo`. */
export const MACHINE_ORIGINS: readonly string[] = ['jev', 'sweep', 'dream']

/** The origins in the order a chart stacks them (the owner's own first, the machines last); one not named here is stacked
 *  after these, by name. */
export const ORIGIN_ORDER: readonly string[] = ['operator', 'transport', 'import', 'jev', 'sweep', 'dream']

/** The window of the growth chart, in days. */
export const GROWTH_DAYS = 90

type D = Record<string, unknown>
const obj = (v: unknown): D => (v && typeof v === 'object' && !Array.isArray(v) ? (v as D) : {})
const str = (v: unknown): string | null => (typeof v === 'string' ? v : null)

/** The `undo` record a machine origin's ledger row carries: the write's id, the key, the record as written, and as it
 *  was (`null` when it was new). `null` when the row has none. */
function undoOf(data: D): { write: string; key: string; wrote: D; before: unknown } | null {
  const u = obj(data.undo)
  const write = str(u.write)
  if (write === null) return null
  return { write, key: str(u.key) ?? '', wrote: obj(u.wrote), before: u.before ?? null }
}

/** The writes the operator took back: the `write` of each `ontology.undo` row, each with the time of its first undo. */
export function undoneWrites(rows: readonly LedgerEntry[]): Map<string, number> {
  const out = new Map<string, number>()
  for (const r of rows) {
    if (r.kind !== 'ontology.undo') continue
    const w = str(obj(r.data).write)
    if (w !== null && !out.has(w)) out.set(w, r.at_unix_ms)
  }
  return out
}

export interface GrowthDay {
  /** The local date, `2026-10-04`. */
  day: string
  /** Categories made that day, by the origin that made them. */
  categories: Record<string, number>
  /** Kinds added that day. */
  kinds: number
  /** Of the categories made that day, how many a later undo took back (their own row stays; the mark says so). */
  undone: number
}

export interface Growth {
  /** `GROWTH_DAYS` days, oldest first, the last one today. */
  days: GrowthDay[]
  /** The origins seen in the window, in stacking order. */
  origins: string[]
  /** Categories made in the window, and of those, how many were taken back. */
  made: number
  undone: number
  /** Kinds added in the window. */
  kinds: number
}

/** The local dates of the `n` days that end at `now`'s, oldest first. Built from the calendar (not by 24-hour steps), so
 *  a day that is 23 or 25 hours long is still one day. */
export function lastDays(now: Date, n: number): string[] {
  const out: string[] = []
  for (let i = n - 1; i >= 0; i--) out.push(localDay(new Date(now.getFullYear(), now.getMonth(), now.getDate() - i)))
  return out
}

/** The ledger's `ontology.category` and `ontology.kind` rows folded by local day over the window. A category counts once,
 *  on the day of its first row (a later row of the same id is a change, not a new category, and a category made before the
 *  window is not counted in it); a kind counts on its first row that does not retire it. A category whose write was
 *  undone is counted all the same, and marked in `undone` on its own day: the chart shows what was written and what was
 *  taken back, not only what is left. */
export function growthOf(rows: readonly LedgerEntry[], now: Date, days = GROWTH_DAYS): Growth {
  const keys = lastDays(now, days)
  const by = new Map(keys.map((d) => [d, { day: d, categories: {} as Record<string, number>, kinds: 0, undone: 0 } as GrowthDay]))
  const undone = undoneWrites(rows)
  const seenCategory = new Set<string>()
  const seenKind = new Set<string>()
  const origins = new Set<string>()
  let made = 0, took = 0, kinds = 0
  for (const r of rows) {
    if (r.kind !== 'ontology.category' && r.kind !== 'ontology.kind') continue
    const d = obj(r.data)
    const slot = by.get(localDay(new Date(r.at_unix_ms)))
    if (r.kind === 'ontology.category') {
      const id = str(d.id)
      if (id === null || seenCategory.has(id)) continue
      seenCategory.add(id)
      if (!slot) continue
      const origin = str(d.origin) ?? 'unknown'
      origins.add(origin)
      slot.categories[origin] = (slot.categories[origin] ?? 0) + 1
      made++
      const w = undoOf(d)?.write
      if (w !== undefined && undone.has(w)) { slot.undone++; took++ }
    } else {
      const name = str(d.name)
      if (name === null || d.retired === true || seenKind.has(name)) continue
      seenKind.add(name)
      if (!slot) continue
      slot.kinds++
      kinds++
    }
  }
  const rank = (o: string) => { const i = ORIGIN_ORDER.indexOf(o); return i < 0 ? ORIGIN_ORDER.length : i }
  const ordered = [...origins].sort((a, b) => rank(a) - rank(b) || a.localeCompare(b))
  return { days: keys.map((k) => by.get(k)!), origins: ordered, made, undone: took, kinds }
}

export interface GrowthSeries {
  days: string[]
  /** One column per origin, stacked, a count a day. */
  stacks: { origin: string; values: number[] }[]
  kinds: number[]
  undone: number[]
  /** The tallest day's categories, for the axis. */
  max: number
}

/** The growth as a chart reads it: columns by origin, and the lines for kinds and for undone writes. */
export function seriesOf(g: Growth): GrowthSeries {
  const stacks = g.origins.map((origin) => ({ origin, values: g.days.map((d) => d.categories[origin] ?? 0) }))
  const max = Math.max(0, ...g.days.map((d) => Object.values(d.categories).reduce((a, b) => a + b, 0)))
  return { days: g.days.map((d) => d.day), stacks, kinds: g.days.map((d) => d.kinds), undone: g.days.map((d) => d.undone), max }
}

/** The words under the chart: what the window holds. */
export function growthWords(g: Growth): string {
  const n = (k: number, one: string, many: string) => `${k.toLocaleString()} ${k === 1 ? one : many}`
  const undone = g.undone > 0 ? `, ${g.undone.toLocaleString()} taken back` : ''
  return `${n(g.made, 'category', 'categories')} made${undone} and ${n(g.kinds, 'kind', 'kinds')} added in ${g.days.length} days`
}

/** The sentence a sweep-minted kind's description ends with when Jev's pick named it: `NAMED_NOTE` in the core's `mint.rs`. */
export const NAMED_NOTE = 'Jev picked its name.'

export interface Minted {
  /** The write's id (a judgment's, for Jev's): what `ontology.undo` takes. One write may mint several categories. */
  write: string
  category: string
  name: string
  kind: string
  parent: string | null
  origin: string
  /** When the category row was written, ms. */
  at: number
  /** The session whose memberships the write set, when the ledger holds that row. */
  session: string | null
  /** The origin's confidence in the membership it set, when the ledger holds it. */
  confidence: number | null
  undone: boolean
  /** When the write was taken back, ms. */
  undoneAt: number | null
  /** How many categories this write minted: an undo takes all of them back. */
  siblings: number
  /** The write's kind was named by Jev's pick (its description ends with `NAMED_NOTE`). */
  named: boolean
}

/** The membership rows of each machine write: the session whose list it set, and the confidence it gave each category. */
function writeLists(rows: readonly LedgerEntry[]): Map<string, { session: string | null; confidence: Map<string, number> }> {
  const lists = new Map<string, { session: string | null; confidence: Map<string, number> }>()
  for (const r of rows) {
    if (r.kind !== 'ontology.membership') continue
    const u = undoOf(obj(r.data))
    if (!u) continue
    const wrote = u.wrote
    const slot = lists.get(u.write) ?? { session: null, confidence: new Map<string, number>() }
    slot.session ??= str(wrote.session) ?? r.session_id
    for (const m of Array.isArray(wrote.members) ? wrote.members : []) {
      const mo = obj(m)
      const c = str(mo.category)
      if (c !== null && typeof mo.confidence === 'number') slot.confidence.set(c, mo.confidence)
    }
    lists.set(u.write, slot)
  }
  return lists
}

/** What a machine origin minted, newest first: the `ontology.category` rows that carry an `undo` whose `before` is null
 *  (the category was new), each with its session and confidence from the write's membership row, and whether the write
 *  was taken back. Rows of an operator or the transport carry no `undo` and are not in it. */
export function mintedOf(rows: readonly LedgerEntry[], origins: readonly string[] = MACHINE_ORIGINS): Minted[] {
  const undone = undoneWrites(rows)
  const lists = writeLists(rows)
  const named = new Set<string>()
  for (const r of rows) {
    const u = r.kind === 'ontology.kind' ? undoOf(obj(r.data)) : null
    if (u && str(u.wrote.description)?.endsWith(NAMED_NOTE)) named.add(u.write)
  }
  const out: Minted[] = []
  const seen = new Set<string>()
  for (const r of rows) {
    if (r.kind !== 'ontology.category') continue
    const d = obj(r.data)
    const origin = str(d.origin)
    const u = undoOf(d)
    const id = str(d.id)
    if (!u || id === null || origin === null || !origins.includes(origin) || u.before !== null || seen.has(`${u.write}\u0000${id}`)) continue
    seen.add(`${u.write}\u0000${id}`)
    const list = lists.get(u.write)
    out.push({
      write: u.write, category: id, name: str(d.name) ?? id, kind: str(d.kind) ?? '', parent: str(d.parent), origin,
      at: r.at_unix_ms, session: list?.session ?? r.session_id, confidence: list?.confidence.get(id) ?? null,
      undone: undone.has(u.write), undoneAt: undone.get(u.write) ?? null, siblings: 0, named: named.has(u.write),
    })
  }
  const per = new Map<string, number>()
  for (const m of out) per.set(m.write, (per.get(m.write) ?? 0) + 1)
  for (const m of out) m.siblings = per.get(m.write) ?? 1
  return out.sort((a, b) => b.at - a.at || a.category.localeCompare(b.category))
}

export interface Renamed {
  write: string
  category: string
  from: string
  to: string
  /** When the category row was written, ms. */
  at: number
  undone: boolean
}

/** What the sweep renamed, newest first: the `ontology.category` rows of a `-rename` write, whose `before` held another name. */
export function renamedOf(rows: readonly LedgerEntry[]): Renamed[] {
  const undone = undoneWrites(rows)
  const out: Renamed[] = []
  for (const r of rows) {
    if (r.kind !== 'ontology.category') continue
    const u = undoOf(obj(r.data))
    const from = u ? str(obj(u.before).name) : null
    const to = u ? str(u.wrote.name) : null
    if (!u || !u.write.endsWith('-rename') || from === null || to === null) continue
    out.push({ write: u.write, category: str(obj(r.data).id) ?? '', from, to, at: r.at_unix_ms, undone: undone.has(u.write) })
  }
  return out.sort((a, b) => b.at - a.at)
}

/** The address of the judgment that made a write (the Judgment view reads `?id=`). */
export const judgmentHref = (write: string): string => `/judgment?id=${encodeURIComponent(write)}`

/** The confirm for an undo, in the words of what it sends: the write's id, and what it takes back. */
export function undoAsk(m: Minted): string {
  const what = m.siblings > 1 ? `all ${m.siblings} categories that write minted, ${m.name} among them` : `the category ${m.name} (${m.category})`
  return `Undo the write ${m.write} (ontology.undo)?\n\nIt takes back ${what}, and puts the earlier member lists back as they were. It is labelled wrong in the learning ledger. A category another session now uses is kept, and the undo is refused.`
}

/** A confidence as the list shows it: `0.82`, or a dash when the ledger holds none. */
export const confidenceWords = (c: number | null): string => (c === null ? '—' : c.toFixed(2))

// ---------------------------------------------------------------- origin filter (theseus-tq7s8, R7b)

/** What a category with no `ontology.category` row in the ledger is filed under (a seed's, or a row older than the history
 *  the page holds): the filter has it as an option, so no category falls out of every filter. */
export const UNKNOWN_ORIGIN = 'unknown'

export interface CategoryOrigin {
  /** The origin on the category's first ledger row: later rows of the same id are changes, not a new maker. */
  origin: string
  /** The origin's confidence in the membership set by the write that minted it, when the ledger holds it. */
  confidence: number | null
}

/** Each category's origin and confidence from the ledger: the origin of its first `ontology.category` row (`added_by` is free
 *  text; the row's `origin` is the word), and the confidence the membership row of its minting write gave it (a machine's
 *  write only: an operator's category has none). A category with no row is not in the map. */
export function originsOf(rows: readonly LedgerEntry[]): Map<string, CategoryOrigin> {
  const lists = writeLists(rows)
  const out = new Map<string, CategoryOrigin>()
  for (const r of rows) {
    if (r.kind !== 'ontology.category') continue
    const d = obj(r.data)
    const id = str(d.id)
    if (id === null || out.has(id)) continue
    const write = undoOf(d)?.write
    out.set(id, { origin: str(d.origin) ?? UNKNOWN_ORIGIN, confidence: (write !== undefined ? lists.get(write)?.confidence.get(id) : undefined) ?? null })
  }
  return out
}

/** A category's origin, `unknown` when the ledger has no row for it. */
export const originOf = (m: ReadonlyMap<string, CategoryOrigin>, id: string): CategoryOrigin =>
  m.get(id) ?? { origin: UNKNOWN_ORIGIN, confidence: null }

/** A tree row as the filtered tree draws it: `context` is a row kept only because something below it matches. */
export interface FilteredRow extends TreeRow { context: boolean }

/** The rows a filter by origin keeps: every row whose origin is wanted, and every ancestor of one (marked `context`, so a
 *  match below it still shows where it hangs). Nothing wanted keeps every row, none of them context. The rows stay depth
 *  first, as `treeOrder` made them. */
export function originFilter(rows: readonly TreeRow[], m: ReadonlyMap<string, CategoryOrigin>, wanted: ReadonlySet<string>): FilteredRow[] {
  if (wanted.size === 0) return rows.map((r) => ({ ...r, context: false }))
  const match = rows.map((r) => wanted.has(originOf(m, r.category.id).origin))
  const keep = [...match]
  // Depth first: walking back from a match, each row of lesser depth than the last one taken is its next ancestor up.
  rows.forEach((r, i) => {
    if (!match[i]) return
    let depth = r.depth
    for (let j = i - 1; j >= 0 && depth > 1; j--) {
      if (rows[j].depth < depth) { keep[j] = true; depth = rows[j].depth }
    }
  })
  return rows.flatMap((r, i) => (keep[i] ? [{ ...r, context: !match[i] }] : []))
}

/** How many of the rows each origin made, for the filter's chips; every row is in one. */
export function originCounts(rows: readonly TreeRow[], m: ReadonlyMap<string, CategoryOrigin>): Map<string, number> {
  const out = new Map<string, number>()
  for (const r of rows) {
    const o = originOf(m, r.category.id).origin
    out.set(o, (out.get(o) ?? 0) + 1)
  }
  return out
}

/** The origins the filter offers, in the chart's order, then `unknown`. */
export const ORIGIN_CHOICES: readonly string[] = [...ORIGIN_ORDER, UNKNOWN_ORIGIN]

/** `?origin=jev,sweep` as a set: the words that name an origin the filter offers, whatever else the address holds. */
export function parseOriginParam(v: string | null): Set<string> {
  return new Set((v ?? '').split(',').map((w) => w.trim()).filter((w) => ORIGIN_CHOICES.includes(w)))
}

/** A set of origins as the address holds it, in the chart's order; `null` (no parameter) when none is picked. */
export function originParam(s: ReadonlySet<string>): string | null {
  const words = ORIGIN_CHOICES.filter((o) => s.has(o))
  return words.length ? words.join(',') : null
}

// Personas (theseus-utfnl): a `persona:<key>` category each, its soul one small file. The view reads `persona.list`.

/** A persona's soul as the list shows it: live, awaiting its soul, or live but over `[context] persona_max_tokens`. */
export type SoulState = 'live' | 'awaiting' | 'over_cap'

export function soulState(p: Pick<PersonaInfo, 'awaiting' | 'over_cap'>): SoulState {
  return p.awaiting ? 'awaiting' : p.over_cap ? 'over_cap' : 'live'
}

/** The state in words, with its size against the cap: `live · 312 of 2,000 tokens`. */
export function soulWords(p: Pick<PersonaInfo, 'awaiting' | 'over_cap' | 'soul_tokens'>, cap: number): string {
  const size = p.soul_tokens === undefined ? '' : ` · ${p.soul_tokens.toLocaleString('en-US')} of ${cap.toLocaleString('en-US')} tokens`
  switch (soulState(p)) {
    case 'awaiting': return 'awaiting its soul'
    case 'over_cap': return `over its cap${size}: carried whole, trim it`
    default: return `live${size}`
  }
}

/** The personas that have a key (the built-in default has none), the default first, then by key. */
export function personasOf(r: Pick<PersonaListResult, 'personas'>): PersonaInfo[] {
  return r.personas
    .filter((p) => p.key !== undefined)
    .sort((a, b) => Number(!!b.is_default) - Number(!!a.is_default) || (a.key ?? '').localeCompare(b.key ?? ''))
}

/** What the add form sends: trimmed, and only what was written (a persona with no soul awaits it). */
export function addParams(f: { key: string; name: string; emoji: string; card: string; soul: string }): PersonaAddParams {
  const p: PersonaAddParams = { key: f.key.trim(), from: 'the cockpit' }
  if (f.name.trim()) p.name = f.name.trim()
  if (f.emoji.trim()) p.emoji = f.emoji.trim()
  if (f.card.trim()) p.card = f.card.trim()
  if (f.soul.trim()) p.soul = f.soul
  return p
}

/** What the confirm says before a soul is written: the persona, the cap, and that the file is replaced (and kept). */
export function soulAsk(key: string, cap: number): string {
  return `Write the soul of ${key}? The file is replaced and the old one kept beside it. A soul over ${cap.toLocaleString('en-US')} tokens is refused. It goes live at the next compile.`
}
