// The calendar layer's data rules (theseus-jid7t.5; the owner's rule 3): redact mode masks a sensitive calendar on
// every surface the lens draws (the "DOM scan": there is no DOM in `npm test`, so every string the lens's fact
// functions make for an event is scanned for the invented content); a sensitive event's verbs carry only the time and
// the calendar's label; the states each say their words and fix; the New task sheet offers US models only for a
// sensitive event; nothing is stored or logged.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { CalendarStatus, ProfileInfo } from '../src/protocol.gen/index.ts'
import {
  BAR, comingEvents, drawnEvents, eventFacts, eventMatches, EVENT_VERBS, eventVerbPayload, findEvent, joinWindows, payloadStrings, profileChoices,
  redactEvent, SHARING_STEPS, stateOf, US_ONLY_WORDS,
} from '../src/ops/calendar.ts'
import { cal, ev, local, OPEN, SECRET, twoCalendars } from './ops-calendar-fixtures.ts'

const read = (p: string) => readFileSync(new URL(`../${p}`, import.meta.url), 'utf8')

/** Every string a surface of the lens makes for an event. */
function surfaces(f: ReturnType<typeof eventFacts>): string[] {
  return [f.block, f.tip, f.header, ...f.facts.flat(), f.call ?? '', f.location ?? '', f.coming, ...f.row, f.search]
}
/** The content of an event as distinct strings, the attendee count as the words the facts say it in. */
const contentOf = (x: typeof SECRET) => [x.title, x.location, x.call, `${x.attendees} attendees`, String(x.attendees)]
const hits = (strings: readonly string[], needles: readonly string[]) => needles.filter((n) => strings.some((s) => s.includes(n)))

test('redact mode: no surface draws a sensitive event’s title, place, call link or attendees; times and colour stay', () => {
  const { work, personal, secret, open } = twoCalendars()
  const red = eventFacts(secret, work, true)
  assert.deepEqual(hits(surfaces(red), contentOf(SECRET)), [], 'nothing of the work event under redact')
  assert.ok(!surfaces(red).some((s) => s.includes(SECRET.html_link)), 'Google’s link is never drawn as text')
  assert.equal(red.block, BAR)
  assert.match(red.header, /14:00–15:00 · Work 🔒/)
  const shown = redactEvent(secret, work, true)
  assert.equal(shown.start_ms, secret.start_ms)
  assert.equal(shown.end_ms, secret.end_ms)
  assert.equal(shown.redacted, true)
  // Off: the same event draws whole.
  const whole = eventFacts(secret, work, false)
  assert.deepEqual(hits(surfaces(whole), [SECRET.title, SECRET.location, SECRET.call, `${SECRET.attendees} attendees`]).length, 4)
  // An open calendar is never masked, redact or not.
  for (const on of [true, false]) {
    const f = eventFacts(open, personal, on)
    assert.deepEqual(hits(surfaces(f), [OPEN.title, OPEN.location, OPEN.call, `${OPEN.attendees} attendees`]).length, 4, `open, redact ${on}`)
  }
})

test('redact mode reaches Coming up and the search: a sensitive title neither shows nor matches', () => {
  const { work, personal, secret } = twoCalendars()
  const now = local(2026, 10, 23, 13, 30)
  const red = comingEvents([work, personal], now, true)
  assert.equal(red.length, 1, 'the open one is at 18:00, past three hours')
  assert.deepEqual(hits(red.map((c) => c.line), contentOf(SECRET)), [])
  assert.match(red[0].line, /^▬+ · Work 🔒$/)
  assert.match(comingEvents([work], now, false)[0].line, /Quarzite merger huddle/)
  assert.equal(eventMatches(secret, work, true, 'quarzite'), false, 'under redact the title does not match')
  assert.equal(eventMatches(secret, work, true, 'work'), true, 'its calendar does')
  assert.equal(eventMatches(secret, work, false, 'quarzite merger'), true)
  assert.equal(eventMatches(secret, work, true, 'xylophone'), false, 'nor its place')
})

test('a sensitive event’s verbs carry only its time and calendar; an open one’s may quote its title', () => {
  const { work, personal, secret, open } = twoCalendars()
  const now = local(2026, 10, 23, 9)
  for (const v of EVENT_VERBS) {
    const p = eventVerbPayload(secret, work, v, now)
    const strings = payloadStrings(p)
    if (p.kind === 'link') {
      // A link verb opens Google's own link or the call in a new tab: nothing to the daemon.
      assert.ok(v === 'join' || v === 'google', v)
      continue
    }
    assert.deepEqual(hits(strings, [...contentOf(SECRET), SECRET.html_link]), [], `${v} carries nothing of the event`)
    assert.ok(strings.some((s) => s.includes('Work')), `${v} names the calendar`)
  }
  const remind = eventVerbPayload(secret, work, 'remind', now)
  assert.equal(remind.kind, 'call')
  if (remind.kind === 'call') {
    assert.equal(remind.method, 'wake.create')
    assert.equal(remind.params.note, 'Coming up: the Work event at 14:00 on Fri 10/23.')
    assert.equal(new Date(remind.params.when).getTime(), local(2026, 10, 23, 13, 50), 'ten minutes before, as an instant')
  }
  const prep = eventVerbPayload(secret, work, 'prep', now)
  assert.ok(prep.kind === 'sheet' && prep.usOnly && /calendar tool/.test(prep.brief))
  const ask = eventVerbPayload(secret, work, 'ask', now)
  assert.ok(ask.kind === 'ask' && ask.usOnly && /calendar tool/.test(ask.input))
  // The open calendar's verbs quote its title and time, and nothing more.
  for (const v of ['remind', 'prep', 'ask'] as const) {
    const s = payloadStrings(eventVerbPayload(open, personal, v, now))
    assert.ok(s.some((x) => x.includes(OPEN.title)), `${v} quotes the open title`)
    assert.deepEqual(hits(s, [OPEN.location, OPEN.call, OPEN.html_link]), [], `${v} sends no place or link`)
    const p = eventVerbPayload(open, personal, v, now)
    assert.ok(p.kind !== 'sheet' || !p.usOnly)
  }
  assert.deepEqual(eventVerbPayload(open, personal, 'join', now), { kind: 'link', href: OPEN.call })
  assert.deepEqual(eventVerbPayload(secret, work, 'google', now), { kind: 'link', href: SECRET.html_link })
  // Under redact the panel opens the call from the redacted event: there is none to open.
  assert.equal(eventVerbPayload(redactEvent(secret, work, true), work, 'join', now).kind, 'none')
  assert.equal(eventVerbPayload(secret, work, 'remind', local(2026, 10, 23, 15)).kind, 'none', 'a reminder for a meeting begun is refused')
})

test('the four states: each status has its tone, words, fix and what the strip draws', () => {
  const want: Record<CalendarStatus, [string, RegExp, string]> = {
    ok: ['ok', /read/, 'events'],
    busy_only: ['wait', /free\/busy/, 'busy'],
    not_found: ['wait', /not shared yet/, 'lane'],
    forbidden: ['fault', /refused/, 'none'],
    auth_lapsed: ['fault', /consent lapsed/, 'none'],
    off: ['fault', /Google is off/, 'none'],
  }
  for (const [status, [tone, words, draws]] of Object.entries(want) as Array<[CalendarStatus, [string, RegExp, string]]>) {
    const s = stateOf({ status, detail: '' })
    assert.equal(s.tone, tone, status)
    assert.match(s.words, words, status)
    assert.equal(s.draws, draws, status)
    if (status !== 'ok') assert.ok(s.fix && s.fix.length > 20, `${status} has its fix`)
  }
  assert.equal(stateOf({ status: 'not_found', detail: '' }).fix, SHARING_STEPS)
  assert.match(stateOf({ status: 'auth_lapsed', detail: '' }).fix!, /theseus google auth/)
  assert.match(stateOf({ status: 'forbidden', detail: 'the daemon says: share it again' }).fix!, /^the daemon says: share it again/, 'the daemon’s words first')
  // The sharing steps are generic: no account, address or organisation.
  assert.doesNotMatch(SHARING_STEPS, /@|\.com|\.org/)
  // A stale week is never drawn as current: a calendar not read draws no events, whatever it holds.
  const e = ev('evk_x', 1, 2)
  for (const status of ['forbidden', 'auth_lapsed', 'off', 'not_found'] as const) assert.deepEqual(drawnEvents(cal('work.cal.example.test', { status, events: [e] })), [], status)
  assert.equal(drawnEvents(cal('work.cal.example.test', { status: 'busy_only', events: [e] })).length, 1)
})

test('a busy block draws as busy, with no title, and an event is found by its opaque key', () => {
  const work = cal('work.cal.example.test', { status: 'busy_only', events: [ev('evk_b', local(2026, 10, 23, 9), local(2026, 10, 23, 10), { busy: true })] })
  const f = eventFacts(work.events[0], work, false)
  assert.equal(f.block, 'busy')
  assert.ok(!f.facts.some(([k]) => k === 'attendees'))
  assert.equal(findEvent([work], 'evk_b')?.cal.id, 'work.cal.example.test')
  assert.equal(findEvent([work], 'evk_nope'), null)
})

test('the spans’ answers join: one calendar each, its events once, a failed span’s status wins', () => {
  const a = ev('evk_a', 1, 2)
  const b = ev('evk_b', 3, 4)
  const joined = joinWindows([
    { calendars: [cal('work.cal.example.test', { events: [a] }), cal('personal.cal.example.test', { events: [a] })] },
    { calendars: [cal('work.cal.example.test', { events: [a, b] }), cal('personal.cal.example.test', { status: 'auth_lapsed', detail: 'consent', events: [] })] },
  ])
  assert.deepEqual(joined.find((c) => c.id === 'work.cal.example.test')!.events.map((e) => e.key), ['evk_a', 'evk_b'])
  assert.equal(joined.find((c) => c.id === 'personal.cal.example.test')!.status, 'auth_lapsed')
})

test('the New task sheet: a sensitive event offers US models only, the rest greyed with the reason; none known, none picked', () => {
  const p = (name: string, origin?: string): ProfileInfo => ({ name, provider: 'x', model: `${name}-model`, max_output_tokens: 1, has_system: true, live: false, ...(origin && { origin }) } as ProfileInfo)
  const profiles = [p('harbour', 'cn'), p('quay', 'us'), p('jetty', 'us'), p('dock')]
  const s = profileChoices(profiles, true, 'jetty')
  assert.equal(s.pick, 'jetty', 'the live one, when it is of US origin')
  assert.deepEqual(s.choices.filter((c) => c.off).map((c) => c.name), ['harbour', 'dock'], 'not of US origin, or of none known')
  for (const c of s.choices.filter((x) => x.off)) assert.equal(c.off, US_ONLY_WORDS)
  assert.equal(profileChoices(profiles, true, 'harbour').pick, 'quay', 'the live one is not: the first that is')
  assert.equal(profileChoices([p('harbour', 'cn'), p('dock')], true).pick, '', 'none of US origin: it fails closed')
  const open = profileChoices(profiles, false, 'harbour')
  assert.equal(open.pick, '', 'an open event: the parent’s model, as before')
  assert.ok(open.choices.every((c) => !c.off))
  assert.equal(US_ONLY_WORDS, 'this event’s calendar is sensitive: US models only')
  const sheet = read('src/ops/NewTask.tsx')
  assert.match(sheet, /disabled=\{!!c\.off\}/)
  assert.match(sheet, /if \(closed\) \{ setSaid/)
  assert.match(read('src/ops/EventDetail.tsx'), /<NewTask brief=\{prepSheet\.brief\} title=\{prepSheet\.title\} usOnly=\{prepSheet\.usOnly\}/)
})

test('nothing is stored, logged or put in the address; the links open in a new tab with no referrer', () => {
  const files = ['src/ops/calendar.ts', 'src/ops/calendarwatch.ts', 'src/ops/timeline.ts', 'src/ops/drag.ts', 'src/ops/TimelineLens.tsx', 'src/ops/WeekStripV2.tsx', 'src/ops/EventDetail.tsx', 'src/ops/timelinekeys.ts']
  for (const f of files) {
    const code = read(f).replace(/\/\/.*$/gm, '')
    assert.ok(!/console\.|localStorage|sessionStorage|indexedDB|usePref\(/.test(code), `${f} stores or logs`)
  }
  const panel = read('src/ops/EventDetail.tsx')
  assert.equal((panel.match(/target="_blank" rel="noopener noreferrer"/g) ?? []).length, 2, 'Join the call and Open in Google')
  // The address carries an event's opaque key only, as `ev:<key>`.
  assert.match(read('src/ops/WeekStripV2.tsx'), /onPick\(`ev:\$\{b\.key\}`\)/)
  // Every verb that reaches the daemon goes through `useOpsAct`'s gate.
  assert.match(panel, /send\('event_remind', p\.method, p\.params\)/)
  assert.match(panel, /send<\{ session_id: string \}>\('event_ask', 'session\.open'/)
  assert.match(read('src/ops/act.ts'), /if \(!enabled\(verb, past\)\) return \{ ok: false, error: PAST_WORDS \}/)
})

test('the calendar read: the one timer, every five minutes while visible, stopped when hidden and on unmount', () => {
  const hook = read('src/ops/calendarwatch.ts')
  const code = hook.replace(/\/\/.*$/gm, '')
  assert.equal((code.match(/setInterval\(/g) ?? []).length, 1, 'one timer')
  assert.match(code, /EVERY_MS = 5 \* 60_000/)
  assert.match(code, /if \(document\.visibilityState === 'visible'\) every = setInterval\(\(\) => void read\(\), EVERY_MS\)/)
  assert.match(code, /if \(every\) clearInterval\(every\)/)
  assert.match(code, /method === 'calendar\.changed'/)
  assert.ok(!/refetchInterval|useRpc\(/.test(code))
  // A failed read drops the last one's events: a stale week is never shown as current.
  assert.match(code, /setState\(\{ calendars: \[\], error:/)
})
