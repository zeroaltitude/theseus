// The Operations page's address (theseus-jid7t.3): a link reopens exactly what was on screen, and the three old pages'
// addresses go to their lens with their query kept.
import test from 'node:test'
import assert from 'node:assert/strict'
import { DEFAULT_LENS, opsHref, parseAddress, parseFilters, redirectOf, writeAddress, writeFilters, type Address } from '../src/ops/address.ts'

test('each old page redirects to its lens and keeps ?w= and ?sel=', () => {
  assert.equal(redirectOf('/agenda', '?w=-1&sel=routine%3Ases_a%3A1700000000000'), '/ops?lens=timeline&w=-1&sel=routine%3Ases_a%3A1700000000000')
  assert.equal(redirectOf('/tasks', '?sel=ses_t1'), '/ops?lens=board&sel=ses_t1')
  assert.equal(redirectOf('/threads', ''), '/ops?lens=threads')
  assert.equal(redirectOf('/threads/', 'w=2'), '/ops?lens=threads&w=2', 'a trailing slash, and a query with no ?')
  // A lens the old address carried is the old page's, not its own.
  assert.equal(redirectOf('/tasks', '?lens=graph&q=quay'), '/ops?lens=board&q=quay')
  for (const p of ['/threads/harbour-survey', '/ops', '/actions', '/', '/agendas']) assert.equal(redirectOf(p, '?w=1'), null, p)
  // What the redirect writes, the page reads.
  const a = parseAddress(redirectOf('/agenda', '?w=3&sel=wak_9')!.split('?')[1])
  assert.equal(a.lens, 'timeline')
  assert.equal(a.w, 3)
  assert.equal(a.sel, 'wak_9')
})

test('an empty address is the board, nothing selected, nothing filtered', () => {
  assert.deepEqual(parseAddress(''), { lens: DEFAULT_LENS, sel: null, f: {}, q: '', w: 0, join: null, redact: false, do: null })
  assert.equal(DEFAULT_LENS, 'board')
  assert.equal(parseAddress('lens=nonsense').lens, 'board')
  assert.equal(parseAddress('w=999').w, 52, 'the Agenda’s own bound')
  assert.equal(parseAddress('w=x').w, 0)
  assert.equal(parseAddress('redact=yes').redact, false, 'only 1 redacts')
  assert.equal(parseAddress('do=other').do, null)
})

test('a link reopens what was on screen: every key round-trips, foreign keys are kept', () => {
  const shown: Address[] = [
    { lens: 'threads', sel: 'harbour-survey', f: { thread: 'harbour-survey', state: 'running' }, q: 'quay walls', w: 0, join: null, redact: true, do: null },
    { lens: 'board', sel: 'ses_t1', f: { bucket: 'needs', model: 'sonnet', place: 'discord dm', kind: 'task' }, q: '', w: 0, join: null, redact: false, do: 'answer' },
    { lens: 'timeline', sel: 'routine:ses_a:1700000000000', f: { ended: '1d', project: 'tsk_p1' }, q: '', w: -2, join: 'ses_c1', redact: true, do: null },
    { lens: 'graph', sel: 'tsk_a', f: {}, q: 'a, b: c & d', w: 1, join: null, redact: false, do: null },
  ]
  for (const a of shown) {
    const href = opsHref(a)
    assert.ok(href.startsWith('/ops?lens='), href)
    assert.deepEqual(parseAddress(href.split('?')[1]), a, href)
  }
  // The task graph's own pick and a chart's table survive a write.
  const kept = writeAddress({ ...parseAddress(''), lens: 'graph' }, new URLSearchParams('task=tsk_a&table=x&lens=board&sel=old'))
  assert.equal(kept.get('task'), 'tsk_a')
  assert.equal(kept.get('table'), 'x')
  assert.equal(kept.get('lens'), 'graph')
  assert.equal(kept.get('sel'), null, 'the page’s own key is replaced, not merged')
})

test('filters read defensively and write in one order', () => {
  assert.deepEqual(parseFilters('state:running,nonsense:1,:x,kind:,place:discord%20dm,model:%E0%A4%A'), { state: 'running', place: 'discord dm' })
  assert.equal(writeFilters({ state: 'running', bucket: 'needs' }), writeFilters({ bucket: 'needs', state: 'running' }))
  assert.equal(writeFilters({ place: 'a,b:c' }), 'place:a%2Cb%3Ac')
  assert.deepEqual(parseFilters(writeFilters({ place: 'a,b:c' })), { place: 'a,b:c' })
})
