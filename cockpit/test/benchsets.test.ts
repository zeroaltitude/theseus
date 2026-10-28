// Where the Benchmarks page's task sets come from (theseus-2w02d): the manifest's `dataset_ui` block, so a dataset the
// manifest lists is never fetched; a dataset it does not list is read in full, as before; and a manifest that predates
// the block (or cannot be read) is no worse than the page was.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'
import { datasetsUi, readManifest, readUiBlocks } from '../src/lib/benchindex.ts'

const SET = { set_id: 'fixture-set', label: 'A fixture', short: 'fixture', about: 'x' }
const FILES: Record<string, string> = {
  'a-listed-with-ui': JSON.stringify({ suite: 'x', ui: SET, summary: {} }),
  'b-listed-without-ui': JSON.stringify({ suite: 'x', summary: {} }),
  'c-unlisted-with-ui': JSON.stringify({ suite: 'x', ui: { ...SET, set_id: 'c' } }),
  'd-unlisted-without-ui': JSON.stringify({ suite: 'x' }),
  'e-not-json': '{oops',
}
const NAMES = ['index', ...Object.keys(FILES)]
const manifest = (dataset_ui: unknown, extra: Record<string, unknown> = {}) =>
  JSON.stringify({ version: 1, about: 'x', ...(dataset_ui === undefined ? {} : { dataset_ui }), runs: [], ...extra })
const LISTED = manifest({ 'a-listed-with-ui': { ...SET, set_id: 'from-the-manifest' }, 'b-listed-without-ui': null })

function counting() {
  const reads: string[] = []
  return { reads, read: async (n: string) => { reads.push(n); return FILES[n] } }
}

test('a dataset the manifest lists is not read: its block comes from the manifest, and a listed null is no block', async () => {
  const { reads, read } = counting()
  const got = await datasetsUi(NAMES, LISTED, read)
  assert.ok(!reads.includes('a-listed-with-ui'), 'the manifest already holds its block')
  assert.ok(!reads.includes('b-listed-without-ui'), 'the manifest says it has none')
  assert.deepEqual(got.find((d) => d.report === 'a-listed-with-ui')?.ui, { ...SET, set_id: 'from-the-manifest' })
  assert.equal(got.find((d) => d.report === 'b-listed-without-ui')?.ui, undefined)
})

test('a dataset the manifest does not list is read in full, and its block taken; an unreadable one gives none', async () => {
  const { reads, read } = counting()
  const got = await datasetsUi(NAMES, LISTED, read)
  assert.deepEqual(reads.sort(), ['c-unlisted-with-ui', 'd-unlisted-without-ui', 'e-not-json'])
  assert.deepEqual(got.find((d) => d.report === 'c-unlisted-with-ui')?.ui, { ...SET, set_id: 'c' })
  assert.equal(got.find((d) => d.report === 'd-unlisted-without-ui')?.ui, undefined)
  assert.deepEqual(got.find((d) => d.report === 'e-not-json'), { report: 'e-not-json' })
})

test('the manifest is not itself read as a dataset, and the names keep the order they were given', async () => {
  const { reads, read } = counting()
  const got = await datasetsUi(NAMES, LISTED, read)
  assert.ok(!reads.includes('index'))
  assert.deepEqual(got.map((d) => d.report), Object.keys(FILES))
})

test('a manifest with no block, an old one, reads every dataset as the page always did', async () => {
  for (const text of [manifest(undefined), undefined, null, '{oops', manifest({}, { version: 2 }), manifest([]), manifest('x')]) {
    const { reads, read } = counting()
    const got = await datasetsUi(NAMES, text, read)
    assert.deepEqual(reads.sort(), Object.keys(FILES).sort(), String(text))
    assert.deepEqual(got.find((d) => d.report === 'a-listed-with-ui')?.ui, SET)
  }
})

test('an entry that is neither a block nor null is not trusted: that dataset is read', () => {
  const m = readUiBlocks(manifest({ a: SET, b: null, c: 'x', d: 3, e: [] }))
  assert.deepEqual([...m.keys()].sort(), ['a', 'b'])
})

test('the manifest the old reader sees is unchanged by the block: same rows, same series, same reason', () => {
  const rows = [{ report: 'r', benchmark: 'b', tier: null, arm: 'a', variant: 'a', harness: 'A', model: null, date: '2026-03-01', commit: null,
    dataset: 'docs/benchmarks/r.json', trials: 3, tasks: 1, metrics: { pass_rate: { value: 0.5, lo: 0.1, hi: 0.9, k: 1, n: 2 } } }]
  const plain = JSON.stringify({ version: 1, about: 'x', runs: rows })
  const withUi = JSON.stringify({ version: 1, about: 'x', dataset_ui: { r: SET }, runs: rows })
  assert.deepEqual(readManifest(withUi), readManifest(plain))
  assert.equal(readManifest(withUi).reason, null)
})

test('the committed manifest lists the two generated sets, and what it lists is what the dataset says', () => {
  const dir = new URL('../../docs/benchmarks/', import.meta.url)
  const doc = JSON.parse(readFileSync(new URL('index.json', dir), 'utf8')) as { dataset_ui?: Record<string, unknown> }
  const listed = readUiBlocks(JSON.stringify({ version: 1, ...doc }))
  assert.ok(listed.size > 40, 'bench/report/manifest.py build writes every dataset it reads')
  for (const name of ['2026-10-18-terminal-bench-v1-high', '2026-10-18-terminal-bench-v1-max']) {
    assert.equal((listed.get(name) as { set_id?: unknown } | undefined)?.set_id, name.endsWith('high') ? 'tb-v1-high' : 'tb-v1-max', name)
  }
  // A stale entry would show the page a set the dataset no longer carries: a dataset edited by hand needs the manifest rebuilt.
  // Datasets the manifest does not list are not checked: a newer run is read in full until the manifest is rebuilt.
  for (const [name, ui] of listed) {
    const path = new URL(`${name}.json`, dir)
    if (!existsSync(path)) continue
    assert.deepEqual(ui, (JSON.parse(readFileSync(path, 'utf8')) as { ui?: unknown } | null)?.ui, name)
  }
})
