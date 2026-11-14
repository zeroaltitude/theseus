// Memories, the simple page: what Theseus remembers and what a turn sees of it. The explorer of episodes, topics and
// one turn's context, and the books the imported episodes are filed in.
import { lazy } from 'react'
import { Hosted } from './Hosted'

const TABS = [
  { id: 'context', label: 'Memory and context', hint: 'episodes, topics, and exactly what one turn was shown', view: lazy(() => import('@/views/Context')) },
  { id: 'books', label: 'Books', hint: 'the imported episodes, by the book each is filed in', view: lazy(() => import('@/views/Books')) },
] as const

export default function SimpleMemories() {
  return <Hosted title="Memories" sub="What Theseus remembers, and what each turn sees of it." tabs={TABS} />
}
