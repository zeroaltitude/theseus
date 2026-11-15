// Shelves and books are two things (the owner's ruling, 2026-11-14, GitHub issue 17): a shelf is one of the sorted sets
// of imported conversations and exists; a book is what Theseus will write from a shelf, planned for 3.0.0, and none
// exists. What a person reads must not call a shelf a book, the Books view and the context page's note must say which
// of the two they show, and the note and its help words must agree. The pages are React, which this runner does not
// render, so their sources are read as text.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { EXPLAINERS } from '../src/lib/explainers.ts'
import { BOOK_WORDS, FIRST_CUT, SHELVES_TODAY, bookTitle, shelfTitle } from '../src/lib/books.ts'

const src = (p: string): string => readFileSync(new URL(`../src/${p}`, import.meta.url), 'utf8')

/** The sentences of `text` that name a book. */
const bookSentences = (text: string): string[] => text.split(/(?<=[.:;])\s+/).filter((s) => /\bbooks?\b/i.test(s))

/** A sentence about books is about the planned ones: it says they are written, planned, or not built. */
const aboutWrittenBooks = (s: string): boolean => /written|will write|to write|planned|not built|none exists|none is|not a book|not books|finished books|does not yet|not yet/i.test(s)

const SHELF_HELP = ['context.books', 'context.hints', 'context.filters', 'books.chart', 'books.book', 'books.episode'] as const

test('the words a person reads name a book only where they mean a written one', () => {
  const texts = [FIRST_CUT, SHELVES_TODAY, ...Object.values(BOOK_WORDS), ...['diary', 'sop', 'unsorted'].map(bookTitle)]
  for (const id of SHELF_HELP) texts.push(...Object.values(EXPLAINERS[id]))
  for (const t of texts) for (const s of bookSentences(t)) assert.ok(aboutWrittenBooks(s), `calls a shelf a book: ${s}`)
})

test('the Books view says which of the two it shows, and that written books are planned and not built', () => {
  assert.match(FIRST_CUT, /shows shelves, not books/)
  assert.match(FIRST_CUT, /planned for Theseus 3\.0\.0/)
  assert.match(FIRST_CUT, /None is written yet/)
  assert.match(SHELVES_TODAY, /Shelves exist today/)
  assert.match(SHELVES_TODAY, /planned for Theseus 3\.0\.0/)
  assert.equal(shelfTitle('diary'), 'Diary shelf')
  assert.equal(shelfTitle('sop'), 'SOP reference shelf')
  assert.match(shelfTitle('unsorted'), /no shelf/)
})

test('the context page note and its help words agree: shelves today, books planned for 3.0.0 and not built', () => {
  const note = src('views/Context.tsx')
  const help = Object.values(EXPLAINERS['context.books']).join(' ')
  for (const text of [note, help]) {
    assert.match(text, /Shelves exist today|Shelves, today/)
    assert.match(text, /planned for Theseus 3\.0\.0/)
    assert.match(text, /seven shelves/)
  }
  assert.match(help, /none exists/)
  assert.match(note, /are not built/)
  // The note and the help name the same seven shelves.
  for (const shelf of ['dictionary', 'encyclopedia', 'cookbook', 'standing procedures', 'diary', 'casebook', 'register']) {
    assert.ok(note.includes(shelf), `the note lacks ${shelf}`)
    assert.ok(help.includes(shelf), `the help lacks ${shelf}`)
  }
})

test('no page calls a shelf a book: the labels, headings and titles say shelf', () => {
  const pages = ['views/Books.tsx', 'views/Context.tsx', 'simple/Memories.tsx', 'components/Shell.tsx', 'ship/NoWebGL.tsx', 'views/Systems.tsx', 'ship/news.ts']
  const forbidden = [/book hints?/i, /the books\b(?!.{0,40}(planned|not built))/i, /by book\b/i, />book<|label: 'book'|word: 'book/, /book: \{/, /'The books'/, /Books view/, /label: 'Books'/, /, 'Books'\]/, /hinted here/]
  for (const p of pages) {
    // Comments are for the next reader of the code, which may keep the word; the strings a person reads may not.
    const text = src(p).split('\n').filter((l) => !/^\s*(\/\/|\*|\/\*\*)/.test(l)).join('\n')
    for (const f of forbidden) assert.doesNotMatch(text, f, `${p} matches ${f}`)
  }
  assert.match(src('components/Shell.tsx'), /to: '\/books', label: 'Shelves'/, 'the route keeps its name, the label says shelves')
  assert.match(src('simple/Memories.tsx'), /id: 'books', label: 'Shelves'/)
})
