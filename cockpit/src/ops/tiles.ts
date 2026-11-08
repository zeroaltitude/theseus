// The command bar's tiles as a state (theseus-emqbm): which tile is selected and how each one looks. A tile is a counter
// and a filter, and the filter is ONE bucket: choosing one replaces the last, choosing the chosen one clears it. Pure:
// `npm test` runs it (test/ops-tiles.test.ts), and it imports only the page's own types.
import type { Bucket, Filters } from './address.ts'

/** The filters after a click on a tile, or its key: the bucket chosen, replacing any other, or cleared when it was the
 *  chosen one. Every path that sets the bucket filter asks this (the tile, its key, the empty inspector's counts). */
export function toggleBucket(f: Filters, b: Bucket): Filters {
  const { bucket, ...rest } = f
  return bucket === b ? rest : { ...rest, bucket: b }
}

export interface TileLook {
  /** The tile wears the lit treatment: a ring and a glow. */
  lit: boolean
  /** The tile's pressed state, for assistive tech. */
  pressed: boolean
  /** Something waits on the owner: drawn on the number, never as the lit treatment. */
  attention: boolean
}

/** How a tile looks, from its bucket, its count and the one bucket the filter holds. */
export function tileLook(b: Bucket, value: number, selected: string | undefined): TileLook {
  const pressed = selected === b
  return { lit: pressed, pressed, attention: b === 'needs' && value > 0 }
}
