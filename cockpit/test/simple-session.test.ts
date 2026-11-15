// A session's own pages open under the simple bar (GitHub issue 17), at the addresses they always had. The routes are
// read as text, as the other route tests do: the router is a browser's.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'

const read = (p: string) => readFileSync(new URL(`../src/${p}`, import.meta.url), 'utf8')
const main = read('main.tsx')
const simpleLayout = main.slice(main.indexOf('element: <SimpleShell />'), main.indexOf('element: <Shell />'))
const shipLayout = main.slice(main.indexOf('element: <Shell />'))

test('a whole session, one turn of it and a label taken off it are routes of the simple frame', () => {
  assert.match(simpleLayout, /\{ path: 'session\/:id', element: <SimpleSession view="deck" \/> \}/)
  assert.match(simpleLayout, /\{ path: 'session\/:id\/turn\/:turn', element: <SimpleSession view="turn" \/> \}/)
  assert.match(simpleLayout, /\{ path: 'session\/:id\/unlabel\/:flag', element: <SimpleSession view="unlabel" \/> \}/)
})

test('and no longer routes of the Ship’s frame, so no address matches twice', () => {
  assert.doesNotMatch(shipLayout, /path: 'session\//)
  assert.doesNotMatch(main, /views\/(SessionDeck|TurnInspect|Unlabel)/, 'main loads them through the simple page, not by themselves')
})

test('the simple page shows the Ship’s own views, so what a session page shows and does has no second copy', () => {
  const s = read('simple/memories/Session.tsx')
  assert.match(s, /deck: lazy\(\(\) => import\('@\/views\/SessionDeck'\)\)/)
  assert.match(s, /turn: lazy\(\(\) => import\('@\/views\/TurnInspect'\)\)/)
  assert.match(s, /unlabel: lazy\(\(\) => import\('@\/views\/Unlabel'\)\)/)
})

test('its frame leads back to Ops and to the page of what the turn was shown', () => {
  const s = read('simple/memories/Session.tsx')
  assert.match(s, /<Link to="\/"/)
  assert.match(s, /turnHere\(id, turn\)/)
  // A session that does not exist yet has no turn to show.
  assert.match(s, /id !== 'new'/)
})
