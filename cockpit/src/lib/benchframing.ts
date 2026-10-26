// Models against harnesses, two framings (theseus-ycp9h). A comparison of harnesses on ONE model (framing A, the
// controlled one) is not a comparison of harnesses that each ran their own (framing B), and Theseus routed over every
// model (T) is neither. The page says which, in words and by grouping, never by a new marker shape: the diamond already
// means "ran under conditions its peers did not share", and an arm can be both model-varying and unequal.
//
// Framing is DERIVED from what the dataset holds, never guessed: an arm is B when its model differs from the set's
// reference (the table's `model` column, the label's "(Model, effort)", the dataset's `native_model`) or it carries the
// `other-model` tag; T when it is Theseus and the dataset says it routed; A otherwise.
//
// OWNER CALL: whether Theseus-general (T) is *the as-shipped arm* or *a line beside the Claude-routed series* is open
// (theseus-snjhp, theseus-uhc2d). Nothing here chooses. T is its own class, drawn at slot 0 and always labelled like any
// Theseus arm, with no larger marker and no badge; the Verdict still says when it is dominated.
import { family, labelModel, sameModel, type Point } from './bench.ts'
import type { IndexRow } from './benchindex.ts'

export type Framing = 'A' | 'B' | 'T'

/** The blocks in the order they are listed: the controlled comparison first. */
export const FRAMINGS: readonly Framing[] = ['A', 'T', 'B']
export const FRAMING_WORDS: Record<Framing, string> = { A: 'Same model, harness varies', T: 'Theseus: all models, its own choice', B: 'Model varies' }

/** Whether a Theseus key names a routed one: the dataset's own word for it in the key (`theseus-routed`), or, for a
 *  versioned key (`theseus-68`), the dataset's model line saying Theseus itself is Jev-routed. A model line that only
 *  describes the routed arm as the thing compared against ("the routed arm: Jev's pick…") does not make a fixed-model
 *  or refused-commands variant routed. The one test the legend (`framingOf`) and the card (`framingOfRow`) share. */
const routed = (key: string, modelLine?: string | null) =>
  family(key) === 'theseus' && (/routed/.test(key) || (key !== 'theseus' && /Jev-routed/.test(modelLine ?? '')))

export function framingOf(p: Point, reference: string | undefined = p.refModel ?? p.model): Framing {
  if (routed(p.harness, p.model)) return 'T'
  if (p.tags.some((t) => t.kind === 'other-model') || p.native) return 'B'
  // An arm model the dataset states that its peers' reference does not match, or one stated where no reference is known.
  return p.armModel && !(reference && sameModel(p.armModel, reference)) ? 'B' : 'A'
}

/** The same decision for a manifest row (the browse cards), from its arm, variant and label. */
export function framingOfRow(r: IndexRow): Framing {
  // A manifest row names no routing of its own: the variant says it, or the dataset's model line says Theseus was routed.
  if (r.arm === 'theseus' && routed(r.variant, r.model)) return 'T'
  const m = labelModel(r.harness)
  if (m && !sameModel(r.model ?? '', m)) return 'B'
  // A dataset that names a harness's own model in the model line: "anthropic/… (Codex CLI: openai/…)".
  for (const [, who] of (r.model ?? '').matchAll(/\(([^():]+):\s*[^()]+\)/g)) if (r.harness.toLowerCase().startsWith(who.trim().toLowerCase())) return 'B'
  return 'A'
}

/** A framing-B point's model, for its label ("Codex CLI, GPT-6-Astra"); nothing for the rest. */
export const modelLabel = (p: Point): string | undefined => (framingOf(p) === 'B' ? p.armModel : undefined)

/** Items split into the framing blocks, the empty blocks left out. */
export function byFraming<T>(items: readonly T[], framing: (x: T) => Framing): { framing: Framing; words: string; items: T[] }[] {
  return FRAMINGS.map((f) => ({ framing: f, words: FRAMING_WORDS[f], items: items.filter((x) => framing(x) === f) })).filter((b) => b.items.length)
}

// ---------------------------------------------------------------- arms.json, when it exists

/** `docs/benchmarks/arms.json` (theseus-snjhp): `{version: 1, arms: [{key, name, family, framing, model, harness, status}]}`.
 *  It does not exist at this build's base, so this reads it if and only if it is there and trusted; the arms it lists as
 *  planned are named in gray words, never with a value. */
export function plannedArms(text: string | null | undefined): string[] {
  if (!text) return []
  try {
    const doc = JSON.parse(text) as { version?: unknown; arms?: unknown }
    if (doc.version !== 1 || !Array.isArray(doc.arms)) return []
    return doc.arms.flatMap((a) => (a && typeof a === 'object' && (a as { status?: unknown }).status === 'planned' && typeof (a as { name?: unknown }).name === 'string' && (a as { name: string }).name ? [(a as { name: string }).name] : []))
  } catch { return [] }
}
