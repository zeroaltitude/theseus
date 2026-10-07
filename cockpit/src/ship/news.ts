// What's new on the Ship (theseus-hnof.2, the owner's C6): the full tour on a browser's first visit (and on ? and the
// Tour button, always); after an update that adds something this browser hasn't seen, a few skippable stops that fly to
// just the new things, once. Pure: the Ship asks `tourPlan` what to open, and a test holds it.
//
// The key is the cockpit's version for this purpose, `COCKPIT_VERSION`: a date, which an update that adds a stop here
// moves to its own date. The browser keeps the version it last saw (`SEEN_KEY`), written when it closes the full tour
// or the news, however it closes them (done or skipped): a stop shows once.

/** Where a stop points: an instrument by its selector, or the fleet itself (the camera fits it). */
export type NewsAnchor = { kind: 'dom'; selector: string } | { kind: 'fleet' }

export interface NewsItem {
  id: string
  /** The cockpit version that brought it (a date, `YYYY-MM-DD`). */
  since: string
  title: string
  body: string
  anchor: NewsAnchor
}

/** The things an update added, oldest first. Each lane's addition is one item. */
export const NEWS: NewsItem[] = [
  {
    id: 'console', since: '2026-10-07', title: 'The console: the engine and tokens a minute',
    body: 'The console keeps the two gauges no other place shows: the engine (is the kernel taking new turns, and how many run) and tokens a minute. The live profile and the uptime are in the top bar; scrub back with the ship’s log and the top bar says what they were then.',
    anchor: { kind: 'dom', selector: '.ship-console' },
  },
  {
    id: 'sea', since: '2026-10-07', title: 'The sea carries the work',
    body: 'Dead calm when nothing runs, and then the Ship draws nothing at all. The swell rises with tokens a minute and the turns running, and settles as they end; the sea on the console says how high, in words. Calm mode stills it.',
    anchor: { kind: 'dom', selector: '.ship-sea' },
  },
  {
    id: 'sound', since: '2026-10-07', title: 'Sound, if you want it',
    body: 'Off until you turn it on, here. Then three quiet cues, made in the browser: an oar going out splashes, something waiting for you rings the ship’s bell (you hear it from another window), and a failure sounds a low horn.',
    anchor: { kind: 'dom', selector: '.ship-sound' },
  },
  {
    id: 'since', since: '2026-10-07', title: 'Since you last looked',
    body: 'The watch’s sixth plate. Back after a while away, it says how long you were gone and what happened meanwhile: the sessions and tasks that started or finished, what went wrong, the questions that came and went, and what it cost. Show lights it on the chart, replay runs the stretch on the time machine, and seen quiets it. Keys 1 to 6 light each plate.',
    anchor: { kind: 'dom', selector: '.ship-watch-slot' },
  },
]

/** The cockpit's version: the newest stop's. */
export const COCKPIT_VERSION = NEWS.reduce((v, n) => (n.since > v ? n.since : v), '2026-10-06')

/** Where the browser keeps the version it last saw, and (from before versions were kept) that it finished the tour. */
export const SEEN_KEY = 'cockpit.ship.seen'
export const TOUR_KEY = 'cockpit.ship.tour'

/** A browser that finished the tour before versions were kept saw the prototype's Ship: this version. */
export const LEGACY_SEEN = '2026-10-06'

export type TourPlan = { kind: 'tour' } | { kind: 'news'; items: NewsItem[] } | { kind: 'none' }

/**
 * What to open on the Ship's first frame with a fleet.
 * @param tourDone what the browser kept under `TOUR_KEY` ('done' once the full tour closed)
 * @param seen what it kept under `SEEN_KEY`, the version it last saw
 */
export function tourPlan(tourDone: string | null, seen: string | null, current = COCKPIT_VERSION, news = NEWS): TourPlan {
  if (tourDone !== 'done' && seen === null) return { kind: 'tour' }
  const last = seen ?? LEGACY_SEEN
  if (last >= current) return { kind: 'none' }
  const items = news.filter((n) => n.since > last && n.since <= current)
  return items.length ? { kind: 'news', items } : { kind: 'none' }
}
