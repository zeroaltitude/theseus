// A settings question (issue 22) is answered only on its whole card: the compact queue row offers no approve, the
// bulk approve lists it as asking one by one and never sends it, and its words reach every card whole.
import test from 'node:test'
import assert from 'node:assert/strict'
import { answerOf, queueOf, settingsQuestion, settingsWords } from '../src/ops/queue.ts'
import { bulkVerbs } from '../src/ops/selection.ts'
import { confirm, NOW, MIN } from './ops-fixtures.ts'
import type { OpsRow } from '../src/ops/board.ts'

/** The longest card the daemon allows: twelve settings of 400 characters, before and after. */
const LONGEST = `Change 12 settings?\n${Array.from({ length: 12 }, (_, i) => `- memory.k${i}: "${'a'.repeat(400)}" → "${'b'.repeat(400)}" (after a restart)`).join('\n')}\nTheseus's reason, in its own words: "x"\n`

function settings(id: string) {
  return confirm(id, {
    tool: 'config.ack', input: { question: LONGEST }, reason: LONGEST, requested_at_ms: NOW - 3 * MIN, expires_at_ms: NOW + 10 * MIN,
    plain: { who: 'Theseus', what: 'change 12 settings (…); the values are on the card', why: '', held: 'x', risk: 'changes Theseus’s own settings', yes: 'y', no: 'n', detail: LONGEST },
  })
}

test('a settings question is recognised by the daemon’s words, and carries them whole', () => {
  const c = settings('act_s1')
  assert.equal(settingsQuestion(c), true)
  assert.equal(settingsQuestion(confirm('act_o1')), false)
  const w = settingsWords(c)!
  assert.equal(w.detail, LONGEST, 'every character of the longest card')
  assert.ok(w.detail.length > 9_000)
  assert.equal(settingsWords(confirm('act_o2')), null)
})

test('the compact row offers no approve for it, and still lets the owner decline', () => {
  const q = queueOf({ confirms: [settings('act_s1'), confirm('act_o1')], views: [], titleOf: (s) => s })
  const by = new Map(q.map((i) => [i.key, i]))
  assert.equal(answerOf(by.get('q:act_s1')!, { verb: 'approve' }), null, 'no y on a settings question')
  assert.equal(answerOf(by.get('q:act_s1')!, { verb: 'grant' }), null)
  assert.ok(answerOf(by.get('q:act_s1')!, { verb: 'decline' }), 'a no is harmless')
  assert.deepEqual(answerOf(by.get('q:act_o1')!, { verb: 'approve' })?.params, { question_id: 'act_o1', option: 'approve' }, 'any other approval is as it was')
})

test('bulk approve never sends it and lists it as asking one by one', () => {
  const q = queueOf({ confirms: [settings('act_s1'), confirm('act_o1')], views: [], titleOf: (s) => s })
  const row = (i: (typeof q)[number]): OpsRow => ({ id: i.key, sessionId: i.sessionId } as unknown as OpsRow)
  const offers = bulkVerbs(q.map(row), q)
  const approve = offers.find((o) => o.verb === 'approve')!
  assert.deepEqual(approve.items.map((i) => i.key), ['q:act_o1:approve'], 'only the ordinary approval is in the batch')
  assert.deepEqual((approve.oneByOne ?? []).map((i) => i.key), ['q:act_s1'], 'the settings question is listed, one by one')
})

test('a source line is part of the card the cockpit draws, whole, as one line under its setting', () => {
  const card = [
    'Change 1 setting?',
    '- providers.lantern.api_base: not set (its default) → "https://lantern.example.test" (after a restart)',
    '  Theseus\'s source for this, in its own words (nothing here checked it): "the docs say <b>it</b> \\"quoted\\" \\u{202e}"',
    'Take care: since your last message this conversation has read text from outside (a page, a search or a server), so the request may not be only yours.',
    'Theseus\'s reason, in its own words: "x"',
    '',
  ].join('\n')
  const c = confirm('act_s2', {
    tool: 'config.ack', input: { question: card }, reason: card, requested_at_ms: NOW - MIN, expires_at_ms: NOW + 10 * MIN,
    plain: { who: 'Theseus', what: 'change `providers.lantern.api_base`', why: '', held: 'x', risk: 'changes Theseus’s own settings', yes: 'y', no: 'n', detail: card },
  })
  const w = settingsWords(c)!
  assert.equal(w.detail, card, 'every line, as the daemon wrote it')
  const rows = w.detail.split('\n')
  const at = rows.findIndex((r) => r.startsWith('- providers.lantern.api_base'))
  assert.match(rows[at + 1], /^ {2}Theseus's source for this, in its own words \(nothing here checked it\): "/)
  assert.ok(rows[at + 2].startsWith('Take care: since your last message'), 'the care line is as it was, after it')
  // The cockpit draws the card as a text node, so a tag in a source is text: the card is not parsed as markup.
  assert.ok(rows[at + 1].includes('<b>it</b>'))
})
