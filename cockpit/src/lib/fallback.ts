// A refusal's fallback in words (theseus-7gir.18): the same line as `TurnFallback::line` in theseus-protocol, so the
// cockpit's turn view, the CLI, the terminal UI, and the reply's post on Discord say one thing.
import type { TurnFallback } from '@protocol'

/** A model as a person names it: `claude-sonnet-5-5` is Sonnet 5.5, and a date after the version is dropped; any
 *  other id is itself (`theseus_protocol::route::model_name`). */
export function modelName(id: string): string {
  if (!id.startsWith('claude-')) return id
  const [family = '', ...rest] = id.slice('claude-'.length).split('-')
  if (!/^[a-z]+$/.test(family)) return id
  const version: string[] = []
  for (const p of rest) {
    if (!/^[0-9]{1,2}$/.test(p)) break
    version.push(p)
  }
  const name = family[0].toUpperCase() + family.slice(1)
  return version.length ? `${name} ${version.join('.')}` : name
}

/** `twice`, `3 times`: how many times, in words, for a count above one. */
const times = (n: number): string => (n === 2 ? 'twice' : `${n} times`)

/** `Sonnet 5.5 declined (cyber); Sonnet 5 answered.`, by how the turn ended (its stop reason). A turn that switched
 *  more than once says how many times, and one that stayed on the fallback for more than one answer says so; a
 *  latched turn with a single answer ended on it, so there was nothing to stay for and the line is the ordinary one.
 *  A record from before the counts reads as one refusal and, when `answered`, one answer (`TurnFallback::line`). */
export function fallbackLine(f: TurnFallback, stopReason: string): string {
  const [from, to] = [modelName(f.from), modelName(f.to)]
  const refusals = Math.max(f.refusals ?? 0, 1)
  const answers = f.answered ? Math.max(f.answers ?? 0, 1) : 0
  let declined = f.category ? `${from} declined (${f.category})` : `${from} declined`
  if (refusals > 1) declined = `${declined} ${times(refusals)}`
  if (answers > 0) {
    let said = `${to} answered`
    if (answers > 1) {
      said = `${said} ${times(answers)}`
      if (f.latched) said += ', and the turn stayed there'
    }
    return `${declined}; ${said}.`
  }
  if (stopReason === 'refusal') return `${declined}, and so did ${to}.`
  return `${declined}; the request went to ${to}.`
}
