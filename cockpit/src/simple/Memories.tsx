// Memories, the simple page: what Theseus remembers and what a turn sees of it. The explorer of episodes, topics and
// one turn's context, and the shelves the imported episodes are sorted onto (the route keeps the name `books`).
import { lazy } from 'react'
import { Hosted } from './Hosted'

const TABS = [
  { id: 'context', label: 'Memory and context', hint: 'episodes, topics, and exactly what one turn was shown', view: lazy(() => import('@/views/Context')) },
  { id: 'books', label: 'Shelves', hint: 'the imported episodes, by the shelf each is on; written books are planned, not built', view: lazy(() => import('@/views/Books')) },
] as const

export default function SimpleMemories() {
  return <Hosted title="Memories" sub="What Theseus remembers, and what each turn sees of it." tabs={TABS} />
}
