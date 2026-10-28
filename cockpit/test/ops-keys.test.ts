// The Operations page's keys and regions (theseus-jid7t.3): one table drives the handler and the `?` overlay; a key
// means the most specific thing the focus offers; nothing fires while typing; under the time machine every act is off.
import test from 'node:test'
import assert from 'node:assert/strict'
import { ACTS, BUTTON_ACTS, enabled, keyName, KEYS, overlay, PRIORITY, pressTwice, resolve, SEQUENCES, TWICE_MS, typingIn, type Ctx } from '../src/ops/keys.ts'
import { flip, parseFold, regionsFor } from '../src/ops/regions.ts'

const on = (...c: Ctx[]) => new Set<Ctx>(c)
const allKeys = [...new Set(KEYS.flatMap((k) => k.keys))]

test('no two entries claim one key in one context, and every context is in the priority order', () => {
  const seen = new Map<string, string>()
  for (const k of KEYS) {
    assert.ok(PRIORITY.includes(k.ctx), `${k.verb}: ${k.ctx}`)
    for (const key of k.keys) {
      const at = `${k.ctx} ${key}`
      assert.ok(!seen.has(at), `${key} in ${k.ctx}: ${seen.get(at)} and ${k.verb}`)
      seen.set(at, k.verb)
    }
  }
  assert.equal(new Set(KEYS.map((k) => k.verb)).size, KEYS.length, 'one entry per verb')
})

test('every key in the table has a line in the overlay, and the overlay says how conflicts resolve', () => {
  const lines = overlay().flatMap((g) => g.lines)
  assert.deepEqual(lines.map((l) => l.verb).sort(), KEYS.map((k) => k.verb).sort())
  for (const g of overlay()) assert.ok(g.heading.length > 3, g.ctx)
  const say = (verb: string) => lines.find((l) => l.verb === verb)!
  assert.match(say('filter_paused').words, /on a failure, r retries/)
  assert.match(say('filter_done').words, /on a failure, d dismisses/)
  assert.match(say('filter_all').words, /grant/)
  assert.equal(say('stop').keys, 's ×2')
  assert.equal(say('cancel').keys, 'c ×2')
  assert.equal(say('next').keys, 'tab')
  assert.equal(say('prev').keys, '⇧tab')
  assert.deepEqual([...SEQUENCES].sort(), ['g b', 'g g', 'g h', 'g m', 'g t'])
})

test('a key means the most specific thing the focus offers, as in the TUI', () => {
  assert.equal(resolve('r', on('card', 'failure', 'task'), false)?.verb, 'retry')
  assert.equal(resolve('r', on('card', 'task'), false)?.verb, 'filter_paused', 'no failure: a filter')
  assert.equal(resolve('d', on('card', 'failure'), false)?.verb, 'dismiss')
  assert.equal(resolve('d', on(), false)?.verb, 'filter_done')
  assert.equal(resolve('a', on('answer', 'grant'), false)?.verb, 'grant')
  assert.equal(resolve('a', on('answer'), false)?.verb, 'filter_all', 'no grant offered: show everything')
  assert.equal(resolve('e', on('answer'), false), null)
  assert.equal(resolve('l', on('card', 'job'), false)?.verb, 'follow', 'l follows a job only on a job row')
  assert.equal(resolve('l', on('card', 'task'), false)?.verb, 'right')
  assert.equal(resolve('y', on('card'), false), null, 'nothing to approve')
  assert.equal(resolve('y', on('card', 'answer'), false)?.verb, 'approve')
  assert.equal(resolve('3', on('card', 'choice'), false)?.verb, 'choice')
  assert.equal(resolve('3', on('card', 'answer'), false), null)
  assert.equal(resolve('x', on('card', 'wake'), false)?.verb, 'cancel_wake')
  assert.equal(resolve('g b', on(), false)?.verb, 'lens_board')
  assert.equal(resolve('shift+Tab', on(), false)?.verb, 'prev')
  assert.equal(keyName({ key: 'Tab', shiftKey: true }), 'shift+Tab')
  assert.equal(keyName({ key: 'S', shiftKey: true }), 'S')
})

test('keys never fire while typing', () => {
  for (const key of allKeys) assert.equal(resolve(key, on(...PRIORITY), true), null, key)
  assert.ok(typingIn({ tagName: 'INPUT' }) && typingIn({ tagName: 'TEXTAREA' }) && typingIn({ tagName: 'SELECT' }) && typingIn({ tagName: 'DIV', isContentEditable: true }))
  assert.ok(!typingIn({ tagName: 'BUTTON' }) && !typingIn(null))
})

test('under the time machine every act is off, keys and buttons alike; looking still works', () => {
  for (const k of KEYS) {
    assert.equal(enabled(k.verb, false), true, `${k.verb} live`)
    assert.equal(enabled(k.verb, true), !k.act, `${k.verb} in the past`)
  }
  for (const b of BUTTON_ACTS) {
    assert.equal(enabled(b, true), false, b)
    assert.equal(enabled(b, false), true, b)
  }
  for (const v of ['approve', 'decline', 'choice', 'retry', 'dismiss', 'pause', 'stop', 'stop_all', 'cancel', 'cancel_wake', 'new_task', 'grant']) assert.ok(ACTS.has(v), v)
  for (const v of ['down', 'lens_board', 'search', 'inspect', 'open', 'keys', 'filter_needs']) assert.ok(!ACTS.has(v), v)
})

test('a twice key fires on its second press within the window, and only then', () => {
  let s = pressTwice(null, 'stop', 1000)
  assert.equal(s.fire, false)
  s = pressTwice(s.armed, 'stop', 1000 + TWICE_MS)
  assert.equal(s.fire, true)
  assert.equal(s.armed, null)
  s = pressTwice(pressTwice(null, 'stop', 0).armed, 'stop', TWICE_MS + 1)
  assert.equal(s.fire, false, 'too slow: it arms again')
  s = pressTwice(pressTwice(null, 'stop', 0).armed, 'cancel', 10)
  assert.equal(s.fire, false, 'another verb arms that one')
  assert.equal(s.armed?.verb, 'cancel')
})

test('the regions: folds kept, the inspector over the canvas below 1280 px, the queue the page below 900', () => {
  assert.equal(parseFold(null), 'open')
  assert.equal(parseFold('folded'), 'folded')
  assert.equal(parseFold('nonsense'), 'open')
  assert.equal(flip('open'), 'folded')
  assert.deepEqual(regionsFor(1440, 'open', 'open', false), { queue: 'side', inspector: 'idle', canvas: true }, 'nothing selected: the canvas takes the width')
  assert.deepEqual(regionsFor(1440, 'open', 'open', true), { queue: 'side', inspector: 'side', canvas: true })
  assert.deepEqual(regionsFor(1440, 'folded', 'folded', true), { queue: 'folded', inspector: 'folded', canvas: true })
  assert.deepEqual(regionsFor(1100, 'open', 'open', true), { queue: 'side', inspector: 'over', canvas: true })
  assert.deepEqual(regionsFor(1100, 'open', 'open', false), { queue: 'side', inspector: 'none', canvas: true })
  assert.deepEqual(regionsFor(800, 'folded', 'open', false), { queue: 'page', inspector: 'none', canvas: false }, 'a narrow screen: the queue is the page, whatever the fold')
  assert.deepEqual(regionsFor(800, 'open', 'open', true), { queue: 'page', inspector: 'over', canvas: false })
})

test('the Timeline’s keys (ops-timeline): the lens and a focused mark are more specific than the counters and the page', () => {
  // In the lens, d and w zoom, and beat the counter filters; elsewhere they still filter.
  assert.equal(resolve('d', on('timeline'), false)?.verb, 'zoom_day')
  assert.equal(resolve('w', on('timeline'), false)?.verb, 'zoom_week')
  assert.equal(resolve('d', on(), false)?.verb, 'filter_done')
  assert.equal(resolve('w', on(), false)?.verb, 'filter_working')
  // A failure is more specific still: d dismisses it even in the lens.
  assert.equal(resolve('d', on('failure', 'timeline'), false)?.verb, 'dismiss')
  assert.equal(resolve('R', on('timeline'), false)?.verb, 'redact')
  assert.equal(resolve(',', on('timeline'), false)?.verb, 'week_prev')
  assert.equal(resolve('.', on('timeline'), false)?.verb, 'week_next')
  // j and k step the marks in the lens, the queue elsewhere.
  assert.equal(resolve('j', on('timeline'), false)?.verb, 'mark_next')
  assert.equal(resolve('k', on('timeline'), false)?.verb, 'mark_prev')
  assert.equal(resolve('j', on(), false)?.verb, 'down')
  // On a focused mark, the Horizon's keys: r runs, not the paused filter; x cancels the mark; Enter opens it.
  for (const [key, verb] of [['z', 'mark_snooze'], ['p', 'mark_pause'], ['r', 'mark_run'], ['x', 'mark_cancel'], ['Enter', 'mark_open']] as const) {
    assert.equal(resolve(key, on('mark', 'timeline'), false)?.verb, verb, key)
  }
  assert.equal(resolve('r', on('timeline'), false)?.verb, 'filter_paused', 'no mark: r filters')
  assert.equal(resolve('z', on('timeline'), false), null)
  const lines = overlay().flatMap((g) => g.lines)
  assert.match(lines.find((l) => l.verb === 'zoom_day')!.words, /beats the done filter/)
  assert.match(lines.find((l) => l.verb === 'mark_run')!.words, /beats the paused filter/)
  for (const v of ['mark_snooze', 'mark_pause', 'mark_run', 'mark_cancel', 'event_remind', 'event_prep', 'event_ask', 'drag', 'undo']) assert.ok(ACTS.has(v), `${v} is off under the time machine`)
  for (const v of ['zoom_day', 'redact', 'week_next', 'mark_next', 'mark_open']) assert.ok(!ACTS.has(v), `${v} only looks`)
})

test('ops-act’s keys: m and M and i and I on a session, p on a task, the dock’s space first, and the sequences stay the g ones', () => {
  assert.equal(resolve('m', on('card', 'session'), false)?.verb, 'model_menu')
  assert.equal(resolve('M', on('card', 'session'), false)?.verb, 'mode_menu')
  assert.equal(resolve('i', on('card', 'session'), false)?.verb, 'compose')
  assert.equal(resolve('I', on('card', 'session'), false)?.verb, 'last_turn')
  assert.equal(resolve('m', on('card'), false), null, 'no session, no menu')
  assert.equal(resolve('p', on('card', 'task'), false)?.verb, 'parent')
  assert.equal(resolve('Backspace', on('card', 'task'), false)?.verb, 'parent')
  assert.equal(resolve(' ', on('dock', 'card', 'session'), false)?.verb, 'dock_pause', 'space pauses the dock only while it has focus')
  assert.equal(resolve(' ', on('card', 'session'), false)?.verb, 'select', 'elsewhere space is the selection’s (ops-graph)')
  assert.equal(resolve('\\', on(), false)?.verb, 'fold_dock')
  for (const k of ['PageUp', 'PageDown', 'End', 'L', 'T']) assert.ok(resolve(k, on(), false), k)
  const lines = overlay().flatMap((g) => g.lines)
  assert.equal(lines.find((l) => l.verb === 'palette')!.keys, '⌘K')
  assert.equal(lines.find((l) => l.verb === 'dock_pause')!.keys, '␣')
  assert.ok(ACTS.has('model_menu') && ACTS.has('mode_menu') && !ACTS.has('compose'), 'the menus act; composing is a form (join is a button act)')
  assert.ok(BUTTON_ACTS.includes('join' as never) && enabled('join', true) === false)
})

test('the selection’s keys (ops-graph): ␣ picks the focused card, ⌘A and ctrl+a pick everything shown, esc clears first', () => {
  assert.equal(resolve(' ', on('card'), false)?.verb, 'select')
  assert.equal(resolve(' ', on(), false), null, 'nothing focused: nothing to pick')
  assert.equal(resolve('mod+a', on(), false)?.verb, 'select_all')
  assert.equal(keyName({ key: 'a', shiftKey: false, metaKey: true }), 'mod+a')
  assert.equal(keyName({ key: 'a', shiftKey: false, ctrlKey: true }), 'mod+a')
  assert.equal(keyName({ key: 'a', shiftKey: false }), 'a', 'a bare a is still the filter')
  assert.equal(resolve('a', on(), false)?.verb, 'filter_all')
  const lines = overlay().flatMap((g) => g.lines)
  assert.equal(lines.find((l) => l.verb === 'select')!.keys, '␣')
  assert.equal(lines.find((l) => l.verb === 'select_all')!.keys, '⌘A')
  assert.match(lines.find((l) => l.verb === 'back')!.words, /^back: clear the picked cards first/)
  for (const k of [' ', 'mod+a']) assert.equal(resolve(k, on(...PRIORITY), true), null, `${k} while typing`)
  // The bulk bar's verbs and a drop are buttons that act: off under the time machine.
  for (const b of ['bulk_pause', 'bulk_stop', 'bulk_cancel', 'bulk_rehome', 'bulk_approve', 'bulk_decline', 'bulk_attach', 'drop'] as const) {
    assert.ok((BUTTON_ACTS as readonly string[]).includes(b), b)
    assert.equal(enabled(b, true), false, b)
  }
  assert.ok(!SEQUENCES.includes(' '), 'the space bar is a key, not a sequence')
  // A Board card's pick box keeps the page's keys once clicked: esc still clears, ␣ still picks.
  assert.ok(!typingIn({ tagName: 'INPUT', type: 'checkbox' }))
  assert.ok(typingIn({ tagName: 'INPUT', type: 'text' }) && typingIn({ tagName: 'INPUT' }) && typingIn({ tagName: 'INPUT', type: 'search' }))
})
