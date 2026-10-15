// The sixth plate in live mode (`src/ship/sincedigest.ts`, theseus-qkoi3), run by `npm test`: told by the daemon's
// `work.digest` (a recorded answer, `fixtures/digest.json`), with the time machine and a replay still reading the fold.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { WorkDigestResult } from '@protocol'
import { sinceOf } from '../src/ship/since.ts'
import { digestLines, plateOf, sinceFromDigest, total } from '../src/ship/sincedigest.ts'

const fixture = (name: string) => JSON.parse(readFileSync(new URL(`./fixtures/${name}.json`, import.meta.url), 'utf8')) as WorkDigestResult
/** A burst, a failure and a question still waiting. */
const digest = fixture('digest_burst')
/** Recorded from a real daemon (theseusd's tests/digest.rs, THESEUS_RECORD_DIGEST): three small tasks report, a slow one
 *  is cancelled, a call is answered, a timer fires. */
const recorded = fixture('digest')
const HOUR = 3_600_000
const SINCE = 1791000000000
/** The look began here: the last line of the fixture is after it. */
const UNTIL = 1791000900000
const NOW = 1791010000000

const fold = () => sinceOf({ model: null, rows: [], rowsReady: true, since: SINCE, until: UNTIL, now: NOW })

test('the digest tells the stretch: its lines, failures first, and not what came after the look began', () => {
  const s = sinceFromDigest(fold(), digest, NOW)
  assert.deepEqual(s.lines.map((l) => [l.tag, l.text, l.tone]), [
    ['failed', '✗ Tide table failed', 'fault'],
    ['answered', '✓ you answered: fs.write notes.txt', 'wait'],
    ['reported', '📋 3 tasks reported to harbour survey', 'ok'],
  ])
  assert.equal(s.more, 0)
  assert.deepEqual(s.lines[0].to, { session: 'ses_t' })
  assert.equal(s.failures, 1)
  assert.deepEqual(s.tasks, { started: 0, ended: 3, failed: 1 })
  assert.deepEqual(s.questions, { came: 0, went: 1, missed: 1, waiting: 1 })
  assert.equal(s.tone, 'fault')
  assert.equal(s.caption, 'while you were away: 3 reports, 1 failure and 1 question answered')
  assert.equal(s.quiet, false)
  assert.deepEqual(s.tally.map((t) => [t.part, t.value, t.word, t.sub]), [
    ['sessions', '0', 'sessions', '0 turns'],
    ['tasks', '3', 'finished', '0 started'],
    ['wrong', '1', 'failed', 'went wrong'],
    ['questions', '1', 'answered', '1 still waits'],
    ['spent', '$0', 'spent', '0 calls'],
  ])
})

test('a folded line counts what it stands for', () => {
  assert.equal(total(digest.lines, 'reported'), 4)
  assert.equal(total(digest.lines.filter((l) => l.at_ms <= UNTIL), 'reported'), 3)
  assert.equal(digestLines(digest.lines, UNTIL, NOW).length, 3)
})

test('an empty digest is a quiet stretch', () => {
  const s = sinceFromDigest(fold(), { ...digest, lines: [], questions: [] }, NOW)
  assert.equal(s.quiet, true)
  assert.equal(s.caption, 'nothing happened while you were away')
  assert.equal(s.tone, 'idle')
})

test('the time machine and a replay read the fold, never the digest', () => {
  const f = fold()
  assert.equal(plateOf(f, digest, true, NOW), f, 'in the past: the fold at its moment')
  assert.equal(plateOf(f, null, false, NOW), f, 'a daemon that has no digest: the fold')
  assert.equal(plateOf(f, undefined, false, NOW), f)
  const live = plateOf(f, digest, false, NOW)
  assert.notEqual(live, f)
  assert.equal(live.lines[0].tag, 'failed')
})

test('a stream recorded from a real daemon tells every report, the cancel, the answer and the wake, once', () => {
  const at = recorded.lines[0].at_ms - 60_000
  const until = recorded.lines[recorded.lines.length - 1].at_ms + 1000
  const now = until + 60_000
  const base = sinceOf({ model: null, rows: [], rowsReady: true, since: at - HOUR, until, now })
  const s = sinceFromDigest(base, recorded, now)
  assert.equal(total(recorded.lines, 'reported'), 3)
  assert.equal(s.tasks.ended, 4, 'three reports and a cancel')
  assert.equal(s.failures, 0)
  assert.deepEqual(s.questions, { came: 0, went: 1, missed: 1, waiting: 0 })
  assert.equal(s.caption, 'while you were away: 3 reports, 1 question answered, 1 cancel and 1 wake')
  // Failures first, then the answer, then the newest report; three lines, three more.
  assert.deepEqual(s.lines.map((l) => [l.tag, l.text]), [
    ['answered', '✓ you answered: fs.write /work/note.txt'],
    ['reported', '📋 Count to three. reported'],
    ['reported', '📋 Count to three. reported'],
  ])
  assert.equal(s.more, 3)
  assert.equal(s.tone, 'wait')
  // The ids of the three reports are three tasks: told once each.
  const ids = new Set(digestLines(recorded.lines, until, now).filter((l) => l.tag === 'reported').map((l) => l.id))
  assert.equal(ids.size, 3)
})
