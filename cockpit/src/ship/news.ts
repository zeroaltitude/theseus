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
    body: 'A slow roll when nothing runs, at a few frames a second, and nothing else moves. The swell rises with tokens a minute and the turns running, and settles back to the roll as they end; the sea on the console says how high, in words. Calm mode stills it.',
    anchor: { kind: 'dom', selector: '.ship-sea' },
  },
  {
    id: 'sound', since: '2026-10-07', title: 'Sound, if you want it',
    body: 'Off until you turn it on, here, and off again whenever the page opens. Then the sea, soft waves on a shore that rise a little with the work and go quiet in Calm, and three quiet cues, all made in the browser: an oar going out splashes, something waiting for you rings the ship’s bell (you hear it from another window), and a failure sounds a low horn. The sea, the bell and the horn on every page.',
    anchor: { kind: 'dom', selector: '.ship-sound' },
  },
  {
    id: 'since', since: '2026-10-07', title: 'Since you last looked',
    body: 'The watch’s sixth plate. Back after a while away, it says how long you were gone and what happened meanwhile: the sessions and tasks that started or finished, what went wrong, the questions that came and went, and what it cost. Show lights it on the chart, replay runs the stretch on the time machine, and seen quiets it. Keys 1 to 6 light each plate.',
    anchor: { kind: 'dom', selector: '.ship-watch-slot' },
  },
  {
    id: 'context', since: '2026-10-07', title: 'What Theseus knows, and what a turn sees',
    body: 'The Context page (in the rail): the imported episodes with their sources, places, labels and summaries, the topics, the books’ state, and asking the index. Pick a ship (or a bench) and “Its context” opens what its turn carried: the system block part by part, the guidance, the tools, the recall and why each note came, with token counts.',
    anchor: { kind: 'fleet' },
  },
  {
    id: 'states', since: '2026-10-08', title: 'Live, quiet and retired',
    body: 'The Ship shows the live fleet by default: ships at sea with a turn in the last day, working, or waiting for you. Quiet ones ride at anchor, and retired ones lie up in harbour, dimmed: replaced by a newer session (a superseded ship flies a signal to its successor), retired by hand, or never used. Pick Live, Quiet, Retired or All here, each with its count; nothing is deleted, the palette (⌘K) finds every session, and a session you select shows whatever the filter. The session deck retires and reopens.',
    anchor: { kind: 'dom', selector: '.ship-statebar' },
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
