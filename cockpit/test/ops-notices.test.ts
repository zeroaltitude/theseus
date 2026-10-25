// External-text notices (theseus-g00vv): the holds that make nothing wait are listed apart from the queue, never counted,
// and leave only on the owner's click. Notices.tsx is read as text, as test/ops-page.test.ts reads the page.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dismissAll, dismissReport, holdsOf, noticeFacts, noticeHint, pastNotices, seenCall, seenHolds, trustCall, TRUST_WORDS } from '../src/ops/notices.ts'
import { HOUR, NOW, session } from './ops-fixtures.ts'

const hold = (since: number, over: object = {}) => ({ since_ms: since, tool: 'http.fetch', url: 'https://example.test/tides', node_id: 'nod_9', ...over })
const src = (f: string) => readFileSync(new URL(`../src/ops/${f}`, import.meta.url), 'utf8').replace(/\/\/.*$/gm, '')

test('noticesOf: the holds that do not wait and are not told, oldest first', () => {
  const { notices, waiting } = holdsOf([
    session('ses_b', { external_text: hold(NOW - HOUR) }), session('ses_a', { external_text: hold(NOW - 3 * HOUR) }),
    session('ses_told', { external_text: hold(NOW - 2 * HOUR), external_told: true }), session('ses_waits', { external_text: hold(NOW - 4 * HOUR), external_waits: true }),
    session('ses_gone', { kind: 'task', execution_state: 'complete', external_text: hold(NOW - 5 * HOUR, { via: 'quote' }) }), session('ses_none'),
  ])
  assert.deepEqual(notices.map((n) => n.session_id), ['ses_a', 'ses_b'], 'the told, the waiting, the ended task’s and the unheld are not notices')
  assert.deepEqual(waiting.map((n) => n.session_id), ['ses_waits'])
})

test('in the time machine the holds are read-only notices, an ended session’s left out', () => {
  const hs = [{ session_id: 'ses_x', title: 'x', external_text: hold(NOW - HOUR) }, { session_id: 'ses_y', title: 'y', external_text: hold(NOW - 2 * HOUR) }]
  assert.deepEqual(pastNotices(hs, new Set(['ses_x'])).map((h) => h.session_id), ['ses_y'])
})

test('a notice says what was read, from where, which session and what Trust again does; redact masks the what and the title', () => {
  const h = { session_id: 'ses_q', title: 'Tide chat', external_text: hold(NOW, { via: 'task.report', from_session: 'ses_src', query: 'tide tables' }) }
  const f = noticeFacts(h)
  assert.equal(f.what, '“tide tables”')
  assert.equal(f.how, 'a task’s report')
  assert.equal(f.fromSession, 'ses_src')
  assert.equal(f.nodeId, 'nod_9')
  assert.match(f.trusts, /go back to their postures until it reads external text again; the read and the trust stay in the ledger/)
  const r = noticeFacts(h, true)
  assert.ok(!r.what.includes('tide') && !r.title.includes('Tide'))
  assert.equal(trustCall(h).method, 'policy.trust')
  assert.ok(trustCall(h).ask.includes(TRUST_WORDS))
})

test('"Dismiss all from before notify" is one policy.notice_seen per session, after one question, and nothing else', () => {
  const hs = [{ session_id: 'ses_a', title: 'a', external_text: hold(1) }, { session_id: 'ses_b', title: 'b', external_text: hold(2) }]
  const d = dismissAll(hs)
  assert.equal(d.label, 'Dismiss all from before notify (2)')
  assert.deepEqual(d.calls, [{ method: 'policy.notice_seen', params: { session_id: 'ses_a' } }, { method: 'policy.notice_seen', params: { session_id: 'ses_b' } }])
  assert.match(d.ask, /2 notices/)
  assert.deepEqual(seenCall(hs[0]), d.calls[0])
})

test('Notices.tsx deletes nothing, calls only the two helpers’ methods, and acts on no load', () => {
  const s = src('Notices.tsx')
  assert.ok(!/policy\.|ledger\.|\bdelete\b|forget|erase/i.test(s.replace(/'trust'|'mark'/g, '')), 'no method name of its own, nothing deleted')
  assert.match(s, /seenCall\(h\)/)
  assert.match(s, /trustCall\(h\)/)
  assert.match(s, /dismissAll\(notices\)/)
  assert.ok(!/useEffect/.test(s), 'nothing runs on load')
  assert.match(s, /window\.confirm\(all\.ask\)/)
  const n = src('notices.ts')
  assert.match(n, /method: 'policy\.notice_seen'/)
  assert.match(n, /method: 'policy\.trust'/)
  assert.equal((n.match(/method: '/g) ?? []).length, 2)
})

test('the page counts holds that wait, never notices; the strip is mounted in the queue’s list', () => {
  const page = readFileSync(new URL('../src/views/Ops.tsx', import.meta.url), 'utf8')
  assert.match(page, /const holds = held\.waiting/)
  assert.match(page, /needs: queue\.length/)
  assert.match(page, /<Notices notices=\{notices\}/)
  assert.match(src('Queue.tsx'), /\{notices\}/)
})

test('Dismiss all counts the refusals and says why; a clean run has no line (theseus-jo84p)', () => {
  assert.deepEqual(dismissReport([{ ok: true, words: 'done' }, { ok: true, words: 'done' }]), { done: 2, refused: 0, line: null })
  const r = dismissReport([{ ok: true, words: 'done' }, { ok: false, words: 'its hold waits for you' }, { ok: false, words: 'its hold waits for you' }, { ok: false, words: 'no such session' }])
  assert.equal(r.done, 1)
  assert.equal(r.refused, 3)
  assert.equal(r.line, '3 of 4 not dismissed: its hold waits for you · no such session', 'each reason once')
  assert.equal(dismissReport([]).line, null)
})

test('Dismiss all keeps its button busy while it runs and shows the refusals (theseus-jo84p)', () => {
  const s = src('Notices.tsx')
  assert.match(s, /setSweeping\(true\)/)
  assert.match(s, /finally \{ setSweeping\(false\); settle\(\) \}/)
  assert.match(s, /busy=\{busy === 'notice:all' \|\| sweeping\}/)
  assert.match(s, /dismissReport\(results\)/)
  assert.match(s, /results\.push\(await send\('mark', c\.method, c\.params\)\)/)
})

test('the Bridge hint counts the seen holds apart: N notices (M seen) (theseus-jo84p)', () => {
  const listed = [{ session_id: 'ses_a' }, { session_id: 'ses_b' }, { session_id: 'ses_c' }]
  const sessions = [session('ses_a', { external_told: true }), session('ses_b'), session('ses_c', { external_told: true }), session('ses_x', { external_told: true })]
  assert.equal(seenHolds(listed, sessions), 2, 'only listed holds count; a session not listed does not')
  assert.equal(seenHolds(listed, []), 0)
  assert.equal(noticeHint(3, 2), '3 notices (2 seen): nothing waits on them')
  assert.equal(noticeHint(1, 1), '1 notice (1 seen): nothing waits on it')
  assert.equal(noticeHint(2, 0), '2 notices: nothing waits on them', 'none seen: the old words')
  const bridge = readFileSync(new URL('../src/views/Bridge.tsx', import.meta.url), 'utf8')
  assert.match(bridge, /noticeHint\(heldNotices, heldSeen\)/)
  assert.match(bridge, /seenHolds\(h!?\.external_text/)
})
