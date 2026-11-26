// How much the Benchmarks page is showing (GitHub issue 17, step 4): a family is one card of `cardsOf` (a benchmark and
// tier the manifest measures, or a suite known only from its reports, or the omnibus), and every report the build embeds
// belongs to exactly one family. The page counts its reports and its families from the build's own files, says how many
// families its table can rank, and lists each of the others once with the one reason it is not ranked. Pure, so
// `node --test` runs its test (test/benchfamilies.test.ts).
import type { Run } from './bench.ts'
import { OMNIBUS, suiteOf, type Card } from './benchcards.ts'
import type { Manifest } from './benchindex.ts'

export interface Family {
  card: Card
  /** Every report of the family, newest first. */
  reports: string[]
  /** The family's newest report, which the page links. */
  newest: string
  /** Ranked in the table, or listed with `why` under it. */
  ranked: boolean
  why: string | null
}

export interface Families {
  /** The families in the cards' order, newest first. */
  families: Family[]
  ranked: Family[]
  rest: Family[]
  /** The reports the build holds. */
  reports: number
  /** Reports no family takes: none, while the test holds. */
  unplaced: string[]
}

/** A card speaks in numbers (and is ranked in the table) when the run supports a pass rate and Theseus is one of its arms. */
export const ranks = (c: Card): boolean => !c.weak && !c.legacy && c.lines.some((l) => l.theseus)

/** Why a family measured against itself, or measuring something that is not a pass rate, is not ranked: one reason a
 *  suite, in the words a person who is not a benchmark expert can take. A suite the build gains needs a row here, and
 *  the test fails until it has one. */
export const UNRANKED_WHY: Readonly<Record<string, string>> = {
  'gate-bench': 'a speed and size measurement of Theseus alone: no pass rate',
  h2h: 'a speed measurement beside another harness: no pass rate',
  async: 'how well work overlaps and survives a kill: no pass rate',
  recall: 'how much is remembered over a long chat, not tasks solved: no pass rate',
  'memory-exam': 'Theseus’s own memory settings compared with each other: no other harness',
  retrieval: 'Theseus’s own ways of finding a note compared with each other: no other harness',
  context: 'how close a size estimate is: no pass rate',
  f10: 'a checklist of the first ten minutes, run once: too small to rank',
  threadbench: 'Theseus compared with itself, a switch on and off: no other harness',
  'bughunt-rs': 'bugs fixed a trial, not a pass rate',
  references: 'how many store records a turn reads to lay results kept by reference: no pass rate',
  [OMNIBUS]: 'the review of every run in one page: not a measurement of its own',
}

/** The one reason a family is not ranked, in its own words; null for a ranked one. A manifest family says why its numbers
 *  do not speak (an earlier run, a smoke, too few trials); a suite known only from reports says what it measures. */
export function whyNotRanked(c: Card): string | null {
  if (ranks(c)) return null
  if (c.kind === 'measured') {
    if (c.legacy) return 'superseded by the tiered runs'
    if (c.weak) return c.weak
    return 'Theseus was not one of the arms measured'
  }
  return UNRANKED_WHY[c.id] ?? 'no pass rate measured beside another harness'
}

/** The id of the family a report belongs to, or null when none takes it. A report the manifest cites belongs to its
 *  benchmark and tier; the omnibus to its own; one the manifest does not cite to the suite's card, or, for a suite the
 *  manifest measures, to the family of that suite whose newest measurement is the oldest. */
export function familyIdOf(report: string, suite: string, cards: readonly Card[], manifest: Manifest): string | null {
  const row = manifest.rows.find((r) => r.report === report)
  if (row) {
    const id = `${row.benchmark}:${row.tier ?? ''}`
    if (cards.some((c) => c.id === id)) return id
  }
  if (report === OMNIBUS) return cards.some((c) => c.id === OMNIBUS) ? OMNIBUS : null
  const own = cards.find((c) => c.kind === 'suite' && c.id === suite)
  if (own) return own.id
  const kin = cards.filter((c) => c.kind === 'measured' && suiteOf(c.id.split(':')[0]) === suite)
  return kin.sort((a, b) => a.asOf.localeCompare(b.asOf))[0]?.id ?? null
}

/** The families of the build: `cards` from `cardsOf`, `list` the runs newest first, `reports` the names the build holds. */
export function familiesOf(cards: readonly Card[], list: readonly Run[], reports: readonly string[], manifest: Manifest): Families {
  const suite = new Map(list.map((r) => [r.name, r.suite]))
  const held = new Map<string, string[]>()
  const unplaced: string[] = []
  // `list` is newest first, so each family's reports are too.
  for (const r of list) {
    const id = reports.includes(r.name) ? familyIdOf(r.name, suite.get(r.name) ?? '', cards, manifest) : null
    if (id === null) { if (reports.includes(r.name)) unplaced.push(r.name); continue }
    held.set(id, [...(held.get(id) ?? []), r.name])
  }
  const families: Family[] = []
  for (const card of cards) {
    const mine = held.get(card.id) ?? []
    if (mine.length === 0) continue
    families.push({ card, reports: mine, newest: mine[0], ranked: ranks(card), why: whyNotRanked(card) })
  }
  return { families, ranked: families.filter((f) => f.ranked), rest: families.filter((f) => !f.ranked), reports: reports.length, unplaced }
}

const n = (c: number, one: string, many = `${one}s`) => `${c.toLocaleString('en-US')} ${c === 1 ? one : many}`

/** The one sentence above the table: how much the build holds, how much of it the table ranks, and why not the rest. */
export function summaryWords(f: Families): string {
  const rest = f.rest.length
  const held = `This build holds ${n(f.reports, 'report')}, in ${n(f.families.length, 'family', 'families')} of benchmark.`
  if (f.ranked.length === 0) return `${held} None of them can be ranked yet: ${rest === 1 ? 'it is' : 'they are'} listed below with the reason.`
  const table = `The table ranks ${f.ranked.length} of them: the ones where Theseus’s pass rate was measured beside another harness’s, on enough trials to speak.`
  if (rest === 0) return `${held} ${table}`
  return `${held} ${table} The other ${rest} ${rest === 1 ? 'is' : 'are'} listed under it with the reason ${rest === 1 ? 'it is' : 'each is'} not ranked.`
}
