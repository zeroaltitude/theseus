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

/** `Sonnet 5.5 declined (cyber); Sonnet 5 answered.`, by how the turn ended (its stop reason). */
export function fallbackLine(f: TurnFallback, stopReason: string): string {
  const [from, to] = [modelName(f.from), modelName(f.to)]
  const declined = f.category ? `${from} declined (${f.category})` : `${from} declined`
  if (f.answered) return `${declined}; ${to} answered.`
  if (stopReason === 'refusal') return `${declined}, and so did ${to}.`
  return `${declined}; the request went to ${to}.`
}
