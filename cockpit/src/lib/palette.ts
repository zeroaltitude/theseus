// The chart method's palette checks (theseus-hnof.4), so `npm test` holds every palette the charts draw with to them, in
// the gate and in CI (test/palette.test.ts): each colour inside the lightness band of its mode and above the chroma
// floor, neighbours that stay apart under protanopia and deuteranopia (the Machado, Oliveira and Fernandes 2009
// simulation at severity 1.0) and for normal vision, and each colour's contrast with the surface it is drawn on. A
// colour difference is the OKLab distance ×100 (Björn Ottosson's OKLab); a contrast is WCAG 2's. The thresholds are the
// method's (`references/color-formula.md` of the data-viz method). Pure, with no import, so `node --test` runs it as it is.

export type Mode = 'dark' | 'light'

/** OKLCH lightness a series colour keeps to, per mode. */
export const BAND: Record<Mode, readonly [number, number]> = { dark: [0.48, 0.67], light: [0.43, 0.77] }
/** Below this OKLCH chroma a hue reads as gray and stops telling series apart. */
export const CHROMA_FLOOR = 0.1
/** Neighbours under simulated colour blindness: at the target or over passes; between the floor and the target is legal
 *  only with a second encoding (a label, a gap, a shape); under the floor fails. */
export const CVD_TARGET = 8
export const CVD_FLOOR = 6
/** Neighbours for normal vision: a hard floor, which a second encoding does not excuse. */
export const NORMAL_FLOOR = 15
/** A mark's contrast with its surface; under it, the values must be readable another way (direct labels, the table). */
export const CONTRAST_MIN = 3

type RGB = readonly [number, number, number]
type Kind = 'protan' | 'deutan' | 'tritan'

/** The dichromat simulations, applied to linear RGB. */
const MACHADO: Record<Kind, readonly RGB[]> = {
  protan: [[0.152286, 1.052583, -0.204868], [0.114503, 0.786281, 0.099216], [-0.003882, -0.048116, 1.051998]],
  deutan: [[0.367322, 0.860646, -0.227968], [0.280085, 0.672501, 0.047413], [-0.01182, 0.04294, 0.968881]],
  tritan: [[1.255528, -0.076749, -0.178779], [-0.078411, 0.930809, 0.147602], [0.004733, 0.691367, 0.3039]],
}

/** A `#rrggbb` colour's channels, each 0 to 1 and linear (gamma removed). */
export function linearRgb(hex: string): RGB {
  const h = hex.trim().replace(/^#/, '')
  if (!/^[0-9a-fA-F]{6}$/.test(h)) throw new Error(`not a #rrggbb colour: ${hex}`)
  const ch = (i: number) => {
    const c = parseInt(h.slice(i, i + 2), 16) / 255
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
  }
  return [ch(0), ch(2), ch(4)]
}

/** WCAG 2's relative luminance. */
export function luminance(hex: string): number {
  const [r, g, b] = linearRgb(hex)
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

/** WCAG 2's contrast ratio of two colours, 1 to 21. */
export function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x)
  return (hi + 0.05) / (lo + 0.05)
}

function oklabOfLinear([r, g, b]: RGB): RGB {
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ]
}

/** A colour's OKLCH lightness and chroma. */
export function oklch(hex: string): { L: number; C: number } {
  const [L, a, b] = oklabOfLinear(linearRgb(hex))
  return { L, C: Math.hypot(a, b) }
}

function seen(hex: string, kind?: Kind): RGB {
  const c = linearRgb(hex)
  if (!kind) return c
  const clamp = (v: number) => Math.max(0, Math.min(1, v))
  const M = MACHADO[kind]
  return [0, 1, 2].map((i) => clamp(M[i][0] * c[0] + M[i][1] * c[1] + M[i][2] * c[2])) as unknown as RGB
}

/** How far apart two colours look, as OKLab distance ×100: to a reader with full colour vision, or with `kind`. */
export function deltaE(a: string, b: string, kind?: Kind): number {
  const p = oklabOfLinear(seen(a, kind)), q = oklabOfLinear(seen(b, kind))
  return 100 * Math.hypot(p[0] - q[0], p[1] - q[1], p[2] - q[2])
}

export interface Pair { a: string; b: string; d: number; kind?: Kind }

export interface PaletteReport {
  /** Colours outside the mode's lightness band. */
  offBand: { color: string; L: number }[]
  /** Colours under the chroma floor. */
  lowChroma: { color: string; C: number }[]
  /** The closest pair under protanopia or deuteranopia, and its verdict; tritanopia's closest, for the record. */
  cvd: { worst: Pair; state: 'pass' | 'floor' | 'fail'; tritan: number }
  /** The closest pair for normal vision. */
  normal: { worst: Pair; ok: boolean }
  /** Colours under 3:1 against the surface: legal only with direct labels or a table view. */
  lowContrast: { color: string; ratio: number }[]
  /** No hard failure: band, chroma, a CVD pair under the floor, or the normal-vision floor. */
  ok: boolean
}

/** The checks for a series palette, assigned in this order: `adjacent` pairs for stacks, bars, and lines (only
 *  neighbours touch); `all` pairs for scatter and small multiples, where any two marks can meet. */
export function validatePalette(palette: readonly string[], opts: { mode: Mode; surface: string; pairs?: 'adjacent' | 'all' }): PaletteReport {
  const [lo, hi] = BAND[opts.mode]
  const offBand = palette.map((color) => ({ color, L: oklch(color).L })).filter((x) => x.L < lo || x.L > hi)
  const lowChroma = palette.map((color) => ({ color, C: oklch(color).C })).filter((x) => x.C < CHROMA_FLOOR)
  const n = palette.length
  const pairs: [number, number][] = []
  if (opts.pairs === 'all') { for (let i = 0; i < n; i++) for (let j = i + 1; j < n; j++) pairs.push([i, j]) }
  else for (let i = 0; i + 1 < n; i++) pairs.push([i, i + 1])
  const closest = (kind?: Kind): Pair => pairs
    .map(([i, j]) => ({ a: palette[i], b: palette[j], d: deltaE(palette[i], palette[j], kind), kind }))
    .reduce((w, p) => (p.d < w.d ? p : w), { a: '', b: '', d: Infinity, kind } as Pair)
  const protan = closest('protan'), deutan = closest('deutan')
  const worst = deutan.d < protan.d ? deutan : protan
  const state = worst.d >= CVD_TARGET ? 'pass' : worst.d >= CVD_FLOOR ? 'floor' : 'fail'
  const normal = closest()
  const lowContrast = palette.map((color) => ({ color, ratio: contrast(color, opts.surface) })).filter((x) => x.ratio < CONTRAST_MIN)
  return {
    offBand, lowChroma, cvd: { worst, state, tritan: closest('tritan').d }, normal: { worst: normal, ok: normal.d >= NORMAL_FLOOR },
    lowContrast, ok: !offBand.length && !lowChroma.length && state !== 'fail' && normal.d >= NORMAL_FLOOR,
  }
}
