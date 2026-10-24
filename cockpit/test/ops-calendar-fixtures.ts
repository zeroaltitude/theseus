// Invented calendars for the Timeline lens's tests (theseus-jid7t.5): a personal one, open, in brass, and a work one,
// sensitive, in verdigris, each with distinctive invented content, so a test can scan every string a surface makes for
// it. No real calendar, person, address or link.
import type { CalendarEvent, CalendarWindow } from '../src/protocol.gen/index.ts'

export const local = (y: number, mo: number, d: number, h = 0, mi = 0) => new Date(y, mo - 1, d, h, mi).getTime()

/** The secret parts of the work calendar's event: none may leave the page through a verb, or show under redact. */
export const SECRET = {
  title: 'Quarzite merger huddle',
  location: 'Room Xylophone-9, Tidewater Annex',
  call: 'https://meet.example.test/zzq-plmk-wre',
  attendees: 17,
  html_link: 'https://calendar.example.test/event?eid=c2VjcmV0LXdvcms',
}

/** The open calendar's event: its verbs may quote its title and time. */
export const OPEN = {
  title: 'Lantern-maker pottery class',
  location: 'Studio Brambleworth',
  call: 'https://meet.example.test/aaa-open-bbb',
  attendees: 4,
  html_link: 'https://calendar.example.test/event?eid=b3Blbi1wZXJzb25hbA',
}

export const ev = (key: string, start_ms: number, end_ms: number, over: Partial<CalendarEvent> = {}): CalendarEvent => ({
  key, start_ms, end_ms, all_day: false, attendees: 0, organizer_self: false, html_link: `https://calendar.example.test/event?eid=${key}`, busy: false, ...over,
})

export const cal = (id: string, over: Partial<CalendarWindow> = {}): CalendarWindow => ({
  id, label: id.startsWith('work') ? 'Work' : 'Personal', color: id.startsWith('work') ? '#4a7c6f' : '#b08d57', sensitive: id.startsWith('work'), status: 'ok', detail: '', events: [], ...over,
})

/** Friday 2026-10-23, 14:00 to 15:00, on each calendar. */
export function twoCalendars(): { personal: CalendarWindow; work: CalendarWindow; secret: CalendarEvent; open: CalendarEvent } {
  const secret = ev('evk_w1', local(2026, 10, 23, 14), local(2026, 10, 23, 15), { ...SECRET, self_response: 'accepted', organizer_self: true })
  const open = ev('evk_p1', local(2026, 10, 23, 18), local(2026, 10, 23, 19, 30), { ...OPEN, self_response: 'tentative' })
  return {
    personal: cal('personal.cal.example.test', { events: [open] }),
    work: cal('work.cal.example.test', { events: [secret] }),
    secret, open,
  }
}
