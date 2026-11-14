// Benchmarks, the simple page: how Theseus measures against the harnesses it is compared with, and every run.
import { lazy } from 'react'
import { Hosted } from './Hosted'

const TABS = [
  { id: 'standing', label: 'At a glance', hint: 'one line a benchmark: where Theseus stands against the best harness measured beside it', view: lazy(() => import('./BenchBoard')) },
  { id: 'runs', label: 'Every run', hint: 'the frontier of measured harnesses, every published run and its report', view: lazy(() => import('@/views/Benchmarks')) },
  { id: 'speed', label: 'Speed', hint: 'starts, harness overhead and the write path, against their budgets', view: lazy(() => import('@/views/Speed')) },
] as const

export default function SimpleBench() {
  return <Hosted title="Benchmarks" sub="How Theseus measures up: the published runs, and the speed it holds itself to." tabs={TABS} />
}
