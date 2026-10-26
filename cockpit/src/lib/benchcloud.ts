// The data behind the arm points (theseus-2nscq): a point on the frontier is one arm in one run, many trials folded
// into it. That fold is the right unit for a comparison of arms and stays the claim; this is the finer unit drawn
// faintly behind it (per task where the set has several, per trial where it has one), and the line that says so.
// Pure: the frontier and the verdict are computed from the arm points alone and never see these.
import { property, type Point, type Unit } from './bench.ts'

/** The finer unit for a set: per task when its arms ran more than one task, else per trial. */
export const cloudUnit = (arms: readonly Point[]): Unit => (Math.max(0, ...arms.map((p) => p.tasks)) > 1 ? 'task' : 'trial')

const plural = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString('en-US')} ${n === 1 ? one : many}`

/** The one line that states the fold, whether the cloud is drawn or not. Every number is counted from the points. */
export function cloudCaption(arms: readonly Point[], cloud: readonly Point[], unit: Unit): string {
  const trials = arms.reduce((a, p) => a + p.n, 0)
  return `a point is one arm in one run: ${plural(trials, 'trial')} folded into ${plural(arms.length, 'point')}; ${plural(cloud.length, `${unit} point`)} behind them`
}

/** A cloud point's tooltip lines: its task, the attempts it solved, and both axis values. No percentage: it is a few attempts. */
export function cloudTip(p: Point, xId: string, yId: string): { title: string; lines: string[] } {
  const k = Math.round((p.values.pass?.v ?? 0) * (p.values.pass?.n ?? 0)), n = p.values.pass?.n ?? p.n
  // A few attempts are counts, never a percentage: the pass axis reads "2 of 3" here.
  const fmt = (id: string) => { const v = p.values[id]; const pr = property(id); return !v || !pr ? 'not measured' : id === 'pass' ? `${k} of ${n}` : pr.format(v.v) }
  return {
    title: p.task || 'one trial',
    lines: [`${k} of ${n} attempt${n === 1 ? '' : 's'} solved`, `${property(yId)?.label}: ${fmt(yId)}`, `${property(xId)?.label}: ${fmt(xId)}`],
  }
}

/** The cloud's points that can be drawn on these axes: both values measured, and positive on a log axis. */
export function cloudDrawn(cloud: readonly Point[], xId: string, yId: string, xlog: boolean, ylog: boolean): { p: Point; x: number; y: number }[] {
  return cloud.flatMap((p) => {
    const x = p.values[xId]?.v, y = p.values[yId]?.v
    return x === undefined || y === undefined || (xlog && x <= 0) || (ylog && y <= 0) ? [] : [{ p, x, y }]
  })
}
