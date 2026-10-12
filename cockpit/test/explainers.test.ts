// The "?" on every panel (theseus-onxwh): each panel's `help` id has its words in `lib/explainers.ts`, each entry says
// all three parts in a sentence or more, and no entry is left without a panel. The sources are read as text: the
// panels are React components, which this runner does not render.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { EXPLAINERS, PARTS } from '../src/lib/explainers.ts'

const SRC = new URL('../src', import.meta.url).pathname

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((f) => {
    const p = join(dir, f)
    return statSync(p).isDirectory() ? (f === 'protocol.gen' ? [] : files(p)) : p.endsWith('.tsx') ? [p] : []
  })
}

/** Every opening tag of a panel component in `src`, whole: from `<Panel` to its closing `>`, past the `>` of an arrow
 *  function or a type and the braces and quotes of the attributes. */
function panelTags(text: string): string[] {
  const out: string[] = []
  for (const m of text.matchAll(/<(Panel|ChartPanel|Card)\b/g)) {
    let i = m.index! + m[0].length
    let depth = 0
    let quote = ''
    for (; i < text.length; i++) {
      const c = text[i]
      if (quote) { if (c === quote && text[i - 1] !== '\\') quote = ''; continue }
      if (depth > 0 && (c === '"' || c === "'" || c === '`')) { quote = c; continue }
      if (c === '{') depth++
      else if (c === '}') depth--
      else if (c === '<' && depth === 0 && text[i + 1] !== '/' ) { /* a type argument: <Point> */ const j = text.indexOf('>', i); if (/^<\w+>$/.test(text.slice(i, j + 1))) { i = j; continue } }
      else if (c === '>' && depth === 0 && text[i - 1] !== '=') break
      else if (c === '"' && depth === 0) { quote = c }
    }
    out.push(text.slice(m.index!, i + 1))
  }
  return out
}

const all = files(SRC).map((f) => ({ f, text: readFileSync(f, 'utf8') }))
const defs = new Set(['src/components/ui.tsx', 'src/components/ChartPanel.tsx'])
const sites = all.flatMap(({ f, text }) => panelTags(text).map((tag) => ({ f: f.slice(f.indexOf('src/')), tag })))
  .filter((s) => !s.tag.startsWith('<Card') || s.f.endsWith('Systems.tsx'))

test('there are panels to check, and the scan reads them whole', () => {
  assert.ok(sites.length > 100, `${sites.length} panel tags found`)
  const hel = sites.filter((s) => /\bhelp=/.test(s.tag))
  assert.equal(hel.length, sites.length, `panel tags without a help prop: ${sites.filter((s) => !/\bhelp=/.test(s.tag)).map((s) => `${s.f}: ${s.tag.slice(0, 60)}`).join('; ')}`)
})

test('every panel’s help id has an explainer', () => {
  const missing: string[] = []
  for (const s of sites) {
    if (/\bhelp=\{help\}/.test(s.tag)) { assert.ok(defs.has(s.f) || s.f.endsWith('Systems.tsx'), `${s.f}: a help passed on outside the frame's own wrappers`); continue }
    const m = s.tag.match(/\bhelp=(?:"([^"]+)"|\{false\}|\{'([^']+)'\})/)
    assert.ok(m, `${s.f}: a help prop that is neither a literal id nor {false}: ${s.tag.slice(0, 80)}`)
    const id = m[1] ?? m[2]
    if (id && !EXPLAINERS[id]) missing.push(`${s.f}: ${id}`)
  }
  assert.deepEqual(missing, [])
})

test('no explainer is left without a panel', () => {
  const used = new Set<string>()
  for (const { text } of all) for (const m of text.matchAll(/\bhelp=\{?["']([\w.]+)["']/g)) used.add(m[1])
  for (const { text } of all) for (const m of text.matchAll(/<HelpButton id="([\w.]+)"/g)) used.add(m[1])
  assert.deepEqual(Object.keys(EXPLAINERS).filter((k) => !used.has(k)), [])
})

test('each explainer says its goal, how to read it and why it matters, in plain sentences', () => {
  for (const [id, e] of Object.entries(EXPLAINERS)) {
    for (const [key] of PARTS) {
      const t = e[key]
      assert.ok(t.length >= 25 && t.length <= 520, `${id}.${key}: ${t.length} characters`)
      assert.match(t, /[.?!”]$/, `${id}.${key} ends in a full stop`)
    }
  }
})

test('a panel with no explainer is only a notice, and there are few of them', () => {
  const none = sites.filter((s) => /help=\{false\}/.test(s.tag))
  assert.ok(none.length < 20, `${none.length} panels without an explainer`)
})

test('the panel frame draws the question mark itself, from one place', () => {
  const ui = readFileSync(join(SRC, 'components/ui.tsx'), 'utf8')
  assert.match(ui, /<HelpButton id=\{help\}/)
  const chart = readFileSync(join(SRC, 'components/ChartPanel.tsx'), 'utf8')
  assert.match(chart, /help=\{help\}/)
})
