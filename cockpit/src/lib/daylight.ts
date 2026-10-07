// The daylight mode's colours (theseus-hnof.5): the night theme's colours, each paired with its step by day, and the map
// a chart's option goes through when the cockpit is in daylight.
//
// - The state tones by day keep their hues, deepened (OKLCH lightness lowered, chroma held) to the lightest step that
//   reads as small text on the ivory panel face and on the page's parchment, 4.6:1 or more on both (#fbf8f1, #efe8d8);
//   each sits in the validator's light band for marks (L 0.50 to 0.54). As a set they fail the categorical checks, as the
//   night tones do: they carry state, always with an icon, a shape or a word (palette/ in the lane's report).
// - A chart's series keep their slots: the categorical palette's light steps and the tones' light series steps, which
//   lib/viz.ts keeps beside the dark ones (read here, never changed), validated on the daylight surfaces.
// - The theme's own colours (the night glass, the brass, the ivory ink) map to their daylight inks and papers.
// - A night neutral no pair names (an ivory label, a dark glass) takes the daylight ink or paper of its strength.
// Alpha stays: a brass hairline at 13% by night is a darker brass at 13% by day.
//
// Pure (viz.ts imports a type only), so `node --test` runs it (test/daylight.test.ts).
import { CATEGORICAL, CHROME, OTHER, TONE_SERIES } from './viz.ts'
import type { Tone } from './taxonomy'

export type Mode = 'dark' | 'light'

/** The state tones, by night (the theme's, index.css) and by day. Idle is the faint ink of each mode. */
export const TONES: Record<Mode, Record<Tone, string>> = {
  dark: { live: '#22d3ee', ok: '#34d399', wait: '#fbbf24', fault: '#fb7185', model: '#a78bfa', tool: '#38bdf8', think: '#e879f9', money: '#facc15', idle: '#9c907a' },
  light: { live: '#017282', ok: '#067551', wait: '#846205', fault: '#ba3550', model: '#7254bd', tool: '#066f96', think: '#a233b3', money: '#7d6501', idle: '#655c4b' },
}

/** A tone drawn as a mark (viz.ts's `TONE_MARK`) in each mode: the tones' series steps, idle the de-emphasis gray.
 *  mode.ts sets `TONE_MARK` to the mode's, so the charts drawn outside an ECharts option (the fleet's bar, the Ledger's
 *  tiles, the fuel meters, where a turn's time went, the flame's spans), which set it inline where `daylight()` never
 *  reaches, show by day the colour `daylight()` gives the same series in a chart. */
export const MARKS: Record<Mode, Record<Tone, string>> = {
  dark: { ...Object.fromEntries(TONE_SERIES.dark), idle: OTHER } as Record<Tone, string>,
  light: { ...Object.fromEntries(TONE_SERIES.light), idle: OTHER } as Record<Tone, string>,
}

/** The theme's own colours by night and by day: the papers, the brass, the inks (index.css's tokens). */
export const THEME: Record<Mode, Record<string, string>> = {
  dark: {
    void: '#030912', deck: '#06101d', hull: '#0a1828', navy: '#13314d', ink: '#efe3c8', inkDim: '#c8bb9b', inkFaint: '#9c907a',
    gold: '#d6a548', brass: '#b08d57', magenta: '#f472b6', glass: '#0c1c2f', glassDeep: '#071322',
  },
  light: {
    void: '#efe8d8', deck: '#f6f0e3', hull: '#fbf8f1', navy: '#e4d9c1', ink: '#1d2633', inkDim: '#3b4452', inkFaint: '#655c4b',
    gold: '#7a520b', brass: '#785c30', magenta: '#ad2d6e', glass: '#fffdf8', glassDeep: '#f4eddf',
  },
}

type Rgb = [number, number, number]

const hex6 = (h: string): string | null => {
  const m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(h)
  if (!m) return null
  const s = m[1].length === 3 ? [...m[1]].map((c) => c + c).join('') : m[1]
  return `#${s.toLowerCase()}`
}
const toRgb = (h: string): Rgb => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16)) as Rgb
const key = (r: Rgb) => r.join(',')

/** Night colour → daylight colour, by their RGB. Built from the pairs above and viz.ts's own. */
const PAIRS = new Map<string, Rgb>()
function pair(dark: string, light: string) {
  const d = hex6(dark), l = hex6(light)
  if (d && l && !PAIRS.has(key(toRgb(d)))) PAIRS.set(key(toRgb(d)), toRgb(l))
}
for (const t of Object.keys(TONES.dark) as Tone[]) pair(TONES.dark[t], TONES.light[t])
for (const k of Object.keys(THEME.dark)) pair(THEME.dark[k], THEME.light[k])
CATEGORICAL.dark.forEach((c, i) => pair(c, CATEGORICAL.light[i]))
for (const [tone, c] of TONE_SERIES.dark) {
  const l = TONE_SERIES.light.find(([t]) => t === tone)
  if (l) pair(c, l[1])
}
pair(CHROME.dark.text, THEME.light.ink)
pair(CHROME.dark.secondary, THEME.light.inkDim)
pair(CHROME.dark.muted, THEME.light.inkFaint)
pair(CHROME.dark.surface, THEME.light.hull)
// The charts' brass hairlines and gold rims (rgb(176 141 87), rgb(214 165 72) at any alpha), the old tooltip's glass
// (rgb(6 15 27)), and the plank gold.
pair('#b08d57', THEME.light.brass)
pair('#060f1b', THEME.light.glass)
pair('#e2ae4f', '#8a5d0e')
pair('#ffd27a', '#7a520b')
// The other inks and brasses the views name by hex: the graph's and the money river's labels, the river's banks.
pair('#ddd0b0', THEME.light.inkDim)
pair('#d9cba8', THEME.light.inkDim)
pair('#c9a467', '#8a5d0e')
pair('#f3d9a4', THEME.light.gold)
pair('#020710', THEME.light.glass)
pair('#5eead4', '#0b7766')

/** OKLab lightness and chroma of an sRGB colour. */
function okLC([r, g, b]: Rgb): [number, number] {
  const lin = (c: number) => { const v = c / 255; return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4 }
  const [R, G, B] = [lin(r), lin(g), lin(b)]
  const l = Math.cbrt(0.4122214708 * R + 0.5363325363 * G + 0.0514459929 * B)
  const m = Math.cbrt(0.2119034982 * R + 0.6806995451 * G + 0.1073969566 * B)
  const s = Math.cbrt(0.0883024619 * R + 0.2817188376 * G + 0.6299787005 * B)
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s
  const a = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s
  const bb = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s
  return [L, Math.hypot(a, bb)]
}

/** The daylight colours themselves: never moved again, so an option drawn for the day passes through as it is. */
const DAY = new Set<string>([
  ...Object.values(TONES.light), ...Object.values(THEME.light), ...CATEGORICAL.light, ...TONE_SERIES.light.map(([, c]) => c),
  ...Object.values(CHROME.light),
].map((c) => hex6(c)).filter((c): c is string => !!c).map((c) => key(toRgb(c))))

/** A night neutral no pair names: a light ivory or grey (an ink by night) takes the daylight ink of its strength; a dark
 *  glass (a surface by night) takes the daylight paper. Black and white, colours, the middle greys, and the daylight
 *  colours themselves stay. */
function neutral(r: Rgb): Rgb | null {
  if (DAY.has(key(r))) return null
  const [L, C] = okLC(r)
  if (C >= 0.05 || L >= 0.995 || L <= 0.05) return null
  if (L >= 0.85) return toRgb(THEME.light.ink)
  if (L >= 0.72) return toRgb(THEME.light.inkDim)
  if (L >= 0.62) return toRgb(THEME.light.inkFaint)
  if (L <= 0.3) return toRgb(THEME.light.glass)
  return null
}

const css = (r: Rgb, a: number | null) => (a === null ? `#${r.map((v) => v.toString(16).padStart(2, '0')).join('')}` : `rgba(${r.join(',')},${a})`)

/** One colour by day: a night colour's daylight step, at its own alpha; any other colour, or anything that is not a
 *  colour, as it is. Reads `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb()` and `rgba()` (commas, or spaces and a slash). */
export function daylightColor(c: string): string {
  const s = c.trim()
  if (s.startsWith('#')) {
    const alpha = s.length === 9 ? s.slice(7) : ''
    const base = hex6(alpha ? s.slice(0, 7) : s)
    const to = base && (PAIRS.get(key(toRgb(base))) ?? neutral(toRgb(base)))
    return to ? `${css(to, null)}${alpha.toLowerCase()}` : c
  }
  const m = /^rgba?\(\s*(\d+)[\s,]+(\d+)[\s,]+(\d+)\s*(?:[,/]\s*([\d.]+%?)\s*)?\)$/i.exec(s)
  if (!m) return c
  const rgb: Rgb = [Number(m[1]), Number(m[2]), Number(m[3])]
  const to = PAIRS.get(key(rgb)) ?? neutral(rgb)
  if (!to) return c
  const a = m[4] === undefined ? null : m[4].endsWith('%') ? Number(m[4].slice(0, -1)) / 100 : Number(m[4])
  return css(to, a)
}

const COLOUR_IN_TEXT = /#[0-9a-f]{3,8}\b|rgba?\([^)]*\)/gi

/** A chart's option by day: every colour in it taken to its daylight step (in plain objects and arrays, at any depth;
 *  in `extraCssText`, inside the text). Functions, class instances, and strings that are not colours stay as they are.
 *  A treemap's leaf labels keep their night ivory: they sit on the tiles' solid fills, which deepen by day. */
export function daylight<T>(option: T): T {
  const walk = (v: unknown, k?: string, onTiles = false): unknown => {
    if (typeof v === 'string') return k === 'extraCssText' ? v.replace(COLOUR_IN_TEXT, (x) => daylightColor(x)) : daylightColor(v)
    if (Array.isArray(v)) return v.map((x) => walk(x, undefined, onTiles))
    if (v && typeof v === 'object') {
      const proto = Object.getPrototypeOf(v)
      if (proto !== Object.prototype && proto !== null) return v
      const tiles = onTiles || (v as { type?: unknown }).type === 'treemap'
      const out: Record<string, unknown> = {}
      for (const [kk, x] of Object.entries(v)) out[kk] = tiles && kk === 'label' ? x : walk(x, kk, tiles)
      return out
    }
    return v
  }
  return walk(option) as T
}
