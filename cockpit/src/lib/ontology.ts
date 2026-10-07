// The ontology view's pure parts (theseus-8kk.2): the category tree's order and depth, and what a session's memberships
// and guidance say that its newest compilation's manifest does not yet. Pure, with no import but the protocol's types,
// so `node --test` runs its test (`cockpit/test/ontology.test.ts`) as it is.
import type { OntologyCategory, OntologyKind, OntologyMembership } from '@protocol'

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
