// The palettes the charts draw with, held to the chart method's checks (`src/lib/palette.ts`, theseus-hnof.4), so a
// palette that drifts out of them fails `npm test` in the gate and in CI. Each figure is the method's own validator's on
// the same colours and surfaces, to a tenth.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { BAND, CONTRAST_MIN, contrast, oklch, validatePalette } from '../src/lib/palette.ts'
import { CATEGORICAL, CHROME, DEEP, OTHER, TOKEN_KINDS, TONE_MARK, TONE_SERIES, inkOn } from '../src/lib/viz.ts'
import { toneHex } from '../src/lib/taxonomy.ts'

const HULL = CHROME.dark.surface
const LIGHT = CHROME.light.surface
const near = (got: number, want: number, what: string) => assert.ok(Math.abs(got - want) < 0.05, `${what}: ${got.toFixed(2)}, not ${want}`)

test('the categorical slots pass every check on both panel faces, and in light with the table view as their relief', () => {
  for (const surface of [HULL, DEEP]) {
    const r = validatePalette(CATEGORICAL.dark, { mode: 'dark', surface })
    assert.ok(r.ok)
    assert.equal(r.cvd.state, 'pass')
    near(r.cvd.worst.d, 8.4, 'the closest neighbours under protanopia')
    assert.deepEqual([r.cvd.worst.a, r.cvd.worst.b, r.cvd.worst.kind], ['#199e70', '#c98500', 'protan'])
    near(r.normal.worst.d, 19.3, 'the closest neighbours for normal vision')
    assert.deepEqual(r.lowContrast, [])
  }
  const l = validatePalette(CATEGORICAL.light, { mode: 'light', surface: LIGHT })
  assert.ok(l.ok)
  near(l.cvd.worst.d, 9.1, 'light, under protanopia')
  near(l.normal.worst.d, 19.6, 'light, normal vision')
  // Aqua, yellow, and magenta sit under 3:1 on white: every chart's table view is their relief.
  assert.deepEqual(l.lowContrast.map((x) => x.color), ['#1baf7a', '#eda100', '#e87ba4'])
})

test('a scatter carries three series at most: the first three slots pass all pairs, and a fourth breaks it', () => {
  const three = validatePalette(CATEGORICAL.dark.slice(0, 3), { mode: 'dark', surface: HULL, pairs: 'all' })
  assert.ok(three.ok)
  assert.equal(three.cvd.state, 'pass')
  near(three.cvd.worst.d, 9.4, 'three slots, all pairs, under deuteranopia')
  near(three.normal.worst.d, 20.9, 'three slots, all pairs, normal vision')
  const four = validatePalette(CATEGORICAL.dark.slice(0, 4), { mode: 'dark', surface: HULL, pairs: 'all' })
  assert.equal(four.cvd.state, 'fail')
  assert.equal(four.ok, false)
})

test('the token kinds pass as they are: on both panel faces, and in light where output leans on the table view', () => {
  const kinds = TOKEN_KINDS.map((k) => k.color)
  for (const surface of [HULL, DEEP]) {
    const r = validatePalette(kinds, { mode: 'dark', surface })
    assert.ok(r.ok)
    near(r.cvd.worst.d, 22.0, 'token kinds under deuteranopia')
    near(r.normal.worst.d, 28.1, 'token kinds, normal vision')
    assert.deepEqual(r.lowContrast, [])
  }
  const l = validatePalette(kinds, { mode: 'light', surface: LIGHT })
  assert.ok(l.ok)
  assert.deepEqual(l.lowContrast.map((x) => x.color), ['#0ea5c6'])
})

test('the tones are for state and text: as a series set they fail, and their snapped steps pass', () => {
  // The bright tones, as index.css draws them: every one above the dark band, thinking and tool the same colour under
  // deuteranopia, live and ok close for everyone. They are text on the night glass, where they read at 6:1 or more.
  const raw = ['live', 'ok', 'wait', 'fault', 'model', 'tool', 'think', 'money'] as const
  const r = validatePalette(raw.map((t) => toneHex[t]), { mode: 'dark', surface: HULL })
  assert.equal(r.ok, false)
  assert.equal(r.offBand.length, 8)
  assert.equal(r.cvd.state, 'fail')
  near(r.cvd.worst.d, 0.3, 'thinking against tool under deuteranopia')
  near(r.normal.worst.d, 12.1, 'live against ok, normal vision')
  for (const t of raw) assert.ok(contrast(toneHex[t], HULL) >= 4.5, `${t} as text on the hull`)
  // Their steps for series and marks, in the order the charts assign them.
  const dark = validatePalette(TONE_SERIES.dark.map(([, c]) => c), { mode: 'dark', surface: HULL })
  assert.ok(dark.ok)
  near(dark.cvd.worst.d, 21.5, 'the tone steps under deuteranopia')
  near(dark.normal.worst.d, 24.1, 'the tone steps, normal vision')
  const light = validatePalette(TONE_SERIES.light.map(([, c]) => c), { mode: 'light', surface: LIGHT })
  assert.ok(light.ok)
  assert.deepEqual(light.lowContrast, [])
})

test('a tone drawn as a mark takes its step: in the dark band, and 3:1 on both faces; idle is the de-emphasis gray', () => {
  const [lo, hi] = BAND.dark
  for (const [tone, c] of Object.entries(TONE_MARK)) {
    if (tone === 'idle') { assert.equal(c, OTHER); continue }
    const { L } = oklch(c)
    assert.ok(L >= lo && L <= hi, `${tone} ${c}: L ${L.toFixed(3)}`)
    for (const surface of [HULL, DEEP]) assert.ok(contrast(c, surface) >= CONTRAST_MIN, `${tone} on ${surface}`)
  }
  assert.equal(Object.keys(TONE_MARK).length, 9)
})

test('words inside a fill take whichever ink stands out more from it', () => {
  const fills = [...CATEGORICAL.dark, ...Object.values(TONE_MARK), ...TOKEN_KINDS.map((k) => k.color)]
  for (const f of fills) {
    const ink = inkOn(f)
    const other = ink === DEEP ? CHROME.dark.text : DEEP
    assert.ok(contrast(ink, f) >= contrast(other, f), f)
    assert.ok([DEEP, CHROME.dark.text].includes(ink as typeof DEEP))
  }
  assert.equal(inkOn('#0a1828'), CHROME.dark.text)
  assert.equal(inkOn('#fcfcfb'), DEEP)
})
