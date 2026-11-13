// A session's content labels (theseus-9ymz0): the chips, their tones, and the call Remove makes. SessionDeck.tsx is read as
// text, as test/ops-page.test.ts reads the page.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { LabelView } from '../src/protocol.gen/index.ts'
import { askOf, chipsOf, FLAGS, removedWords, toneOf, unlabelCall, unlabelFailureWords, unlabelRoute, wrongAskOf, wrongCall } from '../src/lib/labels.ts'

const label = (flag: string, over: Partial<LabelView> = {}): LabelView =>
  ({ flag, node: 'nod_e1', kind: 'fetched_page', source: 'http.fetch https://example.test/tides', confidence: 0.92, band: 'act', at_ms: 1_000, ...over })

test('a session with no labels has no chips, and a missing list is none', () => {
  assert.deepEqual(chipsOf([]), [])
  assert.deepEqual(chipsOf(undefined), [])
})

test('a chip names the flag and the node, and its tooltip the kind, the source, the confidence and the band', () => {
  const [c] = chipsOf([label('injection')])
  assert.equal(c.text, '⚑ injection · node nod_e1')
  assert.equal(c.node, 'nod_e1')
  assert.match(c.title, /^fetched_page · http\.fetch https:\/\/example\.test\/tides · 92% · act\./)
  // Pinned to what the daemon actually does at the shipped default (the owner's S17-D8): the call runs and the operator
  // is told. The second half pins that `refuse` is named as the mode that refuses, so neither half can drift back.
  assert.match(c.title, /something tried to steer the session; the call still runs/)
  assert.match(c.title, /injection = "refuse"` refuses its writes, programs and sends/)
})

test('a label with no node says only the flag', () => {
  const [c] = chipsOf([label('secret', { node: undefined, kind: 'operator_message', source: 'discord dm' })])
  assert.equal(c.text, '⚑ secret')
  assert.ok(!('node' in c))
})

test('injection is the warning tone, the other three the dimmer one, and they keep the daemon’s order', () => {
  const chips = chipsOf([label('injection'), label('secret'), label('confidential'), label('personal')])
  assert.deepEqual(chips.map((c) => c.flag), ['injection', 'secret', 'confidential', 'personal'])
  assert.deepEqual(chips.map((c) => c.tone), ['wait', 'idle', 'idle', 'idle'])
  assert.equal(toneOf('a_new_flag'), 'idle')
  assert.match(chips[1].title, /off China-origin models/)
})

test('a flag the table lacks still draws a chip, with no claim about its effect', () => {
  const [c] = chipsOf([label('a_new_flag')])
  assert.equal(c.text, '⚑ a_new_flag · node nod_e1')
  assert.ok(!c.title.includes('It '))
})

test('Remove calls content.unlabel with the session and the flag, and asks first', () => {
  const c = unlabelCall('ses_tides', 'injection')
  assert.equal(c.method, 'content.unlabel')
  assert.deepEqual(c.params, { session_id: 'ses_tides', flag: 'injection' })
  assert.equal(c.ask, askOf('injection', 'ses_tides'))
  assert.match(c.ask, /Remove the injection label from ses_tides\?/)
  assert.match(c.ask, /stay in the ledger/)
  assert.match(unlabelCall('ses_tides', 'personal').ask, /off China-origin models/)
})

test('the deck draws the chips and sends Remove through the pure call', () => {
  const src = readFileSync(new URL('../src/views/SessionDeck.tsx', import.meta.url), 'utf8')
  assert.match(src, /chipsOf\(s\.labels\)/)
  assert.match(src, /unlabelCall\(s\.session_id, c\.flag\)/)
})

test('a chip carries the judgment that raised its label, and one raised before has none', () => {
  const [a, b] = chipsOf([label('secret', { judgment: 'jdg_a1' }), label('personal')])
  assert.equal(a.judgment, 'jdg_a1')
  assert.ok(!('judgment' in b))
})

test('wrong labels the raising judgment’s answer no, and removes nothing', () => {
  const c = wrongCall('ses_tides', label('secret', { judgment: 'jdg_a1' }))
  assert.ok(c)
  assert.equal(c.method, 'judge.label')
  assert.deepEqual(c.params, {
    judgment: 'jdg_a1',
    question: 'secret',
    label: 'no',
    note: 'marked wrong by the owner via the cockpit; the label stays',
  })
  assert.equal(c.ask, wrongAskOf('secret', 'ses_tides'))
  assert.match(c.ask, /miscategorised/)
  assert.match(c.ask, /stays on ses_tides/)
})

test('a label with no judgment has no wrong press, since there is nothing to teach', () => {
  assert.equal(wrongCall('ses_tides', label('secret')), null)
})

test('the deck draws wrong beside Remove, sent through the pure call, and Remove is unchanged', () => {
  const src = readFileSync(new URL('../src/views/SessionDeck.tsx', import.meta.url), 'utf8')
  assert.match(src, /wrongCall\(s\.session_id, c\)/)
  assert.match(src, /unlabelCall\(s\.session_id, c\.flag\)/)
  assert.match(src, /aria-label=\{`Remove the \$\{c\.flag\} label`\}/)
})

// ---------------------------------------------------------------- the footer's removal link (theseus-emqbm)

test('the footer’s address names a session and a flag, and plans the removal without making it', () => {
  const r = unlabelRoute('ses_a1', 'injection')
  assert.equal(r.ok, true)
  if (!r.ok) return
  assert.equal(r.session, 'ses_a1')
  assert.equal(r.flag, 'injection')
  assert.deepEqual(r.call, { method: 'content.unlabel', params: { session_id: 'ses_a1', flag: 'injection' }, ask: askOf('injection', 'ses_a1') })
  assert.match(r.effect!, /the call still runs/)
  assert.match(r.effect!, /injection = "refuse"` refuses its writes, programs and sends/)
})

test('every flag the daemon knows has an address, and nothing else does', () => {
  assert.deepEqual([...FLAGS], ['injection', 'secret', 'confidential', 'personal'])
  for (const f of FLAGS) assert.equal(unlabelRoute('ses_a1', f).ok, true, f)
  for (const bad of ['', 'trust', 'INJECTION', 'injection/../secret']) {
    const r = unlabelRoute('ses_a1', bad)
    assert.equal(r.ok, false, bad)
    if (!r.ok) assert.match(r.why, /is not a content label/)
  }
  const none = unlabelRoute(undefined, 'secret')
  assert.equal(none.ok, false)
})

test('the page says what happened in the daemon’s voice, and that an already-gone label is no failure', () => {
  assert.equal(
    removedWords('injection', 'discord:1'),
    'Removed the injection label (by discord:1): calls that act are refused by nothing now. The label and its removal stay in the ledger.',
  )
  assert.match(removedWords('secret', 'cockpit'), /^Removed the secret label \(by cockpit\)\. /)
  assert.equal(
    unlabelFailureWords('secret', 'ses_a1', 'session ses_a1 carries no secret label, so there is nothing to remove'),
    'The secret label is already off ses_a1. Nothing was changed.',
  )
  assert.match(
    unlabelFailureWords('secret', 'ses_a1', 'removing a content label from guest does not count: a shared place. The session keeps its labels.'),
    /^That removal was refused: /,
  )
  assert.match(unlabelFailureWords('secret', 'ses_a1', 'the link dropped'), /^That removal did not happen: the link dropped$/)
})
