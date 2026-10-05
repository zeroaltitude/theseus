// `policy.explain` as the Policy view reads it (M7 42b): each tool's result and the layers that raised it, a place's
// counts by result, the tools in the order that asks most first, and the search and result filters. Pure, so `npm test`
// runs it.
import type { ExplainLayer, PlaceExplain, ToolExplain } from '@protocol'

/** The postures, strictest first: the order the tools are listed in. */
export const RESULTS = ['refused', 'approve', 'notify', 'open'] as const
export type Result = (typeof RESULTS)[number]

export const isResult = (s: string | null | undefined): s is Result => (RESULTS as readonly string[]).includes(s ?? '')

/** One tool, summarized: where it ended, and the layers that raised it on the way there. */
export interface ToolLine {
  tool: string
  class: string
  offered: boolean
  result: string
  /** The layers that raised the posture, in the gate's order. */
  raisedBy: string[]
  /** The settings those layers name (`[policy.tools] "proc.run" = approve`). */
  settings: string[]
  /** How many layers depend on the call, and are shown as conditions. */
  conditions: number
  refused?: string
}

/** A tightening layer counts only when it names who tightened (its `setting`): its `raised` flag is the whole decision's
 *  against the config's posture, which a call outside the roots or on an approve list raises with no tightening at all. */
const raisedBy = (l: ExplainLayer): boolean => l.raised === true && (l.layer !== 'tightening' || !!l.setting)

export const isTightened = (t: ToolExplain): boolean => t.layers.some((l) => l.layer === 'tightening' && !!l.setting)

export function toolLine(t: ToolExplain): ToolLine {
  const raised = t.layers.filter(raisedBy)
  return {
    tool: t.tool, class: t.class, offered: t.offered, result: t.result,
    raisedBy: raised.map((l) => l.layer),
    settings: raised.flatMap((l) => (l.setting ? [l.setting] : [])),
    conditions: t.conditions?.length ?? 0,
    ...(t.refused ? { refused: t.refused } : {}),
  }
}

/** A tool's family: its first segment (`fs`, `proc`), or its MCP server (`mcp:<server>`). */
export function familyOf(tool: string): string {
  if (tool.startsWith('mcp:')) return tool.split('/')[0]
  return tool.split('.')[0]
}

export interface PlaceSummary {
  /** Tools by result. */
  counts: Record<Result, number>
  /** Tools not offered here. */
  notOffered: number
  /** The tools a tightening raised. */
  tightened: string[]
}

export function placeSummary(p: PlaceExplain): PlaceSummary {
  const counts: Record<Result, number> = { refused: 0, approve: 0, notify: 0, open: 0 }
  const tightened: string[] = []
  let notOffered = 0
  for (const t of p.tools) {
    if (isResult(t.result)) counts[t.result]++
    if (!t.offered) notOffered++
    if (isTightened(t)) tightened.push(t.tool)
  }
  return { counts, notOffered, tightened }
}

/** The tools that ask most first, then by name. */
export function sortTools(tools: readonly ToolExplain[]): ToolExplain[] {
  const rank = (r: string) => { const i = (RESULTS as readonly string[]).indexOf(r); return i < 0 ? RESULTS.length : i }
  return [...tools].sort((a, b) => rank(a.result) - rank(b.result) || a.tool.localeCompare(b.tool))
}

/** The tools whose name, family, or settings hold the search, and (when asked) end at a result. */
export function filterTools(tools: readonly ToolExplain[], q: string, result: Result | null): ToolExplain[] {
  const needle = q.trim().toLowerCase()
  return tools.filter((t) => (!result || t.result === result) && (!needle || t.tool.toLowerCase().includes(needle) || t.layers.some((l) => (l.setting ?? '').toLowerCase().includes(needle))))
}
