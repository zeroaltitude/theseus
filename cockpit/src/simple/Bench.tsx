// Benchmarks, the simple page: how Theseus measures against the harnesses it is compared with, and every run.
import { lazy } from 'react'
import { Hosted } from './Hosted'

const TABS = [
  { id: 'runs', label: 'Runs', hint: 'where Theseus stands, and every published run', view: lazy(() => import('@/views/Benchmarks')) },
  { id: 'speed', label: 'Speed', hint: 'starts, harness overhead and the write path, against their budgets', view: lazy(() => import('@/views/Speed')) },
] as const

export default function SimpleBench() {
  return <Hosted title="Benchmarks" sub="How Theseus measures up: the published runs, and the speed it holds itself to." tabs={TABS} />
}
