// A check's basis in words (`src/lib/check.ts`), run by `npm test`: the same cases as theseus-protocol's
// `a_checks_line_names_what_it_excluded_and_its_model`, so every surface says one thing. Invented ids.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { TaskCheck } from '@protocol'
import { checkLine } from '../src/lib/check.ts'

test("a check's line names what it excluded and its model, and its overlap flags", () => {
  const c: TaskCheck = {
    checked_task: 'ses_0123456789abcdefa1b2c3', checked_short: 'a1b2c3', report_node: 'msg_r', report_at_ms: 1,
    excluded_sessions: ['ses_0123456789abcdefa1b2c3'], admitted: [], profile: 'glm', provider: 'zai', model: 'glm-4.6',
    overlaps: [], at_ms: 2,
  }
  assert.equal(checkLine(c), '🔍 check of task a1b2c3 · independent (excluded ses_…a1b2c3, glm-4.6)')
  c.overlaps = [{ source: 'brief', node_id: 'trs_1', words: 12, span: 'x' }]
  assert.ok(checkLine(c).endsWith(' · overlap: 1 span'))
  c.overlaps.push({ source: 'piece 1', node_id: 'trs_2', words: 14, span: 'y' })
  assert.ok(checkLine(c).endsWith(' · overlap: 2 spans'))
})
