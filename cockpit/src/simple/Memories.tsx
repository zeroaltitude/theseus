// Memories, the simple page: what Theseus remembers and what a turn sees of it. A search over everything it holds, a
// person's and a topic's page, what one turn was shown, then the explorer of imported episodes and the shelves the
// imported episodes are sorted onto (the route keeps the name `books`). The first four are this page's own
// (`memories/`); the last two are the Ship's views, hosted whole.
import { lazy } from 'react'
import { Hosted } from './Hosted'

const TABS = [
  { id: 'search', label: 'Search', hint: 'one box over people, topics, remembered notes and imported conversations', view: lazy(() => import('./memories/Search')) },
  { id: 'people', label: 'People', hint: 'each person Theseus has on file, and the conversations they are in', view: lazy(() => import('./memories/Kinds').then((m) => ({ default: m.People }))) },
  { id: 'topics', label: 'Topics', hint: 'each topic conversations are sorted under, and what is filed beneath it', view: lazy(() => import('./memories/Kinds').then((m) => ({ default: m.Topics }))) },
  { id: 'turn', label: 'A turn', hint: 'exactly what Theseus put in front of the model for one turn, and why', view: lazy(() => import('./memories/Turn')) },
  { id: 'context', label: 'Memory and context', hint: 'episodes, topics, and exactly what one turn was shown', view: lazy(() => import('@/views/Context')) },
  { id: 'books', label: 'Shelves', hint: 'the imported episodes, by the shelf each is on; written books are planned, not built', view: lazy(() => import('@/views/Books')) },
] as const

export default function SimpleMemories() {
  return <Hosted title="Memories" sub="What Theseus remembers, and what each turn sees of it." tabs={TABS} />
}
