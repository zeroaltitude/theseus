// The Personas soul editor (theseus-lky63): the cockpit opens the whole file, warns before a big shrink, and keeps the
// text it shows. React is not rendered here, so the component's wiring is read as text, comments stripped, as
// test/ops-page.test.ts reads its sources.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { bigShrink, SHRINK_WARNING, soulAsk, soulHead } from '../src/lib/ontology.ts'

const code = readFileSync(new URL('../src/components/Personas.tsx', import.meta.url), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/.*$/gm, '')

test('the ask states the old and the new size, and the warning appears exactly when the shrink is big', () => {
  const big = soulAsk('scout', 2000, { oldBytes: 1128, newBytes: 257 })
  assert.match(big, /1,128 B \(about 376 tokens\) -> 257 B \(about 86 tokens\)/)
  assert.ok(big.includes(SHRINK_WARNING))
  const small = soulAsk('scout', 2000, { oldBytes: 1128, newBytes: 1000 })
  assert.match(small, /1,128 B .* -> 1,000 B/)
  assert.ok(!small.includes(SHRINK_WARNING))
  assert.ok(!soulAsk('scout', 2000, { oldBytes: 150, newBytes: 10 }).includes(SHRINK_WARNING))
  assert.match(soulAsk('scout', 2000, { oldBytes: 300, newBytes: 300, backup: 'Oct 1' }), /backup of Oct 1/)
  assert.match(soulAsk('scout', 2000), /Write the soul of scout\? The file is replaced/)
})

test('a big shrink is under half of an old file over 200 bytes', () => {
  assert.equal(bigShrink(1128, 257), true)
  assert.equal(bigShrink(1128, 1000), false)
  assert.equal(bigShrink(150, 10), false, 'a small old file')
  assert.equal(bigShrink(201, 100), true)
  assert.equal(bigShrink(400, 200), false, 'exactly half is not under half')
  assert.equal(bigShrink(0, 0), false)
})

test('the card shows the first three non-empty lines, cut with an ellipsis', () => {
  assert.equal(soulHead(''), '')
  assert.equal(soulHead('  \n\n'), '')
  assert.equal(soulHead('# Identity\n\nA pilot.\n\n  Dry.  \nFourth line.'), '# Identity\nA pilot.\nDry.')
  const long = soulHead('x'.repeat(500))
  assert.equal(long.length, 241)
  assert.ok(long.endsWith('…'))
  assert.equal(soulHead('short'), 'short')
})

test('the editor opens with the file’s text and sends what it loaded', () => {
  const editor = code.slice(code.indexOf('function SoulEditor'), code.indexOf('function AddPersona'))
  assert.match(editor, /call<PersonaSoulReadResult>\('persona\.soul_read', \{ key: persona \}/)
  assert.ok(!/useState\(''\)/.test(editor), 'the soul text is never seeded empty')
  assert.ok(!/setText\(''\)/.test(editor), 'no empty box after a write')
  assert.match(editor, /const \[text, setText\] = useState<string \| null>\(null\)/)
  assert.match(editor, /readOnly=\{text === null\}/)
  assert.match(editor, /expect_digest: live\.digest/)
  assert.match(editor, /if \(out\) await load\(\)/)
  assert.match(editor, /failed !== null/, 'a failed read keeps Write off')
})

test('restore previous reads a listed backup by its time and writes it through the same call', () => {
  const editor = code.slice(code.indexOf('function SoulEditor'), code.indexOf('function AddPersona'))
  assert.match(editor, /'persona\.soul_read', \{ key: persona, bak_ms: b\.ms \}/)
  assert.match(editor, /restore previous/)
  assert.equal((editor.match(/'persona\.soul'/g) ?? []).length, 1, 'one write path')
  assert.match(editor, /bigShrink\(oldBytes, newBytes\)/)
})

test('the card reads the current soul for a persona that has one', () => {
  assert.match(code, /!p\.awaiting && <SoulHead/)
  assert.match(code, /soulHead\(r\.text \?\? ''\)/)
})
