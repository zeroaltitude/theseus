import { readFileSync } from 'node:fs'
import { runInNewContext } from 'node:vm'
import test from 'node:test'
import assert from 'node:assert/strict'
import { eventVerbPayload, payloadStrings, redactEvent } from '../src/ops/calendar.ts'
import { local, SECRET, twoCalendars } from './ops-calendar-fixtures.ts'

test('a sensitive label keeps the event title in reminders, prep tasks and chat', () => {
  const { secret, work } = twoCalendars()
  const now = local(2026, 10, 23, 9)
  const remind = eventVerbPayload(secret, work, 'remind', now)
  assert.ok(remind.kind === 'call')
  assert.ok(remind.params.note.includes(SECRET.title))
  const prep = eventVerbPayload(secret, work, 'prep', now)
  assert.ok(prep.kind === 'sheet')
  assert.equal(prep.title, `Prep: ${SECRET.title}`)
  assert.ok(prep.brief.includes(SECRET.title))
  const ask = eventVerbPayload(secret, work, 'ask', now)
  assert.ok(ask.kind === 'ask')
  assert.equal(ask.label, `about ${SECRET.title}`)
  assert.ok(ask.input.includes(SECRET.title))
})

test('busy-only and untitled calendar actions keep their source-data fallback for either label', () => {
  const { secret, work } = twoCalendars()
  const now = local(2026, 10, 23, 9)
  for (const sensitive of [false, true]) {
    for (const source of [{ ...secret, busy: true, title: undefined }, { ...secret, title: undefined }]) {
      for (const verb of ['remind', 'prep', 'ask'] as const) {
        const payload = eventVerbPayload(source, { ...work, sensitive }, verb, now)
        assert.notEqual(payload.kind, 'none')
        assert.ok(payloadStrings(payload).some((s) => s.includes(work.label)))
        assert.ok(payloadStrings(payload).every((s) => !s.includes(SECRET.title)))
      }
    }
  }
})

// Exercise the exact handler expression, so a raw-event fallback fails even though redactEvent itself is correct.
test('every event verb uses the shown event with screen redaction on and off', () => {
  const detail = readFileSync(new URL('../src/ops/EventDetail.tsx', import.meta.url), 'utf8')
  const expression = detail.match(/const pay = \(v: EventVerb\) => ([^\n]+)/)![1]
  const { secret: ev, work: cal } = twoCalendars()
  const now = local(2026, 10, 23, 9)
  for (const redact of [false, true]) {
    const shown = redactEvent(ev, cal, redact)
    for (const v of ['remind', 'prep', 'ask'] as const) {
      const p = runInNewContext(expression, { v, ev, cal, shown,
        eventVerbPayload: (event: typeof ev, calendar: typeof cal, verb: typeof v) => eventVerbPayload(event, calendar, verb, now),
      })
      const strings = payloadStrings(p)
      assert.notEqual(p.kind, 'none')
      assert.equal(strings.some((s) => s.includes(SECRET.title)), !redact, `${v}, redact=${redact}`)
      if (redact) assert.ok(strings.some((s) => s.includes(cal.label)))
    }
  }
  assert.match(detail, /const pay = \(v: EventVerb\) => eventVerbPayload\(shown, cal, v\)/)
})
