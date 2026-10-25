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

test('ops-act: the page mounts the Join pane, dock, menus and toast; no prompt or ?call= link; the palette gets a feed with the selection seam', () => {
  for (const m of ['<Dock ', '<JoinPane ', '<ModelMenu ', '<ModeMenu ', '<Toast />']) assert.ok(page.includes(m), m)
  assert.match(page, /\/\/ ops-graph: the selection scope/)
  assert.match(page, /selection: vis\.rows\.filter\(\(r\) => picked\.has\(r\.id\)\)\.map\(targetOf\), \/\/ ops-graph: the selection scope/)
  assert.match(page, /\}\), \[focusRow, vis\.rows, picked, rows,/)
  assert.ok(!page.includes('?call='), 'l follows in the inspector')
  assert.match(page, /if \(mo\) return \/\/ a sheet or a menu has the keys/)
  for (const [f, s] of parts) assert.ok(!/window\.prompt/.test(s.replace(/\/\/.*$/gm, '')), `${f} asks with a prompt, not a sheet`)
  const join = src('ops/JoinPane.tsx')
  assert.match(join, /composerStep\(cs, e\)/)
  // J1: a menu carries its session: the Join pane's own (a thread's included), never the focused card's.
  assert.match(page, /onMenu=\{\(m, sid\) => setMenu\(\{ kind: m, sid \}\)\}/)
  assert.match(join, /onMenu\('model', sid\)/)
  assert.match(join, /onMenu\('mode', sid\)/)
  assert.match(page, /<ModelMenu session=\{menuSession\}/)
  assert.ok(!/<Mod[e]l?Menu session=\{focusSession\}/.test(page), 'the menus render for the session they carry')
  assert.match(join, /post\('join', 'turn\.submit'/)
  assert.match(src('ops/jobout.ts'), /run\(\{ kind: 'leave' \}\)/)
  assert.match(src('components/Shell.tsx'), /\{opsOn && <OpsPalette /)
})

test('a session is watched once however many panes hold it, and unwatched when the last leaves', () => {
  const w = src('ops/watch.ts')
  assert.match(w, /if \(n === 1\) client\.call\('session\.watch'/)
  assert.match(w, /if \(left > 0\) \{ holders\.set\(sessionId, left\); return \}/)
  assert.ok(!/useSessionWatch/.test(src('ops/JoinPane.tsx')), 'the pane uses the shared watch')
})

test('the Graph lens and the bulk acts (ops-graph): mounted in its branch, fanned out through enabled, esc clears the picks first', () => {
  assert.match(page, /a\.lens === 'graph' && <GraphLens /)
  assert.ok(!/TaskGraphView/.test(page), 'the old task graph lives on in Actions only')
  // A bulk call and a drop go through `send`, which refuses under the time machine before it calls.
  assert.match(src('ops/act.ts'), /if \(!enabled\(verb, past\)\) return \{ ok: false, words: PAST_WORDS \}/)
  assert.match(src('ops/BulkBar.tsx'), /fanOut\(items\.filter\(\(i\) => i\.method\), \(i\) => send\(`bulk_\$\{verb\}`, i\.method!, i\.params\), 4\)/)
  assert.match(src('ops/DropLayer.tsx'), /await send\('drop', method, params\)/)
  // One send for the page (ops-graph x ops-timeline x ops-act): no read again per call, one settle after a run.
  const actTs = src('ops/act.ts')
  assert.equal((actTs.match(/invalidateQueries/g) ?? []).length, 1, 'send never reads again: settle does, once')
  assert.match(actTs, /settle: \(\) => qc\.invalidateQueries\(\)/)
  assert.match(actTs, /const r = await send<T>\(verb, method, params\)/, 'post is send that rejects')
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

test('profile.list and catalog.list are read once and on their invalidations: no interval in src/ops or the page', () => {
  for (const [f, s] of [['Ops.tsx', page] as const, ...parts]) {
    for (const m of s.matchAll(/useRpc<[^>]+>\('(profile|catalog)\.list',\s*undefined,\s*([^,)]+)/g)) assert.equal(m[2].trim(), '0', `${f}: ${m[1]}.list polls every ${m[2]}`)
  }
  assert.match(page, /useRpc<ProfileList>\('profile\.list', undefined, 0\)/, 'the page still reads them')
  // What brings a fresh read: profile.changed (the always-mounted heartbeat), the menu's invalidate, the dock's act.
  assert.match(src('components/Heartbeat.tsx'), /m === 'profile\.changed'\) void qc\.invalidateQueries\(\{ queryKey: \['profile\.list'\]/)
  assert.match(src('ops/ModelMenu.tsx'), /void qc\.invalidateQueries\(\); onClose\(\)/)
})

test('M opens the mode menu on a task as on a conversation: a task has a session, and the mode board is keyed by it', () => {
  const h = page.slice(page.indexOf('mode_menu: () =>'), page.indexOf('compose: () =>'))
  assert.match(h, /focusSession\?\.kind === 'task'/)
  assert.match(src('ops/Inspector.tsx'), /\(r\.kind === 'conversation' \|\| r\.kind === 'task'\) && r\.sessionId && <ActBtn verb="mode_menu"/)
})

test('the model menu sends one session.profile at a time: set returns while a call is out, and the rows are off', () => {
  const m = src('ops/ModelMenu.tsx')
  const set = m.slice(m.indexOf('const set = async'), m.indexOf('return (\n    <Pop'))
  assert.match(set, /if \(out\.current\) return\n\s+out\.current = true/)
  assert.match(set, /finally \{ out\.current = false/)
  assert.match(m, /disabled=\{busy \|\| !!r\.grey \|\| !on\('model_sticky'\)/)
  const revert = m.slice(m.indexOf('const revert ='), m.indexOf('return (\n    <span'))
  assert.match(revert, /if \(out\.current\) return/)
  // useOpsAct().post stays unguarded: the bulk fan-out relies on it.
  assert.doesNotMatch(src('ops/act.ts'), /out\.current/)
})

test('a hold takes the daemon’s clock, never this browser’s', () => {
  assert.doesNotMatch(src('ops/ui.ts').replace(/\/\/.*$/gm, ''), /Date\.now\(\)/)
})

test('the deck’s live turn is the Operations page’s liveOf: no second copy', () => {
  const deck = src('views/SessionDeck.tsx')
  assert.doesNotMatch(deck, /function useLive/)
  assert.match(deck, /import \{ liveOf, type LiveTurn \} from '@\/ops\/live'/)
  assert.match(deck, /useMemo\(\(\) => liveOf\(events, id\), \[events, id\]\)/)
})

test('the palette sees a watch the moment it appears or ends: its feed is recomputed on the board’s views (join fix)', () => {
  const feed = page.slice(page.indexOf('const feed = useMemo('), page.indexOf('// eslint-disable-line', page.indexOf('const feed = useMemo(')))
  assert.match(feed, /watchRows\(views\)/)
  assert.match(feed, /\}\), \[[^\]]*\bviews\b[^\]]*\]\) $/)
})

test('the Graph’s follow-ups are wired: keys, the palette scope, the verb menu, row-backed picks, homes, space, redact', () => {
  const lens = src('ops/GraphLens.tsx').replace(/\/\/.*$/gm, '')
  const board = src('ops/BoardLens.tsx')
  // n17am 1: the keys walk the graph's nodes from the lens's last layout.
  assert.match(page, /stepNode\(graphRef\.current/)
  assert.match(lens, /layoutRef\.current = g/)
  assert.match(lens, /memory\.lay\(/)
  // n17am 2: the palette opens over the picked cards, and over the focused one when none are picked.
  assert.match(page, /useOpsPalette\.setState\(\{ scope: picked\.size > 0 \? 'selection' : 'one' \}\)/)
  // n17am 5: one verb menu, the page's.
  assert.ok(!/function Menu\(/.test(lens) && !/offersFor|onVerb/.test(lens))
  assert.match(lens, /onContext\(n\.id, e\.clientX, e\.clientY\)/)
  assert.match(page, /onContext=\{\(id, x, y\) => setCtxMenu\(\{ id, x, y \}\)\} redact=\{a\.redact\}/)
  // n17am 6: no ended or retired conversation is a home.
  assert.match(page, /r\.sessionId && alive\(r\) && !r\.session\?\.retired/)
  // n17am 7: only a node with a row is picked, by click and by lasso.
  assert.equal((lens.match(/byId\.has\(/g) ?? []).length >= 3, true)
  assert.match(lens, /if \(byId\.has\(n\.id\)\) onPick\(n\.id\)/)
  assert.match(lens, /filter\(\(id\) => byId\.has\(id\)\)/)
  // n17am 8: space picks the pick box that holds the DOM focus.
  assert.match(page, /document\.activeElement\?\.closest<HTMLElement>\('\[data-pick\]'\)/)
  assert.match(board, /<input type="checkbox" checked=\{picked\} onChange=\{\(\) => onPick\(r\.id\)\} data-pick=\{r\.id\}/)
  // n17am 4: the node and lane titles are drawn through shownTitle.
  const node = src('ops/GraphNode.tsx')
  assert.ok(!/title=\{n\.title\}|⚙ \{n\.title\}|>\{n\.title\}<\/div>|\{lane\.title\}/.test(node.replace(/\/\/.*$/gm, "")), "a title is drawn bare")
  // theseus-hl0r1 F2: `tone` can be a CSS var() or an rgb(), so an alpha is never concatenated onto it
  // (an 8-digit hex is fine where the value is one of taxonomy's own `toneHex` literals, hence the word bound).
  assert.ok(!/\$\{[^}]*\btone\b[^}]*\}(55|66)/.test(node), 'an alpha is concatenated onto a tone, which makes the whole box-shadow invalid')
  // n17am 3: "+N folded" opens its subtree; tidy forgets the openings.
  assert.match(lens, /setOpened\(\(s\) => new Set\(\[\.\.\.s, n\.parent/)
  assert.match(lens, /setOpened\(new Set\(\)\); setTidy/)
})

test('the Graph and the Timeline fill the window through the one helper, and Esc then only leaves it (theseus-g00vv)', () => {
  const code = (s: string) => s.replace(/\/\/.*$/gm, '')
  assert.match(code(src('ops/GraphLens.tsx')), /useFullGraph\('Graph'\)/)
  assert.match(code(src('ops/GraphLens.tsx')), /onInit=\{fullGraph\.onInit\}/)
  assert.match(code(src('ops/TimelineLens.tsx')), /useFullGraph\('Timeline'\)/)
  for (const [f, s] of parts) assert.ok(!/fixed inset-0 z-30/.test(code(s)), `${f} has a full-window rule of its own (the sheets are z-40 overlays)`)
  const p = code(page)
  // The page's handler (window, capture) runs before the helper's (document, capture): it asks fullKey first.
  const k = p.slice(p.indexOf('const k = (e: KeyboardEvent)'))
  assert.ok(k.indexOf("fullKey(e, true) === 'leave'") > 0, 'the handler asks fullKey')
  assert.ok(k.indexOf("fullKey(e, true) === 'leave'") < k.indexOf('resolve(name, c, false)'), 'before it resolves Escape to back')
  assert.match(p, /live\.current\.full && fullKey/)
  // A lens without a full-window view leaves it; `g f` stays the shell's.
  assert.match(p, /FULLABLE\.includes\(patch\.lens\)/)
  assert.match(p, /name === 'f'\) \{ g = 0; return \}/)
})
