// A card's contents stay inside its border (theseus-onxwh): the Systems page's cards take the shared body class, which
// lets a card shrink inside its grid and scrolls a wide table sideways inside it.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { CARD_BODY } from '../src/lib/cardfit.ts'

const read = (p: string) => readFileSync(new URL(`../src/${p}`, import.meta.url), 'utf8')

test('the card body can shrink and scrolls sideways', () => {
  assert.match(CARD_BODY, /\bmin-w-0\b/)
  assert.match(CARD_BODY, /\boverflow-x-auto\b/)
  assert.doesNotMatch(CARD_BODY, /overflow-(hidden|visible)/)
})

test('every card on the Systems page is a Card or a panel with that body', () => {
  const systems = read('views/Systems.tsx')
  assert.match(systems, /bodyClassName=\{bodyClassName \?\? CARD_BODY\}/, 'a Card’s default body is the shared one')
  // The catalog, the widest table, is a Card with the default body, not one with its own.
  const catalog = systems.slice(systems.indexOf('systems.catalog'), systems.indexOf('systems.catalog') + 160)
  assert.doesNotMatch(catalog, /bodyClassName/)
  for (const f of ['SystemsCards', 'Extensions', 'HandsGrid', 'DiskSpool', 'SelfChanges', 'BinaryCard']) {
    const text = read(`components/${f}.tsx`)
    assert.match(text, /bodyClassName=\{CARD_BODY\}/, f)
    assert.doesNotMatch(text, /bodyClassName="px-3\.5 py-2\.5"/, f)
  }
})

test('the panel frame lets a panel shrink below its content inside a grid', () => {
  assert.match(read('components/ui.tsx'), /'panel flex min-h-0 min-w-0 flex-col'/)
})
