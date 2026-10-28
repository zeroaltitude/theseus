// Cross-page links (theseus-gi8o2). The owner found "its manifest, in the Context tab" dead on the Ship. The cause
// is a class, not a typo: a control drawn on more than one page edited *this page's* address, setting a key only one
// of its pages reads. On the others the press did nothing at all, silently.
//
// Held here: the whole addresses that replace those edits, the deck answering `?comp=`, and a guard over the class —
// no shared component writes a key that only one page reads.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'
import { inspectCallHref, inspectMsgHref, manifestHref } from '../src/lib/explorer.ts'

const read = (p: string) => readFileSync(new URL(`../src/${p}`, import.meta.url), 'utf8')

test('the deck’s addresses name the session, the tab and the compilation, each escaped', () => {
  assert.equal(manifestHref('ses_1', 'cmp_7c2191fdf4'), '/session/ses_1?tab=context&comp=cmp_7c2191fdf4')
  assert.equal(inspectMsgHref('ses_1', 'nd_2'), '/session/ses_1?msg=nd_2')
  assert.equal(inspectCallHref('ses_1', 'toolu_3'), '/session/ses_1?call=toolu_3')
  // An id is not a word: anything the protocol hands us goes through encodeURIComponent, in the path and the query.
  assert.equal(manifestHref('ses a/b', 'c&d=e'), '/session/ses%20a%2Fb?tab=context&comp=c%26d%3De')
  assert.equal(inspectMsgHref('s?1', 'n#2'), '/session/s%3F1?msg=n%232')
  // The manifest link carries no `msg` or `call`: the drawer that was covering the manifest closes by the going.
  assert.doesNotMatch(manifestHref('ses_1', 'c1'), /[?&](msg|call)=/)
})

test('the manifest link is a whole address, from the Ship as much as from the deck (the owner’s 13:16 report)', () => {
  const ins = read('components/ModelInspector.tsx')
  // It was `setParams(q => q.set('tab','context'))`. The Ship draws this same drawer and reads no `tab`, so there it
  // did nothing whatsoever.
  assert.doesNotMatch(ins, /set\('tab'/, 'no edit of whatever address the presser happens to be on')
  assert.match(ins, /<Link to=\{manifestHref\(sessionId, comp\.compilation_id\)\}/)
  assert.match(ins, /its manifest, in the Context tab/)
  // Both pages that draw the drawer hand it the session, so the address can be built on either.
  for (const v of ['views/Ship.tsx', 'views/SessionDeck.tsx']) {
    assert.match(read(v), /<ModelInspector sessionId=\{/, v)
  }
})

test('the deck opens the compilation the address names, and says which one it is', () => {
  const deck = read('views/SessionDeck.tsx')
  // `?comp=` is what makes "focused on that compilation's manifest" possible at all: before this the open row was
  // local state, so the link could only switch the tab and land you at the top of the lineage.
  assert.match(deck, /const open = params\.get\('comp'\)/)
  assert.match(deck, /p\.set\('comp', id\)/)
  assert.match(deck, /p\.delete\('comp'\)/)
  // The manifest itself is what renders when a row is open.
  assert.match(deck, /open === c\.compilation_id && <div[^>]*><JsonView value=\{c\.manifest\}/)
  // A long lineage puts the named row below the fold, so it brings itself into view and wears its own ring.
  assert.match(deck, /scrollIntoView/)
  assert.match(deck, /ref=\{open === c\.compilation_id \? asked : undefined\}/)
  // And the route the address names exists.
  assert.match(read('main.tsx'), /path: 'session\/:id'/)
})

test('a transcript off the deck sends its inspect marks to the deck, instead of writing a key nobody reads', () => {
  const t = read('components/Transcript.tsx')
  assert.match(t, /const away = routeId \? null : deck \?\? null/, 'on the deck it toggles in place; elsewhere it travels')
  assert.match(t, /away \? inspectMsgHref\(away, n\.node_id\) : null/)
  assert.match(t, /away \? inspectCallHref\(away, callKey\) : null/)
  // A link, not a button, so the middle button and a modified click open a tab as on any link.
  assert.match(t, /return away\s*\n?\s*\? <Link to=\{away\}/)
  // The Operations page's join pane is the one host that draws no drawer; it is the one that passes `deck`.
  assert.match(read('ops/JoinPane.tsx'), /<Transcript [^>]*deck=\{sid\}/)
  assert.doesNotMatch(read('views/SessionDeck.tsx'), /<Transcript [^>]*deck=/, 'the deck is the deck: it inspects in place')
})

test('no shared component writes an address key only one page reads (the guard over the whole class)', () => {
  // A key belongs to one page when exactly one view reads it. A component under components/ or ops/ can be drawn on
  // any page, so a write of such a key from there is dead everywhere else — which is the defect the owner hit.
  const PAGE_KEYS = ['tab', 'comp']
  const shared: string[] = []
  for (const dir of ['components', 'ops']) {
    for (const f of readdirSync(new URL(`../src/${dir}`, import.meta.url))) {
      if (f.endsWith('.tsx') || f.endsWith('.ts')) shared.push(`${dir}/${f}`)
    }
  }
  assert.ok(shared.length > 40, `the sweep must actually read the tree, not an empty list (${shared.length})`)
  const bad: string[] = []
  for (const f of shared) {
    const src = read(f)
    for (const k of PAGE_KEYS) if (src.includes(`.set('${k}'`)) bad.push(`${f} writes ?${k}=`)
  }
  assert.deepEqual(bad, [], 'these presses are dead on every page that does not read the key; link to the page instead')

  // And the keys really are one page's, so the rule above is about something true.
  const views = readdirSync(new URL('../src/views', import.meta.url)).filter((f) => f.endsWith('.tsx'))
  for (const k of PAGE_KEYS) {
    const readers = views.filter((v) => read(`views/${v}`).includes(`get('${k}')`))
    assert.deepEqual(readers, ['SessionDeck.tsx'], `?${k}= is read by exactly one view`)
  }
})
