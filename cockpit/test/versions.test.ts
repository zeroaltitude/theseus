// A pack's versions (`src/lib/versions.ts`, M5 25f), run by `npm test`: lineages and the line diff. Invented text.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { changed, lineDiff, lineages, versionOf } from '../src/lib/versions.ts'

test('each lineage with a learned version lists its root, then its learned versions in order', () => {
  const v = (pack: string, source: string, root?: string) => ({ pack, source, root, text: '' })
  const ls = lineages([
    v('classify.v1', 'compiled'), v('loop.v1', 'compiled'),
    v('classify.v102', 'learned', 'classify.v1'), v('classify.v101', 'learned', 'classify.v1'),
  ])
  assert.deepEqual(ls.map((l) => [l.root, l.versions.map((x) => x.pack)]), [
    ['classify.v1', ['classify.v1', 'classify.v101', 'classify.v102']],
  ])
  assert.equal(versionOf('classify.v101'), 101)
})

test('the diff keeps common lines and marks the reworded ones, comments aside', () => {
  const a = '# a note\nid = "classify"\nversion = 1\nmeans = "It asks for new work."\n'
  const b = 'id = "classify"\nversion = 101\nmeans = "It asks for new work, a bare stop aside."\n'
  const d = lineDiff(a, b)
  assert.deepEqual(d.filter((l) => l.op !== ' ').map((l) => `${l.op}${l.line}`), [
    '-version = 1', '-means = "It asks for new work."',
    '+version = 101', '+means = "It asks for new work, a bare stop aside."',
  ])
  assert.equal(d[0].line, 'id = "classify"')
  assert.equal(changed(d, 0).length, 4)
})
