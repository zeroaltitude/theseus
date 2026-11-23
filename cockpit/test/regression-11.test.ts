import test from 'node:test'
import assert from 'node:assert/strict'
import { eventVerbPayload, payloadStrings } from '../src/ops/calendar.ts'
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
