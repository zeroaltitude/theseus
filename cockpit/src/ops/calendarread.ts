// The calendar read's answer and the one rule about holding it (theseus-vhtuv). Pure, so a test can import it.
import type { CalendarWindow } from '@protocol'

export interface CalendarRead { calendars: CalendarWindow[]; error: string | null; read: boolean }

export const EMPTY: CalendarRead = { calendars: [], error: null, read: false }

/** What the hook's answer becomes when the spans change: with no span to read (Coming up folded, outside the Timeline)
 *  the last answer is dropped, so it is never drawn again as current; a new span keeps the old answer until its read
 *  lands, so the Timeline does not flicker. */
export const heldRead = (prev: CalendarRead, sig: string): CalendarRead => (sig ? prev : EMPTY)
