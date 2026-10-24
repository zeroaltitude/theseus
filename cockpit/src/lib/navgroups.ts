// The nav rail's accordion groups (theseus-onxwh). Pure: which view is in which group, which group a path is in, and
// which groups are open. The icons stay in the shell; a test holds every view of the rail to exactly one group here.

export interface NavGroup { id: string; label: string; items: readonly string[] }

/** Short labels: the rail is 68 px wide. Grouped by the question each answers: what is happening now, what Theseus knows, what it costs and how it chooses,
 *  what holds it back, how the machine is, and how it measures. The Ship leads the first and is the landing view. */
export const NAV_GROUPS: readonly NavGroup[] = [
  { id: 'live', label: 'Live', items: ['/ship', '/ops', '/bridge', '/fleet', '/actions'] },
  { id: 'memory', label: 'Memory', items: ['/ontology', '/context', '/books'] },
  { id: 'money', label: 'Spend', items: ['/money', '/economics', '/judgment'] },
  { id: 'safety', label: 'Safety', items: ['/boundaries', '/policy'] },
  { id: 'system', label: 'System', items: ['/ledger', '/systems'] },
  { id: 'bench', label: 'Bench', items: ['/benchmarks', '/speed'] },
]

/** Views with no rail item of their own that belong to a group: a session's deck and a thread's page are live work. */
const ALSO: Record<string, string> = { '/session': 'live', '/threads': 'live', '/': 'live' }

/** The group holding the view at `pathname` (the longest item that prefixes it), or null. */
export function groupOf(pathname: string): string | null {
  let best: { id: string; len: number } | null = null
  for (const g of NAV_GROUPS) {
    for (const to of g.items) {
      if ((pathname === to || pathname.startsWith(`${to}/`)) && (!best || to.length > best.len)) best = { id: g.id, len: to.length }
    }
  }
  if (best) return best.id
  const head = `/${pathname.split('/')[1] ?? ''}`
  return ALSO[head] ?? null
}

/** The operator's own choices, as stored: group id → open (true) or folded (false). Anything else is ignored. */
export function parseChoices(raw: string | null): Record<string, boolean> {
  if (!raw) return {}
  try {
    const v: unknown = JSON.parse(raw)
    if (!v || typeof v !== 'object' || Array.isArray(v)) return {}
    const out: Record<string, boolean> = {}
    for (const g of NAV_GROUPS) { const b = (v as Record<string, unknown>)[g.id]; if (typeof b === 'boolean') out[g.id] = b }
    return out
  } catch { return {} }
}

/** Is `id` open? As the operator left it (folded when never chosen), and also open when this page has been in it
 *  (`visited`: the group of the view on load, and of each view since), unless the operator folded it since (`shut`). */
export function isOpen(choices: Record<string, boolean>, shut: ReadonlySet<string>, visited: ReadonlySet<string>, id: string): boolean {
  return !shut.has(id) && (choices[id] === true || visited.has(id))
}

/** The press on a group's header: what is kept, and what this page folded. */
export function pressed(choices: Record<string, boolean>, shut: ReadonlySet<string>, visited: ReadonlySet<string>, id: string): { choices: Record<string, boolean>; shut: Set<string> } {
  const open = isOpen(choices, shut, visited, id)
  const s = new Set(shut)
  if (open) s.add(id); else s.delete(id)
  return { choices: { ...choices, [id]: !open }, shut: s }
}

/** The page enters the view at `pathname`: its group is open (and no longer folded by an earlier press). */
export function entered(shut: ReadonlySet<string>, visited: ReadonlySet<string>, pathname: string): { shut: Set<string>; visited: Set<string> } {
  const g = groupOf(pathname)
  const v = new Set(visited)
  const s = new Set(shut)
  if (g) { v.add(g); s.delete(g) }
  return { shut: s, visited: v }
}
