// The Ship's state filter (theseus-emqx): the fleet the engine draws is the whole model's view under the filter, every
// vessel in the slot the whole model gave it, so switching Live, Quiet, Retired and All never reshuffles the sea: a
// vessel the filter hides leaves its slot empty, and one it shows comes back to the same water. Pure: `npm test` runs
// it (test/sessionState.test.ts).
import type { ShipModel } from './model.ts'

/** What the filter hides, for the fleet's stats: conversations and tasks. */
export interface Hidden { sessions: number; tasks: number }

/** `m` with only the vessels `keep` names, its lights, benches, tethers, currents and formations re-indexed; the
 *  bounds stay the whole fleet's, so a fit frames the same sea. `m` itself when it keeps everything. */
export function viewOf(m: ShipModel, keep: Set<string>): ShipModel {
  if (m.vessels.every((v) => keep.has(v.id))) return m
  const vi = new Map<number, number>()
  const vessels: ShipModel['vessels'] = []
  m.vessels.forEach((v, i) => {
    if (!keep.has(v.id)) return
    vi.set(i, vessels.length)
    vessels.push(v)
  })
  const li = new Map<number, number>()
  const lights: ShipModel['lights'] = []
  m.lights.forEach((l, i) => {
    if (!vi.has(l.vessel)) return
    li.set(i, lights.length)
    lights.push(l)
  })
  const bi = new Map<number, number>()
  const benches: ShipModel['benches'] = []
  m.benches.forEach((b, i) => {
    const v = vi.get(b.vessel)
    if (v === undefined) return
    bi.set(i, benches.length)
    benches.push({ ...b, vessel: v, lights: b.lights.map((x) => li.get(x)!).filter((x) => x !== undefined) })
  })
  for (let k = 0; k < lights.length; k++) {
    const l = lights[k]
    lights[k] = { ...l, vessel: vi.get(l.vessel)!, bench: l.bench >= 0 ? bi.get(l.bench) ?? -1 : -1 }
  }
  for (let k = 0; k < vessels.length; k++) {
    const v = vessels[k]
    vessels[k] = { ...v, benches: v.benches.map((b) => bi.get(b)!).filter((b) => b !== undefined), activeBench: v.activeBench >= 0 ? bi.get(v.activeBench) ?? -1 : -1 }
  }
  const tethers = m.tethers.filter((t) => vi.has(t.from) && vi.has(t.to)).map((t) => ({ ...t, from: vi.get(t.from)!, to: vi.get(t.to)! }))
  const currents = m.currents.filter((c) => vi.has(c.from) && vi.has(c.to)).map((c) => ({
    ...c, from: vi.get(c.from)!, to: vi.get(c.to)!,
    fromLight: c.fromLight === undefined ? undefined : li.get(c.fromLight), toLight: c.toLight === undefined ? undefined : li.get(c.toLight),
  }))
  const formations = m.formations
    .map((f) => ({ ...f, members: f.members.filter((x) => vi.has(x)).map((x) => vi.get(x)!) }))
    .filter((f) => f.members.length > 0)
  const byId = new Map(vessels.map((v, i) => [v.id, i]))
  const lightById = new Map(lights.map((l, i) => [l.id, i]))
  return {
    vessels, lights, benches, tethers, currents, formations, bounds: m.bounds, byId, lightById,
    stats: {
      sessions: vessels.length,
      nodes: lights.length,
      tasks: vessels.filter((v) => v.kind === 'task').length,
      running: vessels.filter((v) => v.rig === 'sail').length,
      waiting: vessels.filter((v) => v.rig === 'lantern').length,
      held: vessels.filter((v) => v.hold).length,
      l1: lights.filter((l) => l.l1).length,
      external: lights.filter((l) => l.external).length,
    },
  }
}

/** What the view leaves out of the whole model. */
export function hiddenOf(whole: ShipModel, view: ShipModel): Hidden {
  const tasks = whole.stats.tasks - view.stats.tasks
  return { sessions: whole.stats.sessions - view.stats.sessions - tasks, tasks }
}
