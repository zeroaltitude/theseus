// The calendar layer's pure half (theseus-jid7t.5): what the Timeline lens draws of `calendar.window`'s answer, and what
// each event verb may carry. Pure: `npm test` runs it (test/ops-calendar.test.ts), and it imports only the protocol's
// types.
//
// The data rules (the owner's rule 3), each held by a test:
// - Every surface the lens draws an event on (its block, tooltip, the header line, the Selected panel, Coming up, the
//   table, the search) is made here from `redactEvent`'s answer, so redact mode masks them all at once: a sensitive
//   calendar's title becomes a bar, and its place, call link and attendee count are dropped; the times and the
//   calendar's colour stay.
// - A verb on a sensitive calendar's event sends only the time and the calendar's label (`eventVerbPayload`): never its
//   title, place, attendees or links. The daemon's own read of the event, with the calendar tool, is what marks the
//   session sensitive.
// - Nothing here is stored or logged: the page holds an event in memory, and the address holds only its opaque key.
import type { CalendarEvent, CalendarStatus, CalendarWindow, CalendarWindowResult } from '@protocol'

/** What a redacted title is drawn as. */
export const BAR = '▬▬▬▬▬▬'
/** The lock beside a sensitive calendar, wherever it is named. */
export const LOCK = '🔒'

/** An event as a surface may draw it: redacted when the page is and its calendar is sensitive. */
export interface Shown extends CalendarEvent { redacted: boolean }

/** `ev` as the page may show it. Under redact, a sensitive calendar's event keeps its key, times, kind and link to
 *  Google (an href, never drawn as text), and loses its title, place, call link and attendee count. */
// Under redact a sensitive event keeps no Google link: its `eid` is the event and calendar ids in base64.
export function redactEvent(ev: CalendarEvent, cal: Pick<CalendarWindow, 'sensitive'>, on: boolean): Shown {
  if (!on || !cal.sensitive) return { ...ev, redacted: false }
  return {
    key: ev.key, start_ms: ev.start_ms, end_ms: ev.end_ms, all_day: ev.all_day, attendees: 0, organizer_self: ev.organizer_self,
    self_response: ev.self_response, html_link: '', busy: ev.busy, redacted: true,
  }
}

/** A hatch over a colour: a sensitive calendar's (with its colour beneath), or a busy block's. */
export const hatch = (color: string, sensitive: boolean, busy: boolean): string => busy
  ? `repeating-linear-gradient(135deg, ${color}55 0 3px, ${color}1a 3px 7px)`
  : sensitive ? `repeating-linear-gradient(135deg, ${color}66 0 2px, ${color}2e 2px 6px)` : `${color}4d`

// ---------------------------------------------------------------- the four states

export interface CalState {
  /** The legend's tone: a state, never the calendar's material. */
  tone: 'ok' | 'wait' | 'fault'
  /** The legend's word for it. */
  words: string
  /** The one fix, in the page's words (the daemon's `detail` where it says more). */
  fix?: string
  /** What the strip draws for it: the events, hatched busy blocks, an empty dashed lane, or nothing (a stale week is
   *  never drawn as current). */
  draws: 'events' | 'busy' | 'lane' | 'none'
}

/** The sharing steps, generic: no account, address or organisation. */
export const SHARING_STEPS = 'From the calendar’s own account, open Google Calendar → the calendar → Settings and sharing, and share it with the account Theseus reads ([google] account) with ‘See all event details’. If only free/busy is offered, the workspace’s admin must allow external sharing. Then add the calendar to [google] calendars and restart.'

const STATES: Readonly<Record<CalendarStatus, CalState>> = {
  ok: { tone: 'ok', words: 'read', draws: 'events' },
  busy_only: { tone: 'wait', words: 'free/busy only: titles are not shared', fix: SHARING_STEPS, draws: 'busy' },
  not_found: { tone: 'wait', words: 'not shared yet', fix: SHARING_STEPS, draws: 'lane' },
  forbidden: { tone: 'fault', words: 'refused by Google', fix: 'Run `theseus google auth` from your own shell, or see the Systems page.', draws: 'none' },
  auth_lapsed: { tone: 'fault', words: 'Google’s consent lapsed', fix: 'Run `theseus google auth` from your own shell to consent again.', draws: 'none' },
  off: { tone: 'fault', words: 'Google is off', fix: 'Set up [google] in the config (the Systems page says how), then restart.', draws: 'none' },
}

/** A calendar's state: its tone, words, fix and what the strip draws. The daemon's `detail` is said first when it
 *  has one. */
export function stateOf(cal: Pick<CalendarWindow, 'status' | 'detail'>): CalState {
  const s = STATES[cal.status] ?? STATES.off
  if (cal.status === 'ok' || !cal.detail) return s
  return { ...s, fix: s.fix && s.fix !== cal.detail ? `${cal.detail} ${s.fix}` : cal.detail }
}

/** The events a calendar's state lets the strip draw: none for a calendar that was not read. */
export function drawnEvents(cal: CalendarWindow): CalendarEvent[] {
  const d = stateOf(cal).draws
  return d === 'events' || d === 'busy' ? cal.events : []
}

// ---------------------------------------------------------------- the words of an event

const WEEKDAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'] as const
const pad = (n: number) => String(n).padStart(2, '0')
/** `14:05`, local. */
export const hm = (at: number): string => { const d = new Date(at); return `${pad(d.getHours())}:${pad(d.getMinutes())}` }
/** `Fri 10/23`, local. */
export const dayWords = (at: number): string => { const d = new Date(at); return `${WEEKDAYS[d.getDay()]} ${d.getMonth() + 1}/${d.getDate()}` }

/** `14:00–15:00`, or `all day`. */
export function spanWords(ev: Pick<CalendarEvent, 'start_ms' | 'end_ms' | 'all_day'>): string {
  return ev.all_day ? 'all day' : `${hm(ev.start_ms)}–${hm(ev.end_ms)}`
}

/** An event's name as every surface draws it: its title, `busy` for a free/busy block, a bar when redacted. */
export function nameOf(s: Shown): string {
  if (s.redacted) return BAR
  if (s.busy) return 'busy'
  return s.title || '(no title)'
}

/** The calendar's name with its lock when it is sensitive. */
export const calName = (cal: Pick<CalendarWindow, 'label' | 'sensitive'>): string => (cal.sensitive ? `${cal.label} ${LOCK}` : cal.label)

const RESPONSE: Readonly<Record<string, string>> = { accepted: 'you accepted', declined: 'you declined', tentative: 'you said maybe', needsAction: 'you have not answered' }

/** Every string the lens draws for one event, from its redacted form alone. The "DOM scan" test reads them all. */
export interface EventFacts {
  /** The block's own label. */
  block: string
  /** The tooltip. */
  tip: string
  /** The panel header's line on hover. */
  header: string
  /** The Selected panel's facts, label and value. */
  facts: Array<readonly [string, string]>
  /** The call link and the place, when the panel may show them. */
  call?: string
  location?: string
  /** Coming up's line. */
  coming: string
  /** The table's row. */
  row: readonly string[]
  /** What the search matches. */
  search: string
}

export function eventFacts(ev: CalendarEvent, cal: CalendarWindow, redact: boolean): EventFacts {
  const s = redactEvent(ev, cal, redact)
  const name = nameOf(s)
  const when = spanWords(s)
  const day = dayWords(s.start_ms)
  const cn = calName(cal)
  const facts: Array<readonly [string, string]> = [['when', `${day} · ${when}`], ['calendar', cn]]
  if (!s.redacted && !s.busy) facts.push(['attendees', s.attendees === 1 ? '1 attendee' : `${s.attendees} attendees`])
  if (s.self_response) facts.push(['you', RESPONSE[s.self_response] ?? s.self_response])
  if (s.organizer_self) facts.push(['organizer', 'you organize it'])
  if (s.location) facts.push(['place', s.location])
  if (s.call) facts.push(['call', s.call])
  const extra = [s.location, s.call && 'a call link', !s.redacted && !s.busy && s.attendees ? `${s.attendees} attendees` : undefined].filter(Boolean).join(' · ')
  return {
    block: name,
    tip: `${name}\n${day} · ${when} · ${cn}${extra ? `\n${extra}` : ''}${s.redacted ? '\nredacted: ⇧R shows it' : ''}`,
    header: `${name} · ${when} · ${cn}`,
    facts,
    call: s.call,
    location: s.location,
    coming: `${name} · ${cn}`,
    row: [day, when, cn, name, s.location ?? '', s.redacted || s.busy ? '' : String(s.attendees)],
    search: s.redacted ? cn : [s.title, s.location, cal.label].filter(Boolean).join('\n'),
  }
}

/** Does the search `q` find this event? Under redact a sensitive event matches only by its calendar's label. */
export function eventMatches(ev: CalendarEvent, cal: CalendarWindow, redact: boolean, q: string): boolean {
  if (!q.trim()) return true
  const text = eventFacts(ev, cal, redact).search.toLowerCase()
  return q.toLowerCase().split(/\s+/).filter(Boolean).every((w) => text.includes(w))
}

/** The privacy rule a sensitive calendar's event shows in the Selected panel. */
export const PRIVACY_NOTE = 'This calendar is sensitive. Its events reach only a model of US origin, never a shared place, and no ledger row names them. These verbs send its time and its calendar’s name, never its title, place, attendees or links: the task or chat reads it with the calendar tool.'

// ---------------------------------------------------------------- the verbs

export type EventVerb = 'join' | 'google' | 'remind' | 'prep' | 'ask'
export const EVENT_VERBS: readonly EventVerb[] = ['join', 'google', 'remind', 'prep', 'ask']

/** What a verb does with an event: open a link in a new tab (nothing to the daemon), make one call, open the New task
 *  sheet filled in, or open a private chat with its first message. */
export type Payload =
  | { kind: 'link'; href: string }
  | { kind: 'call'; method: 'wake.create'; params: { when: string; note: string } }
  | { kind: 'sheet'; brief: string; title: string; usOnly: boolean }
  | { kind: 'ask'; label: string; input: string; usOnly: boolean }
  | { kind: 'none'; why: string }

/** How long before the start Remind me fires. */
export const REMIND_BEFORE_MS = 10 * 60_000

/** The words that name an event in a verb's payload: for a sensitive calendar the calendar and the time alone; for an
 *  open one its title and time (the design's quote). */
function named(ev: CalendarEvent, cal: CalendarWindow): string {
  const at = ev.all_day ? `on ${dayWords(ev.start_ms)}` : `at ${hm(ev.start_ms)} on ${dayWords(ev.start_ms)}`
  if (cal.sensitive || ev.busy || !ev.title) return `the ${cal.label} event ${at}`
  return `“${ev.title}” ${at}`
}

/** What `verb` sends for `ev`. A sensitive calendar's event sends no title, place, attendee or link into a wake's
 *  note, a task's brief or a chat; a link verb only opens Google's own link in a new tab. */
export function eventVerbPayload(ev: CalendarEvent, cal: CalendarWindow, verb: EventVerb, now = Date.now()): Payload {
  const sens = cal.sensitive
  const name = named(ev, cal)
  switch (verb) {
    case 'join': return ev.call ? { kind: 'link', href: ev.call } : { kind: 'none', why: 'this event has no call link' }
    case 'google': return ev.html_link ? { kind: 'link', href: ev.html_link } : { kind: 'none', why: 'Google gave no link to it' }
    case 'remind': {
      const before = ev.start_ms - REMIND_BEFORE_MS
      const at = before > now ? before : ev.start_ms
      if (at <= now) return { kind: 'none', why: 'it has begun: a reminder would be in the past' }
      return { kind: 'call', method: 'wake.create', params: { when: new Date(at).toISOString(), note: `Coming up: ${name}.` } }
    }
    case 'prep': return {
      kind: 'sheet', usOnly: sens,
      title: sens ? `Prep: the ${cal.label} event at ${hm(ev.start_ms)}` : `Prep: ${ev.title ?? `the ${cal.label} event`}`,
      brief: `Prepare me for ${name}. Read it with the calendar tool (calendar ${cal.label}${sens ? '; it is sensitive' : ''}), then gather what I need for it and report back.`,
    }
    case 'ask': return {
      kind: 'ask', usOnly: sens, label: sens ? `about the ${cal.label} event` : `about ${ev.title ?? 'an event'}`,
      input: `I'd like to talk about ${name} on my ${cal.label} calendar. Read it with the calendar tool first.`,
    }
  }
}

/** Every string a payload carries: what the test scans for an event's content. */
export function payloadStrings(p: Payload): string[] {
  return Object.values(p).filter((v): v is string | object => typeof v === 'string' || (typeof v === 'object' && v !== null))
    .flatMap((v) => (typeof v === 'string' ? [v] : Object.values(v).filter((x): x is string => typeof x === 'string')))
}

/** Ask Theseus's calls, in order: open the chat, hold the picked profile on the session (sticky, so every later turn
 *  keeps it), then the first message (which names the profile too). With no pick there is no `session.profile`. The
 *  open call's params do not need the id; the others carry `sessionId`. */
export function askCalls(sessionId: string, p: { label: string; input: string }, pick: string | undefined): Array<{ method: string; params: Record<string, unknown> }> {
  return [
    { method: 'session.open', params: { kind: 'conversation', label: p.label } },
    ...(pick ? [{ method: 'session.profile', params: { session_id: sessionId, profile: pick } }] : []),
    { method: 'turn.submit', params: { session_id: sessionId, input: p.input, ...(pick && { profile: pick }) } },
  ]
}

/** The floor's words when `session.profile` answered that it refuses the profile (an ok answer with `refused` set);
 *  the chat then sends no first turn and never falls back to another profile. */
export const profileRefusal = (r: { refused?: string }): string | undefined => r.refused

// ---------------------------------------------------------------- the New task sheet's profiles

export const US_ONLY_WORDS = 'this event’s calendar is sensitive: US models only'

// ---------------------------------------------------------------- Coming up

export interface ComingEvent { key: string; at_ms: number; line: string; color: string; sensitive: boolean }

/** The calendar's timed events that start within `within` of `now`, soonest first, each line redacted as the page is. */
export function comingEvents(cals: readonly CalendarWindow[], now: number, redact: boolean, within = 3 * 3_600_000): ComingEvent[] {
  const out: ComingEvent[] = []
  for (const cal of cals) {
    for (const ev of drawnEvents(cal)) {
      if (ev.all_day || ev.start_ms < now || ev.start_ms > now + within || ev.self_response === 'declined') continue
      out.push({ key: ev.key, at_ms: ev.start_ms, line: eventFacts(ev, cal, redact).coming, color: cal.color, sensitive: cal.sensitive })
    }
  }
  return out.sort((a, b) => a.at_ms - b.at_ms || (a.key < b.key ? -1 : 1))
}

/** The calendar and event a selection's key names. */
export function findEvent(cals: readonly CalendarWindow[], key: string | null): { ev: CalendarEvent; cal: CalendarWindow } | null {
  if (!key) return null
  for (const cal of cals) for (const ev of drawnEvents(cal)) if (ev.key === key) return { ev, cal }
  return null
}

/** Joins the spans' answers: one entry per calendar, its events from every span, each once by key. */
export function joinWindows(parts: readonly CalendarWindowResult[]): CalendarWindow[] {
  const by = new Map<string, CalendarWindow>()
  for (const p of parts) {
    for (const c of p.calendars) {
      const was = by.get(c.id)
      if (!was) { by.set(c.id, { ...c, events: [...c.events] }); continue }
      const seen = new Set(was.events.map((e) => e.key))
      // A calendar whose read failed in one span says so: its worst status wins.
      if (c.status !== 'ok') by.set(c.id, { ...c, events: [] })
      else if (was.status === 'ok') was.events.push(...c.events.filter((e) => !seen.has(e.key)))
    }
  }
  return [...by.values()]
}
