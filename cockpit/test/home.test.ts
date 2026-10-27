// The mark goes home (theseus-j4cjc): the owner asked that a press on the Theseus mark at the top left always take
// him back to the root URL. Two things are held here: the rules (a plain press goes home live, a press that opens a
// tab leaves this page alone), and that both marks in the frame are wired to them as real anchors.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { HOME, opensElsewhere, pressedHome, type Press } from '../src/lib/home.ts'

const plain: Press = { button: 0, metaKey: false, ctrlKey: false, shiftKey: false, altKey: false }

/** A reset that records what the press asked of it. */
function spy() {
  const did: string[] = []
  return { did, r: { live: () => did.push('live'), clearSelection: () => did.push('selection') } }
}

test('the root URL is the root URL, and `main.tsx` has somewhere for it to land', () => {
  assert.equal(HOME, '/')
  const main = readFileSync(new URL('../src/main.tsx', import.meta.url), 'utf8')
  // The index route is what makes `/` a real destination rather than a blank frame.
  assert.match(main, /index: true, element: <Navigate to="\/ship" replace \/>/)
})

test('a plain press goes home and puts the page back the way a fresh load of / has it', () => {
  const { did, r } = spy()
  assert.equal(opensElsewhere(plain), false)
  assert.equal(pressedHome(plain, r), true)
  assert.deepEqual(did, ['live', 'selection'], 'the moment goes live, and nothing stays picked')
})

test('a press that opens the root URL in another tab leaves this page exactly as it stands', () => {
  // Each of these is a press the browser answers itself; React Router's Link declines to route them, so the page
  // the operator is reading stays on its moment and keeps its selection.
  const elsewhere: Record<string, Press> = {
    'the middle button': { ...plain, button: 1 },
    'the right button': { ...plain, button: 2 },
    '⌘-click': { ...plain, metaKey: true },
    'ctrl-click': { ...plain, ctrlKey: true },
    'shift-click': { ...plain, shiftKey: true },
    'alt-click': { ...plain, altKey: true },
    'a press something else already answered': { ...plain, defaultPrevented: true },
  }
  for (const [what, e] of Object.entries(elsewhere)) {
    const { did, r } = spy()
    assert.equal(opensElsewhere(e), true, what)
    assert.equal(pressedHome(e, r), false, what)
    assert.deepEqual(did, [], `${what}: nothing of this page was reset`)
  }
})

test('the mark is an anchor, not a button, and it resets through the two stores a reload starts clean', () => {
  const mark = readFileSync(new URL('../src/components/HomeMark.tsx', import.meta.url), 'utf8')
  // An anchor is what makes the middle button, ⌘-click and "copy link address" work at all. A <button onClick=nav()>
  // would satisfy a plain click and silently break every one of those.
  assert.match(mark, /<Link\b/, 'a Link renders an <a href>; a button would not')
  assert.match(mark, /to=\{HOME\}/)
  assert.match(mark, /pressedHome\(e,/)
  assert.doesNotMatch(mark, /useNavigate|<button/, 'no programmatic navigation: the anchor does the going')
  // The reset reaches the stores outside React's render, so the press needs no subscription of its own.
  assert.match(mark, /useAsOf\.getState\(\)\.set\(null\)/, 'home is live')
  assert.match(mark, /useSelection\.getState\(\)\.clear\(\)/, 'a fresh page has nothing picked')
})

test('both Theseus marks in the frame wear it, so every view and route has the way home', () => {
  // The rail's coin is the top-left-most thing in the app; the bar's wordmark is what the owner's screenshot shows.
  // Both live in the Shell, which is the layout route for every path, so this covers every view.
  const shell = readFileSync(new URL('../src/components/Shell.tsx', import.meta.url), 'utf8')
  assert.match(shell, /<HomeMark[^>]*>\s*<Coin\b/, 'the rail’s coin')
  assert.doesNotMatch(shell, /<div className="relative" title="Theseus">/, 'the inert mark is gone')

  const bar = readFileSync(new URL('../src/components/Heartbeat.tsx', import.meta.url), 'utf8')
  assert.match(bar, /<HomeMark[\s\S]{0,400}?wordmark[^>]*>THESEUS<\/span>/, 'the bar’s wordmark')
  // The version sits inside the anchor too: the owner presses the mark, not a 9-pixel word in it.
  assert.match(bar, /<HomeMark[\s\S]{0,600}?v\$\{h\.version\}[\s\S]{0,200}?<\/HomeMark>/)

  for (const [name, src] of [['Shell.tsx', shell], ['Heartbeat.tsx', bar]] as const) {
    assert.match(src, /import \{ HomeMark \} from '\.\/HomeMark'/, name)
  }
})
