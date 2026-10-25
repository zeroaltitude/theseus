// The filter chip (theseus-odj0m): what a counter's click is filtering by, on the canvas it filters, "paused·queued · 4 ×".
// Its × clears the filter; so does Esc, through the page's `back` handler. The Graph's toolbar wears it; the Board
// imports it the same way. The words are `chipWords` (ops/board.ts), the count the counter's own.
import { X } from 'lucide-react'
import type { Bucket } from './address'
import { chipWords } from './board'
import { OWNER_CALLS, type OwnerCalls } from './state'

export interface FilterChipProps { bucket: Bucket; n: number; onClear: () => void; calls?: OwnerCalls }

export function FilterChip({ bucket, n, onClear, calls = OWNER_CALLS }: FilterChipProps) {
  return (
    <span data-testid="ops-filter-chip" className="flex items-center gap-1 rounded bg-gold/15 px-1.5 text-gold ring-1 ring-gold/40">
      <span className="num">{chipWords(bucket, n, calls)}</span>
      <button type="button" onClick={onClear} aria-label="Clear the filter" title="clear the filter (Esc)" className="rounded hover:text-ink"><X size={10} /></button>
    </span>
  )
}
