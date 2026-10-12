// The Ship's kept view prefs (theseus-onxwh): the watch panel's fold and the depth gauge's size read what the browser
// kept, and anything unknown is the default.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { DEPTH_KEY, flipWatch, parseDepth, parseWatch, WATCH_KEY } from '../src/lib/prefs.ts'

test('the watch panel is out unless the browser kept it folded, and the control flips it', () => {
  assert.equal(parseWatch(null), 'out')
  assert.equal(parseWatch('nonsense'), 'out')
  assert.equal(parseWatch('in'), 'in')
  assert.equal(parseWatch(flipWatch('out')), 'in', 'folded, kept, and read back folded')
  assert.equal(parseWatch(flipWatch('in')), 'out')
})

test('the depth gauge is small unless the browser kept it minimized or maximized', () => {
  assert.equal(parseDepth(null), 'small')
  assert.equal(parseDepth('huge'), 'small')
  assert.equal(parseDepth('min'), 'min')
  assert.equal(parseDepth('max'), 'max')
  assert.equal(parseDepth('small'), 'small')
})

test('the two prefs have their own keys, and the Ship reads them through the kept-pref hook', () => {
  assert.notEqual(WATCH_KEY, DEPTH_KEY)
  const ship = readFileSync(new URL('../src/views/Ship.tsx', import.meta.url), 'utf8')
  assert.match(ship, /usePref\(DEPTH_KEY, parseDepth\)/)
  assert.match(ship, /<WatchDock>/)
  const dock = readFileSync(new URL('../src/ship/WatchDock.tsx', import.meta.url), 'utf8')
  assert.match(dock, /usePref\(WATCH_KEY, parseWatch\)/)
  assert.match(dock, /<Watch\b|\{children\}/, 'the watch stays mounted while folded: nothing it shows is lost')
  assert.doesNotMatch(dock, /\{out && children\}|out \? children/, 'the folded panel is not unmounted')
})

test('a folded panel is reachable again by a tab, and reduced motion has no slide', () => {
  const dock = readFileSync(new URL('../src/ship/WatchDock.tsx', import.meta.url), 'utf8')
  assert.match(dock, /ship-slide-tab/)
  assert.match(dock, /inert=\{!out\}/)
  const css = readFileSync(new URL('../src/ship/ship.css', import.meta.url), 'utf8')
  assert.match(css, /prefers-reduced-motion: reduce\) \{ \.ship-slide \{ transition: none; \} \}/)
  assert.match(css, /\.ship-root\[data-calm="1"\] \.ship-slide \{ transition: none; \}/)
})
