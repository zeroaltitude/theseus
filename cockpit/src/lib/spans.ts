// A turn's trace, flat, for the flame chart and the session deck's timeline (theseus-hnof): each span with its depth, a
// kind's colour as a mark, and where the turn's time went by kind. Pure, so `node --test` runs it (test/spans.test.ts).
import type { Span } from '@protocol'
import type { Tone } from './taxonomy.ts'
import { TONE_MARK } from './viz.ts'

/** A span's kind, in the cockpit's tones. */
export const kindTone: Record<string, Tone> = {
  turn: 'live', lock: 'wait', loop: 'model', compile: 'think', store: 'idle', provider: 'model', tool: 'tool', mark: 'money',
}

/** A span kind's colour as a mark: its tone's step (`TONE_MARK`); a kind the cockpit has no tone for is the gray. */
export const spanColor = (kind: string): string => TONE_MARK[kindTone[kind] ?? 'idle']

export interface Flat { name: string; kind: string; depth: number; start: number; end: number; attrs: unknown }

/** The trace's spans in order, each with its depth; a span still open ends where it starts. */
export function flatten(s: Span, depth = 0, out: Flat[] = []): Flat[] {
  out.push({ name: s.name, kind: s.kind, depth, start: s.start_us, end: s.end_us ?? s.start_us, attrs: s.attrs })
  for (const c of s.children ?? []) flatten(c, depth + 1, out)
  return out
}

/** Where a turn's time went, by kind of span, the most first (µs): the leaf-ish spans only. The turn and its loops hold
 *  the rest, and a `tools` span holds calls that ran together, so its calls count, not it; a mark is an instant. */
export function timeByKind(flat: readonly Flat[]): [kind: string, us: number][] {
  const m = new Map<string, number>()
  for (const f of flat) {
    if (f.kind !== 'turn' && f.kind !== 'loop' && f.kind !== 'tools' && f.end > f.start) m.set(f.kind, (m.get(f.kind) ?? 0) + (f.end - f.start))
  }
  return [...m.entries()].sort((x, y) => y[1] - x[1] || x[0].localeCompare(y[0]))
}
