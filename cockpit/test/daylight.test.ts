// The daylight mode's colours (`src/lib/daylight.ts`), run by `npm test` (theseus-hnof.5): every tone by day reads as
// small text on the daylight papers, and a chart's option takes every night colour to its daylight step, alpha kept,
// leaving names, functions and what is not a colour alone.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { CHOICES, choiceOf, daylight, daylightColor, MARKS, modeOf, nextChoice, THEME, TONES } from '../src/lib/daylight.ts'
import { CATEGORICAL, CHROME, TONE_MARK, TONE_SERIES } from '../src/lib/viz.ts'

/** WCAG contrast, as the validator computes it. */
const lum = (h: string) => {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16) / 255).map((c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4))
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}
const contrast = (a: string, b: string) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05) }

test('every tone and ink by day reads as small text on the panel face and the page (4.5:1 or more)', () => {
  for (const surface of [THEME.light.hull, THEME.light.void]) {
    for (const [tone, c] of Object.entries(TONES.light)) assert.ok(contrast(c, surface) >= 4.5, `${tone} ${c} on ${surface}: ${contrast(c, surface).toFixed(2)}`)
    for (const ink of ['ink', 'inkDim', 'inkFaint', 'gold']) assert.ok(contrast(THEME.light[ink], surface) >= 4.5, `${ink} on ${surface}`)
  }
  assert.deepEqual(Object.keys(TONES.light).sort(), Object.keys(TONES.dark).sort())
})

test('a night colour goes to its daylight step, at its own alpha, in every notation', () => {
  assert.equal(daylightColor('#22d3ee'), TONES.light.live)
  assert.equal(daylightColor('#22D3EE'), TONES.light.live)
  assert.equal(daylightColor('#22d3ee66'), `${TONES.light.live}66`)
  assert.equal(daylightColor('rgba(34,211,238,0.45)'), 'rgba(1,114,130,0.45)')
  assert.equal(daylightColor('rgba(34, 211, 238, .35)'), 'rgba(1,114,130,0.35)')
  assert.equal(daylightColor('rgb(176 141 87 / 0.22)'), 'rgba(120,92,48,0.22)')
  assert.equal(daylightColor(CHROME.dark.text), THEME.light.ink)
  assert.equal(daylightColor(CHROME.dark.grid), 'rgba(120,92,48,0.13)')
  CATEGORICAL.dark.forEach((c, i) => assert.equal(daylightColor(c), CATEGORICAL.light[i]))
  for (const [tone, c] of TONE_SERIES.dark) assert.equal(daylightColor(c), TONE_SERIES.light.find(([t]) => t === tone)![1])
})

test('what is not a night colour stays as it is; a night neutral no pair names takes the ink or paper of its strength', () => {
  for (const s of ['#ops', '#abcdef', 'sonnet', 'transparent', 'rgba(0,0,0,0.6)', '#ffffff', '#6e6450', '#846205', '{value} ms', '']) assert.equal(daylightColor(s), s)
  assert.equal(daylightColor('#ece2cc'), THEME.light.ink, 'a bright ivory label')
  assert.equal(daylightColor('#bdb197'), THEME.light.inkDim, 'a dimmer one')
  assert.equal(daylightColor('rgba(11,25,42,0.9)'), 'rgba(255,253,248,0.9)', 'a night glass')
  // Light steps never move again: by day, a colour already mapped stays.
  for (const c of [...Object.values(TONES.light), ...Object.values(THEME.light), ...CATEGORICAL.light]) assert.equal(daylightColor(c), c, c)
})

test('a chart option by day: colours at any depth, the tooltip frame inside its text, the rest untouched', () => {
  const fmt = (v: number) => `${v} ms`
  class Gradient { stops: string[]; constructor(stops: string[]) { this.stops = stops } }
  const g = new Gradient(['#22d3ee'])
  const option = {
    textStyle: { color: '#c8bb9b', fontFamily: 'Inter Variable' },
    tooltip: { backgroundColor: 'rgba(6,15,27,0.96)', extraCssText: 'border-radius: 8px; box-shadow: 0 0 16px -6px rgba(34,211,238,.35), 0 10px 28px rgba(0,0,0,.6);' },
    series: [{ name: '#ops', color: '#3987e5', data: [1, 2, { value: 3, itemStyle: { color: '#fb718588' } }], label: { formatter: fmt }, extra: g,
      areaStyle: { color: { type: 'linear', colorStops: [{ offset: 0, color: '#22d3ee55' }, { offset: 1, color: '#22d3ee00' }] } } }],
  }
  const day = daylight(option)
  assert.equal(day.textStyle.color, THEME.light.inkDim)
  assert.equal(day.textStyle.fontFamily, 'Inter Variable')
  assert.equal(day.tooltip.backgroundColor, 'rgba(255,253,248,0.96)')
  assert.equal(day.tooltip.extraCssText, 'border-radius: 8px; box-shadow: 0 0 16px -6px rgba(1,114,130,0.35), 0 10px 28px rgba(0,0,0,.6);')
  assert.equal(day.series[0].name, '#ops')
  assert.equal(day.series[0].color, CATEGORICAL.light[0])
  assert.deepEqual(day.series[0].data.slice(0, 2), [1, 2])
  assert.equal((day.series[0].data[2] as { itemStyle: { color: string } }).itemStyle.color, `${TONES.light.fault}88`)
  assert.equal(day.series[0].label.formatter, fmt)
  assert.equal(day.series[0].extra, g, 'a class instance is passed through whole')
  assert.deepEqual(day.series[0].areaStyle.color.colorStops.map((s) => s.color), [`${TONES.light.live}55`, `${TONES.light.live}00`])
  assert.equal(option.textStyle.color, '#c8bb9b', 'the night option is not changed')
})

test('a tone drawn as a mark has its step by day, and it is the colour daylight() gives the same series', () => {
  // The charts drawn in HTML take TONE_MARK as the mode sets it (mode.ts); the ECharts ones go through daylight().
  assert.deepEqual(Object.keys(MARKS.light).sort(), Object.keys(TONE_MARK).sort())
  for (const [tone, night] of Object.entries(MARKS.dark)) {
    assert.equal(night, TONE_MARK[tone as keyof typeof TONE_MARK], `${tone} by night is the mark it was`)
    assert.equal(daylightColor(night), MARKS.light[tone as keyof typeof MARKS.light], `${tone} by day`)
  }
})

test('a treemap keeps its leaf labels ivory on the deepened tiles, and maps the rest', () => {
  const day = daylight({ series: [{ type: 'treemap', label: { color: '#efe3c8' }, upperLabel: { color: '#c8bb9b' }, levels: [{ itemStyle: { borderColor: '#0a1828' } }] }] })
  assert.equal(day.series[0].label.color, '#efe3c8')
  assert.equal(day.series[0].upperLabel.color, THEME.light.inkDim)
  assert.equal(day.series[0].levels[0].itemStyle.borderColor, THEME.light.hull)
})

test('night, daylight, or the system: night is the default, the system light or not; the address chooses for one page', () => {
  // Nothing chosen: night, whatever the system says.
  assert.equal(choiceOf(null, null), 'dark')
  assert.equal(modeOf(choiceOf(null, null), true), 'dark')
  // Kept in this browser; the address's, for one page, over it; anything else is not a choice.
  assert.equal(choiceOf(null, 'system'), 'system')
  assert.equal(choiceOf('light', 'system'), 'light')
  assert.equal(choiceOf('system', 'dark'), 'system')
  assert.equal(choiceOf('noon', 'light'), 'light')
  assert.equal(choiceOf('noon', 'dusk'), 'dark')
  // The system's follows it.
  assert.deepEqual([modeOf('system', true), modeOf('system', false), modeOf('light', false), modeOf('dark', true)], ['light', 'dark', 'light', 'dark'])
  // The rail's button: night, daylight, the system's, night.
  assert.deepEqual(CHOICES.map(nextChoice), ['light', 'system', 'dark'])
})

test("the page's first paint chooses as the app does: public/mode.js against choiceOf and modeOf", () => {
  const src = readFileSync(new URL('../public/mode.js', import.meta.url), 'utf8')
  const words = [null, 'dark', 'light', 'system', 'noon']
  for (const asked of words) {
    for (const kept of words) {
      for (const light of [true, false]) {
        const classes = new Set<string>()
        const document = { documentElement: { classList: { add: (c: string) => classes.add(c), remove: (c: string) => classes.delete(c) } } }
        const location = { search: asked === null ? '' : `?mode=${asked}` }
        const localStorage = { getItem: () => kept }
        const window = { matchMedia: (q: string) => ({ matches: q === '(prefers-color-scheme: light)' && light }) }
        new Function('document', 'location', 'localStorage', 'window', src)(document, location, localStorage, window)
        const want = modeOf(choiceOf(asked, kept), light)
        assert.equal(classes.has('light'), want === 'light', `asked ${asked}, kept ${kept}, the system ${light ? 'light' : 'dark'}`)
      }
    }
  }
})

test("the look's three choices: the rail's button steps through them, the palette lists each, the system is followed", () => {
  const shell = readFileSync(new URL('../src/components/Shell.tsx', import.meta.url), 'utf8')
  assert.match(shell, /onClick=\{\(\) => setChoice\(nextChoice\(choice\)\)\}/)
  assert.match(shell, /\{CHOICES\.map\(\(c\) => \{/)
  assert.match(shell, /system: \{ Icon: SunMoon, name: 'Follow the system'/)
  const mode = readFileSync(new URL('../src/lib/mode.ts', import.meta.url), 'utf8')
  assert.match(mode, /window\.matchMedia\(SYSTEM_LIGHT\)\.addEventListener\('change'/)
  assert.match(mode, /if \(st\.choice !== 'system' \|\| mode === st\.mode\) return/)
  assert.match(mode, /const choice = initial\(\)\nconst start = modeOf\(choice, systemLight\(\)\)/)
})
