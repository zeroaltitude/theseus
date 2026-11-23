// Configuration's map (issue 17): the inventory of settings is held to the daemon and to the cockpit. Every method it names
// is a method of the protocol crate; every setting called live is called from some page of the cockpit; every one called
// "command line only" is called from none (the daemon refuses them from any page); every section
// of the configuration file's example is on the page; and every tab a row points to is a tab of the page.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
import type { Health } from '../src/protocol.gen/index.ts'
import { CONFIG_TABS, CLI_ONLY, FIXED, LIVE, tableHead } from '../src/simple/config/settings.ts'

const ROOT = new URL('../..', import.meta.url).pathname
const SRC = join(ROOT, 'cockpit/src')

function files(dir: string, ext: RegExp): string[] {
  return readdirSync(dir).flatMap((f) => {
    const p = join(dir, f)
    return statSync(p).isDirectory() ? (f === 'protocol.gen' ? [] : files(p, ext)) : ext.test(f) ? [p] : []
  })
}
const cockpit = files(SRC, /\.tsx?$/).filter((f) => !f.endsWith('simple/config/settings.ts')).map((f) => readFileSync(f, 'utf8')).join('\n')
const protocol = files(join(ROOT, 'crates/theseus-protocol/src'), /\.rs$/).map((f) => readFileSync(f, 'utf8')).join('\n')
const live = LIVE.flatMap((s) => s.methods)
const cliOnly = CLI_ONLY.flatMap((s) => s.methods)

test('every method the map names is a method of the protocol crate', () => {
  const missing = [...live, ...cliOnly].filter((m) => !protocol.includes(`"${m}"`))
  assert.deepEqual(missing, [])
})

test('what is called live is called from the cockpit, and what is command-line only is called from none', () => {
  assert.deepEqual(live.filter((m) => !cockpit.includes(`'${m}'`)), [], 'a live setting no page calls')
  assert.deepEqual(cliOnly.filter((m) => cockpit.includes(`'${m}'`)), [], 'a page now calls it: move the setting to LIVE')
})

test('no setting is listed twice, and no method is both live and without a page', () => {
  const ids = [...LIVE, ...FIXED, ...CLI_ONLY].map((s) => s.id)
  assert.equal(new Set(ids).size, ids.length)
  assert.deepEqual(cliOnly.filter((m) => live.includes(m)), [])
})

test('every section of the configuration file’s example is on the page, and no section is on it twice', () => {
  const example = readFileSync(join(ROOT, 'crates/theseus-core/config/theseus.example.toml'), 'utf8')
  const sections = new Set([...example.matchAll(/^#?\s*\[\[?([A-Za-z_][\w.\-"]*)\]\]?/gm)].map((m) => tableHead(m[1])))
  assert.ok(sections.size > 40, `${sections.size} sections read`)
  const listed = FIXED.flatMap((s) => s.tables.map(tableHead))
  assert.deepEqual([...sections].filter((t) => !listed.includes(t)), [], 'in the file, not on the page')
  assert.deepEqual([...new Set(listed)].filter((t) => !sections.has(t)), [], 'on the page, not in the file')
  const heads = FIXED.map((s) => new Set(s.tables.map(tableHead)))
  for (const t of sections) assert.equal(heads.filter((h) => h.has(t)).length, 1, `${t} is in exactly one group`)
})

test('each row that points into this page points to a tab of it, and the page has those tabs in that order', () => {
  for (const s of LIVE) if ('tab' in s.at) assert.ok(CONFIG_TABS.includes(s.at.tab), `${s.id}: ${s.at.tab}`)
  const page = readFileSync(join(SRC, 'simple/Config.tsx'), 'utf8')
  assert.deepEqual([...page.matchAll(/\bid: '(\w+)'/g)].map((m) => m[1]), [...CONFIG_TABS])
})

test('the words are for a person: no internal name, and each setting says Theseus where it names it', () => {
  for (const s of [...LIVE, ...FIXED, ...CLI_ONLY]) {
    for (const t of [s.name, s.words]) assert.doesNotMatch(t, /\b(kernel|daemon|theseusd|rpc|protocol|ledger)\b/i, `${s.id}: ${t}`)
    assert.ok(s.words.length >= 20 && /[.]$/.test(s.words), `${s.id} says its words in a sentence`)
  }
})

test('the memory row does not teach threads as a setting: they are retired, and only the persona key of their section is used', () => {
  const memory = FIXED.find((s) => s.id === 'memory')!
  assert.doesNotMatch(memory.words, /sessions and threads/, 'threads offered as a current setting')
  assert.match(memory.words, /which persona answers a message \(the threads section’s one setting still used: threads are retired/)
  assert.ok(memory.tables.includes('threads'), 'the section is still in the example file, so it stays on the page')
})

test('what Theseus is running with now is read from health, and a missing report says nothing', () => {
  const h = { model: 'm', provider: 'p', profile: 'live', kernel: { admission_ceiling: 8, spend_limit_usd: 100, spend_limit_mode: 'notify' } } as unknown as Health
  assert.equal(FIXED.find((s) => s.id === 'limits')!.now!(h), '8 turns at once · $100 a conversation, then notify')
  assert.equal(FIXED.find((s) => s.id === 'models')!.now!(h), 'm on p, profile live')
  assert.equal(FIXED.find((s) => s.id === 'limits')!.now!({ kernel: { admission_ceiling: 4, spend_limit_usd: 12.5 } } as unknown as Health), '4 turns at once · $12.5 a conversation')
})
