// Telemetry, the simple page: every instrument the Ship keeps, in the order an operator needs them. What is
// happening now comes first, then what it costs, then the machine, then the record.
import { lazy } from 'react'
import { Hosted } from './Hosted'

const TABS = [
  { id: 'now', label: 'Now', hint: 'latency, turns, tools, context and rate-limit headroom', view: lazy(() => import('@/views/Bridge').then((m) => ({ default: m.Bridge }))) },
  { id: 'spend', label: 'Spend', hint: 'where the money goes, by model, by kind of token and by session', view: lazy(() => import('@/views/Economics')) },
  { id: 'budgets', label: 'Budgets', hint: 'the money river, the pace and each budget', view: lazy(() => import('@/views/Money')) },
  { id: 'system', label: 'System', hint: 'the daemon, the kernel, its bindings, secrets by name and its store', view: lazy(() => import('@/views/Systems')) },
  { id: 'safety', label: 'Safety', hint: 'the gate, the sandbox, the broker, the places and what left the machine', view: lazy(() => import('@/views/Boundaries')) },
  { id: 'judgment', label: 'Judgment', hint: 'what the small model decided, and how often it was right', view: lazy(() => import('@/views/Judgment')) },
  { id: 'ledger', label: 'Ledger', hint: 'every row Theseus wrote, searchable', view: lazy(() => import('@/views/Ledger')) },
] as const

export default function SimpleTelemetry() {
  return <Hosted title="Telemetry" sub="Every measurement Theseus keeps of itself, the most urgent first." tabs={TABS} />
}
