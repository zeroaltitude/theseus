// The add-a-kind form's pure half (theseus-s430x, R7c): the params of `ontology.kind.add` built from the form's fields, with
// the daemon's own checks mirrored (`theseus-ontology`'s `Kind::check_row` and `ontology_kind_add`) so a refusal is read
// before anything is sent, and the confirm worded with the exact JSON that goes. The daemon still judges every call: this
// only saves the round trip, and a rule it does not mirror comes back as the daemon's own words. Pure, with no import but
// the protocol's types, so `node --test` runs its test (`cockpit/test/ontokinds.test.ts`) as it is.
import type { OntologyKindAddParams } from '@protocol'

/** The origins an interpreted kind may name as assigners: every one but the transport (a kind the transport assigns is a
 *  given kind, and an interpreted kind never takes a membership from it). In the chart's order. */
export const KIND_ORIGINS: readonly string[] = ['operator', 'import', 'jev', 'sweep', 'dream']

/** A kind carries at most this many examples, each a line of at most `EXAMPLE_MAX` characters. */
export const EXAMPLES_MAX = 5
export const EXAMPLE_MAX = 100
/** A description's longest, in characters. */
export const DESCRIPTION_MAX = 2000
/** The rules the compiler reads. */
export const KIND_RULES: readonly string[] = ['chain', 'intent_line']

export interface KindForm {
  name: string
  description: string
  assignedBy: string[]
  /** A positive whole number, or `many`; empty leaves the daemon's default (3). */
  perSession: string
  /** Another kind's name; empty for none. */
  parent: string
  /** A whole number; empty puts the kind after the last. */
  precedence: string
  /** `chain` or `intent_line`; empty is `chain`. */
  rule: string
  /** One example a line. */
  examples: string
}

/** The empty form: who may assign it starts at the daemon's default, the operator and Jev. */
export const NO_KIND_FORM: KindForm = { name: '', description: '', assignedBy: ['operator', 'jev'], perSession: '', parent: '', precedence: '', rule: '', examples: '' }

export interface KindBuilt {
  /** The call, or `null` while any field is at fault. */
  params: OntologyKindAddParams | null
  /** The fields at fault, each with the words the daemon would refuse it in. */
  errors: Partial<Record<keyof KindForm, string>>
}

const U32_MAX = 4_294_967_295
const length = (s: string) => [...s].length
const KIND_NAME = /^[a-z][a-z0-9_-]{0,31}$/
const NAME_WORDS = 'a kind’s name is a lowercase letter, then up to 31 lowercase letters, digits, `_`, or `-`'

/** The params of `ontology.kind.add` for a form, or the fields at fault. Fields left empty are left out (but the
 *  description, which the protocol's type requires and the daemon trims), so the daemon's defaults stay the daemon's; the keys come in the protocol's order, which is the order the confirm shows them. `taken`
 *  holds the names the kinds table has (retired ones too: the daemon refuses a name it lists). */
export function buildKindAdd(f: KindForm, taken: readonly string[]): KindBuilt {
  const errors: KindBuilt['errors'] = {}
  const name = f.name.trim()
  if (!KIND_NAME.test(name)) errors.name = `${JSON.stringify(name)} is not a name: ${NAME_WORDS}`
  else if (taken.includes(name)) errors.name = `the kind \`${name}\` is in the table already`

  const description = f.description.trim()
  if (length(description) > DESCRIPTION_MAX) errors.description = `it is ${length(description)} characters long, and the most is ${DESCRIPTION_MAX}`
  else if ([...description].some((c) => /\p{Cc}/u.test(c) && c !== '\n' && c !== '\t')) errors.description = 'it holds a control character'

  const by = f.assignedBy
  const badOrigin = by.find((o) => !KIND_ORIGINS.includes(o))
  if (badOrigin !== undefined) errors.assignedBy = badOrigin === 'transport'
    ? 'the transport assigns given kinds only: an interpreted kind never takes a membership from it'
    : `${JSON.stringify(badOrigin)} is not an origin an interpreted kind names`
  else if (new Set(by).size !== by.length) errors.assignedBy = 'it lists an origin twice'

  const per = f.perSession.trim()
  if (per !== '' && per !== 'many' && !(/^[1-9][0-9]*$/.test(per) && Number(per) <= U32_MAX)) {
    errors.perSession = `per session is a whole number of 1 or more, or "many", not ${JSON.stringify(per)}`
  }

  const parent = f.parent.trim()
  if (parent !== '' && !KIND_NAME.test(parent)) errors.parent = `${JSON.stringify(parent)} is not a kind’s name: ${NAME_WORDS}`

  const prec = f.precedence.trim()
  if (prec !== '' && !(/^[0-9]+$/.test(prec) && Number(prec) <= U32_MAX)) errors.precedence = `precedence is a whole number, not ${JSON.stringify(prec)}`

  const rule = f.rule.trim()
  if (rule !== '' && !KIND_RULES.includes(rule)) errors.rule = `${JSON.stringify(rule)} is not a rule: use chain or intent_line`

  const examples = f.examples.split('\n').map((e) => e.trim()).filter((e) => e !== '')
  if (examples.length > EXAMPLES_MAX) errors.examples = `it lists ${examples.length}, and a kind carries at most ${EXAMPLES_MAX}`
  else {
    const long = examples.find((e) => length(e) > EXAMPLE_MAX)
    const control = examples.find((e) => [...e].some((c) => /\p{Cc}/u.test(c)))
    if (long !== undefined) errors.examples = `an example is ${length(long)} characters long, and the most is ${EXAMPLE_MAX}`
    else if (control !== undefined) errors.examples = 'an example holds a control character'
  }

  if (Object.keys(errors).length) return { params: null, errors }
  const params: OntologyKindAddParams = {
    name,
    description,
    ...(by.length ? { assigned_by: [...by] } : {}),
    ...(per ? { per_session: per } : {}),
    ...(parent ? { parent } : {}),
    ...(prec ? { precedence: Number(prec) } : {}),
    ...(rule ? { rule } : {}),
    ...(examples.length ? { examples } : {}),
  }
  return { params, errors }
}

/** The confirm for adding a kind, in the words of what it sends: the exact params, as the daemon will read them. */
export function kindAddAsk(p: OntologyKindAddParams): string {
  return `Add the kind “${p.name}” (ontology.kind.add)?\n\nIt sends exactly:\n\n${JSON.stringify(p, null, 2)}\n\nThe kind is interpreted: its categories are made by the origins named, and it needs no code. It cannot be deleted, only retired once nothing holds it. A kind that is already in the table is refused.`
}
