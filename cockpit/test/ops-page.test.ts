// The Operations page's wiring (theseus-jid7t.3): React, which this runner does not render, so its sources are read as
// text, as test/navgroups.test.ts does. The key table drives the page's handler and its overlay; every act goes
// through `enabled`; the page reads on pushes, never on a timer of its own.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import { KEYS } from '../src/ops/keys.ts'

const src = (f: string) => readFileSync(new URL(`../src/${f}`, import.meta.url), 'utf8')
const page = src('views/Ops.tsx')
const parts = readdirSync(new URL('../src/ops/', import.meta.url)).filter((f) => f.endsWith('.tsx')).map((f) => [f, src(`ops/${f}`)] as const)

test('every key in the table has a handler in the page, and the page draws the overlay from the table', () => {
  const handlers = page.slice(page.indexOf('const handlers: Record<Verb'), page.indexOf('// end of handlers'))
  assert.ok(handlers.length > 200, 'the page keeps its handlers in one table')
  for (const k of KEYS) assert.match(handlers, new RegExp(`\\n\\s+${k.verb}: `), k.verb)
  assert.match(page, /let hit = resolve\(name, c, false\)/)
  assert.match(page, /if \(typingIn\(e\.target/)
  assert.match(src('ops/KeysOverlay.tsx'), /overlay\(\)\.map/)
})

test('every act on the page goes through enabled: no bare call, no useAct but the one wrapper', () => {
  for (const [f, s] of [['Ops.tsx', page] as const, ...parts]) {
    const code = s.replace(/\/\/.*$/gm, '')
    assert.ok(!/\buseAct\(/.test(code), `${f} acts around enabled`)
    if (f !== 'NewTask.tsx' && f !== 'Ops.tsx') assert.ok(!/\bcall\(/.test(code), `${f} calls the daemon directly`)
    assert.ok(!/<Btn\b/.test(code), `${f} uses a button that ignores the time machine`)
  }
  // act.ts, a .ts, holds the one useAct. The two direct calls: the New task form's start (guarded by on('new_task')) and the report lines' read.
  assert.match(src('ops/NewTask.tsx'), /if \(!on\('new_task'\)\) return/)
  assert.match(page, /call<\{ nodes: \{ kind: string; text\?: string \}\[\] \}>\('session\.history'/)
  assert.match(src('ops/act.ts'), /enabled\(verb, past\) \? run\(/)
})

test('the page reads on pushes: no timer that asks, no refetch interval', () => {
  for (const [f, s] of [['Ops.tsx', page] as const, ...parts]) {
    assert.ok(!/setInterval|refetchInterval/.test(s.replace(/\/\/.*$/gm, '')), `${f} polls`)
  }
  // The pushed reads ignore their interval (lib/rpc.ts's PUSHED); the lens's thread panel reads once and on the push.
  for (const m of ['task.list', 'confirm.list', 'session.list']) assert.match(page, new RegExp(`useRpc<[^>]+>\\('${m.replace('.', '\\.')}'`))
  assert.match(src('ops/ThreadPanel.tsx'), /'thread\.show', \{ slug \}, live \? 0 : 5000\)/)
})

test('the page is routed, on the rail second after the Ship, and in the time machine’s folds', () => {
  assert.match(src('main.tsx'), /path: 'ops', lazy: async \(\) => \(\{ Component: \(await import\('\.\/views\/Ops'\)\)\.default \}\)/)
  assert.match(src('components/Shell.tsx'), /\{ to: '\/ship', label: 'Ship', icon: Sailboat \},\n {2}\{ to: '\/ops', label: 'Ops', icon: ShipWheel \},/)
  assert.match(src('lib/navgroups.ts'), /items: \['\/ship', '\/ops',/)
  assert.match(src('lib/world.ts'), /'\/ops'/)
  // The Ship stays the landing view (the owner's decision 2).
  assert.match(src('main.tsx'), /\{ index: true, element: <Navigate to="\/ship" replace \/> \}/)
})

test('a deep link survives the queue’s first read, and the Threads lens matches its threads by slug', () => {
  // The queue is empty until its reads land: only an item that was there and left moves the cursor.
  assert.match(page, /if \(isItemKey\(a\.sel\) && was\.includes\(a\.sel!\) && !keys\.includes\(a\.sel!\)\)/)
  // The lens groups the board itself: a thread is the same thread by its slug, not by its object.
  assert.match(page, /matches=\{\(t\) => shownSlugs\.has\(t\.group\.slug\)\}/)
})

test('the keys answer only the selected card’s own item: no view or no session matches nothing', () => {
  // A row with no live view and an item with no view compared undefined === undefined, so y/n/t/a/e answered an
  // unrelated approval; an item with no session matched a wake row with none (the ops-shell review's F1, F2).
  assert.match(page, /row\.view != null && i\.view\?\.id === row\.view\.id/)
  assert.match(page, /row\.sessionId != null && i\.sessionId === row\.sessionId/)
  assert.match(page, /const focusRow = row \?\? \(item\?\.sessionId \? rows\.find/)
})

test('a page left alone renders nothing new: its lists hold still between renders', () => {
  // The router's asks were a fresh array every render, so the queue's keys changed every render and the arrival
  // effect set state every render: an endless loop that starved every navigation (the lens switch did nothing).
  assert.match(src('ops/asks.ts'), /return useMemo\(\(\) => asks\.filter\(\(x\) => x\.ask\.expires_ms > now\), \[asks, now\]\)/)
  assert.match(page, /const came = arrived\(was, queue\)\n\s+if \(came\.length\) setFresh\(new Set\(came\)\)/)
  assert.match(page, /const NONE: never\[\] = \[\]/)
})

test('the Graph lens and the bulk acts (ops-graph): mounted in its branch, fanned out through enabled, esc clears the picks first', () => {
  assert.match(page, /a\.lens === 'graph' && <GraphLens /)
  assert.ok(!/TaskGraphView/.test(page), 'the old task graph lives on in Actions only')
  // A bulk call and a drop go through `send`, which refuses under the time machine before it calls.
  assert.match(src('ops/act.ts'), /if \(!enabled\(verb, past\)\) return \{ ok: false, words: PAST_WORDS \}/)
  assert.match(src('ops/BulkBar.tsx'), /fanOut\(items\.filter\(\(i\) => i\.method\), \(i\) => send\(`bulk_\$\{verb\}`, i\.method!, i\.params\), 4\)/)
  assert.match(src('ops/DropLayer.tsx'), /await send\('drop', method, params\)/)
  // Approve always opens the review sheet; a floor approval is listed, never sent.
  assert.match(src('ops/BulkBar.tsx'), /else if \(verb === 'approve'\) setAsking\(\{ verb, items: o\.items, floor: o\.oneByOne \?\? \[\]/)
  assert.match(src('ops/BulkBar.tsx'), /asks one by one/)
  assert.match(page, /back: \(\) => \{ if \(picked\.size\) unpick\(\); else/)
  assert.match(page, /if \(e\.altKey \|\| \(\(e\.metaKey \|\| e\.ctrlKey\) && \(keyName\(e\) !== 'mod\+a' \|\| !live\.current\.picks\)\)\) return/)
  assert.match(page, /select_all: \(\) => pickAll\(vis\.rows\.map\(\(r\) => r\.id\)\)/)
  // A selection box on each Board card, and the failed items stay picked.
  assert.match(src('ops/BoardLens.tsx'), /<input type="checkbox" checked=\{picked\} onChange=\{\(\) => onPick\(r\.id\)\}/)
  assert.match(src('ops/BulkBar.tsx'), /if \(keep\) keepFailed\(results\)/)
  // The flowing edge follows stir's rule and Calm's.
  const css = readFileSync(new URL('../src/index.css', import.meta.url), 'utf8')
  assert.match(css, /html:not\(\.stirred\) \.ops-flow \.react-flow__edge-path \{ animation: none !important; \}/)
  assert.match(css, /html\.calm \.ops-flow \.react-flow__edge-path, html\.calm \.ops-fade-in \{ animation: none !important; \}/)
})
