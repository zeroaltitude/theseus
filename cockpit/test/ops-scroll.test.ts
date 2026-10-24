// The Operations page's scrolling (theseus-g00vv): the needs-you list scrolls, its cards keep their height, and the
// Inspector's sections keep theirs. React is not rendered here, so the sources are read as text.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'

const src = (f: string) => readFileSync(new URL(`../src/${f}`, import.meta.url), 'utf8').replace(/^\s*\/\/.*$/gm, '')

test('the queue’s list scrolls and its cards do not shrink to fit it (item 7)', () => {
  const q = src('ops/Queue.tsx')
  const list = q.match(/className="([^"]*)p-1\.5"/)?.[0] ?? ''
  assert.match(list, /min-h-0/)
  assert.match(list, /overflow-y-auto/)
  // A flex item with overflow-hidden has a minimum height of 0: without shrink-0 the cards squeeze instead of the list scrolling.
  const card = q.slice(q.indexOf('role="button"'), q.indexOf('ring-wait/25'))
  assert.match(card, /\bshrink-0\b|\bflex-none\b/)
})

test('the inspector’s boxes scroll their block content, which does not shrink (item 7 check)', () => {
  const page = src('views/Ops.tsx')
  assert.equal((page.match(/<div className="min-h-0 flex-1 overflow-y-auto"><Inspector /g) ?? []).length, 2, 'side and over')
})
