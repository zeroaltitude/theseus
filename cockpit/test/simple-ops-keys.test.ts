// The keys on the simple Ops page (GitHub issue 17, "Keys"): the Operations page's key table is the only table. The
// page's handler names exactly the entries it has a place for, each key means what it means on the Ship's page, and the
// help lists those keys and no others, in the table's own words. React is not rendered here, so the handler's source is
// read as text, and the keys are resolved by the same pure function the page calls, over the fixtures both pages read.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { KEYS, overlay, resolve, type Ctx } from '../src/ops/keys.ts'
import { focusOf, helpHere, isSimple, resolveHere, SIMPLE_VERBS } from '../src/simple/ops/keys.ts'
import { stepBy, workSetOf } from '../src/simple/ops/workset.ts'
import { store } from './simple-ops-fixtures.ts'

const src = (f: string) => readFileSync(new URL(`../src/${f}`, import.meta.url), 'utf8')
const hook = src('simple/ops/useOpsKeys.ts')
const i = store()
const w = workSetOf(i)
const at = (sel: string) => focusOf(w, i.sessions, sel)
const press = (key: string, sel: string) => resolveHere(key, at(sel), false)?.verb ?? null

test('every key the page claims is an entry of the Operations page’s table, and has a handler; every handler is claimed', () => {
  const table = new Set<string>(KEYS.map((k) => k.verb))
  for (const v of SIMPLE_VERBS) assert.ok(table.has(v), `${v} is not in ops/keys.ts`)
  const body = hook.slice(hook.indexOf('return {\n        down:'), hook.indexOf('const onKey'))
  assert.ok(body.length > 500, 'the handlers are one table in the hook')
  const named = [...body.matchAll(/^ {8}([a-z_]+): /gm)].map((m) => m[1])
  assert.deepEqual([...named].sort(), [...SIMPLE_VERBS].sort(), 'the handler table and SIMPLE_VERBS name the same verbs')
  assert.match(hook, /resolveHere\(name, f, false\)/)
  assert.match(hook, /typingIn\(e\.target/)
})

/** The entries of the Operations page's table that have no place on this page, on purpose: the lenses, the graph's and
 *  the board's picks, the Timeline's marks, the filters, the dock, the folds, the Ship's links, the palette. A new entry
 *  of `ops/keys.ts` is in neither list until someone decides, and the test below fails until they do. */
const NOT_HERE = [
  'left', 'right', 'follow', 'dock_pause', 'lens_graph', 'lens_board', 'lens_timeline', 'home', 'search', 'fold_queue', 'fold_inspector', 'fold_dock', 'palette', 'self_changes', 'filter_needs', 'filter_working', 'filter_paused', 'filter_done', 'filter_all', 'zoom_day', 'zoom_week', 'redact', 'week_prev', 'week_next', 'mark_next', 'mark_prev', 'mark_snooze', 'mark_pause', 'mark_run', 'mark_cancel', 'mark_open', 'select', 'full_window', 'select_all',
]

test('every entry of the Operations page’s table has a verdict here: it works on this page, or it is left out on purpose', () => {
  const decided = new Set<string>([...SIMPLE_VERBS, ...NOT_HERE])
  for (const k of KEYS) assert.ok(decided.has(k.verb), `${k.verb} (${k.keys.join(' ')}) is new in ops/keys.ts: add it to SIMPLE_VERBS with a handler, or to NOT_HERE`)
  for (const v of NOT_HERE) assert.ok(KEYS.some((k) => k.verb === v), `${v} is no longer in the table`)
  for (const v of NOT_HERE) assert.ok(!(SIMPLE_VERBS as readonly string[]).includes(v), `${v} is on both lists`)
})

test('the page takes no key table of its own: it imports the Operations page’s and copies no entry', () => {
  const mine = ['simple/ops/keys.ts', 'simple/ops/useOpsKeys.ts', 'simple/ops/KeysHelp.tsx'].map(src).join('\n').replace(/\/\/.*$/gm, '')
  assert.match(mine, /from '\.\.\/\.\.\/ops\/keys(\.ts)?'/)
  assert.ok(!/keys: \[/.test(mine), 'no entry of the table is written again')
  assert.ok(!/words: '/.test(mine), 'no words of the table are written again')
})

test('the help is the table’s overlay, kept to the keys that work here, in the table’s own words', () => {
  const help = helpHere()
  const ship = overlay()
  for (const g of help) {
    const theirs = ship.find((x) => x.ctx === g.ctx)!
    for (const l of g.lines) {
      assert.ok(isSimple(l.verb), `${l.verb} listed but not handled`)
      assert.deepEqual(l, theirs.lines.find((x) => x.verb === l.verb), `${l.verb}: the table’s keys and words, untouched`)
    }
  }
  const listed = new Set(help.flatMap((g) => g.lines.map((l) => l.verb)))
  for (const v of SIMPLE_VERBS) assert.ok(listed.has(v), `${v} works and is not in the help`)
  assert.equal(listed.size, SIMPLE_VERBS.length, 'nothing else is listed')
  // What the page has no place for is not promised: the lenses, the graph's picks, the Timeline's marks, the filters, the dock.
  for (const v of ['lens_graph', 'lens_board', 'select', 'select_all', 'zoom_day', 'filter_needs', 'dock_pause', 'palette', 'fold_queue']) assert.ok(!listed.has(v as never), `${v} is listed`)
  const page = src('simple/ops/KeysHelp.tsx')
  assert.match(page, /helpHere\(\)\.map/)
  assert.match(src('simple/Ops.tsx'), /<KeysHelp onClose=/)
  assert.match(src('simple/Ops.tsx'), /Keys<\/button>/, 'a visible way to the help')
})

test('moving: j and k, the arrows and tab walk the lists; esc clears; none of it acts', () => {
  const first = w.order[0]
  assert.equal(press('j', first), 'down')
  assert.equal(press('ArrowDown', first), 'down')
  assert.equal(press('k', first), 'up')
  assert.equal(press('Tab', first), 'next')
  assert.equal(press('shift+Tab', first), 'prev')
  assert.equal(press('Escape', first), 'back')
  assert.equal(press('Enter', 'ses_t2'), 'inspect')
  assert.equal(press('o', 'ses_t2'), 'open')
  for (const verb of ['down', 'up', 'next', 'prev', 'back', 'inspect', 'open'] as const) assert.ok(!KEYS.find((k) => k.verb === verb)!.act, `${verb} moves or looks`)
  // j walks the order the lists show, to its ends and no further.
  assert.equal(stepBy(w.order, first, 1), w.order[1])
  assert.equal(stepBy(w.order, w.order.at(-1)!, 1), w.order.at(-1))
  assert.equal(stepBy(w.order, first, -1), first)
  assert.equal(stepBy(w.order, null, 1), first)
  assert.equal(stepBy(w.order, null, -1), w.order.at(-1))
  assert.equal(stepBy([], null, 1), null)
  let at2: string | null = null
  const walked: string[] = []
  for (let n = 0; n < w.order.length; n++) { at2 = stepBy(w.order, at2, 1); walked.push(at2!) }
  assert.deepEqual(walked, w.order, 'j from nothing reaches every entry once')
})

test('answering: y and n on an approval, a choice on a question, a retry or a dismiss on a failure; a key means what it means on the Ship', () => {
  assert.equal(press('y', 'q:act_c1'), 'approve')
  assert.equal(press('n', 'q:act_c1'), 'decline')
  assert.equal(press('1', 'q:act_q1'), 'choice')
  assert.equal(press('2', 'q:act_q1'), 'choice')
  assert.equal(press('r', 'f:exe_t5'), 'retry')
  assert.equal(press('d', 'f:exe_t5'), 'dismiss')
  assert.equal(press('r', 'ses_t2'), null, 'r filters on the Ship; here it is nothing, off a failure')
  assert.equal(press('d', 'ses_t2'), null)
  assert.equal(press('y', 'ses_t2'), null, 'nothing to approve on a running task')
  // A card's own ask answers when the card is the choice, as on the Ship: the first ask that waits on it.
  assert.equal(press('1', 'ses_t1'), 'choice', 'the task that waits on a question')
  for (const verb of ['approve', 'decline', 'choice', 'retry', 'dismiss'] as const) assert.ok(KEYS.find((k) => k.verb === verb)!.act, `${verb} acts`)
  // The same answer as the Ship's key, key for key, over every ask and card of the store.
  for (const sel of [...w.queue.map((q) => q.key), ...w.rows.map((r) => r.id)]) {
    const f = at(sel)
    for (const k of ['y', 'n', '1', 'a', 'e', 'r', 'd', 's', 'S', 'c', 'u', 'P', 'x', 'm', 'M', 'i', 'p', 'I']) {
      const here = resolveHere(k, f, false)?.verb
      const ship = resolve(k, f.ctxs as ReadonlySet<Ctx>, false)?.verb
      if (here) assert.equal(here, ship, `${k} on ${sel}`)
    }
  }
})

test('steering: s twice stops the running turn, S asks, P pauses a task, c twice cancels it, m and M open the menus, i writes', () => {
  assert.equal(press('s', 'ses_tides'), 'stop')
  assert.equal(press('S', 'ses_tides'), 'stop_all')
  assert.equal(press('s', 'ses_t2'), null, 'a task is not a running conversation')
  assert.equal(press('P', 'ses_t2'), 'pause')
  assert.equal(press('c', 'ses_t2'), 'cancel')
  assert.equal(press('u', 'ses_t2'), 'not_this')
  assert.equal(press('m', 'ses_tides'), 'model_menu')
  assert.equal(press('M', 'ses_tides'), 'mode_menu')
  assert.equal(press('i', 'ses_tides'), 'compose')
  assert.equal(press('p', 'ses_t2'), 'parent')
  assert.equal(press('I', 'ses_tides'), 'last_turn')
  assert.equal(press('x', 'wak_r1'), 'cancel_wake')
  assert.equal(press('N', 'ses_t2'), 'new_task')
  assert.equal(press('?', 'ses_t2'), 'keys')
  for (const verb of ['stop', 'stop_all', 'pause', 'cancel', 'not_this', 'cancel_wake', 'new_task', 'model_menu', 'mode_menu'] as const) assert.ok(KEYS.find((k) => k.verb === verb)!.act, `${verb} acts: off under the time machine`)
  assert.ok(KEYS.find((k) => k.verb === 'stop')!.twice && KEYS.find((k) => k.verb === 'cancel')!.twice, 'the dangerous ones need a second press')
  // Each acts through the Ship's own call.
  for (const call of ["'execution.stop', { execution_id: f.exe }", "'task.cancel', { task: row.id }", "'task.not_this', { task: row.id }", "row.paused ? 'task.resume' : 'task.pause', { task: row.id }", "'wake.cancel', { wake: row.id }"]) assert.ok(hook.includes(call), call)
  assert.match(hook, /pressTwice\(armed\.current, verb, Date\.now\(\)\)/)
  assert.match(hook, /useOpsAct\(\)/)
})

test('nothing fires while a person types, and a key the Ship has and this page has not does nothing', () => {
  for (const k of ['j', 'y', 's', '?', 'Escape']) assert.equal(resolveHere(k, at('ses_tides'), true), null, k)
  for (const k of ['g', 'b', 'w', ' ', 'f', 'L', 'T', '/', '[', ']', '\\', 'h', 'l', ',', '.']) assert.equal(press(k, 'ses_tides'), null, `${k} is not claimed here`)
  const typing = hook.slice(hook.indexOf('const onKey'))
  assert.match(typing, /if \(typingIn\(e\.target as HTMLElement \| null\)\) return/)
  assert.match(typing, /if \(q\.modal\) return/, 'a sheet or a menu that is open has the keys')
  assert.match(typing, /if \(e\.altKey \|\| e\.metaKey \|\| e\.ctrlKey\) return/, 'a browser chord is the browser’s')
})

test('a grant is its offer’s own key: a for a command, e for edits, the other falls through', () => {
  const g = workSetOf({
    ...i,
    confirms: i.confirms.map((c) => (c.correlation_id === 'act_c1' ? { ...c, grant_offer: { kind: 'edits', words: 'yes, and don’t ask again here' } as never } : c)),
  })
  const f = focusOf(g, i.sessions, 'q:act_c1')
  assert.equal(resolveHere('e', f, false)?.verb, 'grant')
  assert.equal(resolveHere('a', f, false)?.verb ?? null, null, 'a is not an edits offer’s key, and a filter is not a thing here')
})
