// Configuration, the simple page: what can be changed about Theseus while it runs. Who it is and what it knows
// about, the rules it keeps, what it may do, and the machine's own settings.
import { lazy } from 'react'
import { PersonasPanel } from '@/components/Personas'
import { Hosted } from './Hosted'

/** The personas alone: who Theseus is in each place, and the file each one's character is written in. */
function Personality() {
  return <div className="flex flex-col gap-3"><PersonasPanel disabled={false} /></div>
}

const TABS = [
  { id: 'personality', label: 'Personality', hint: 'the personas: who Theseus is in each place, and the file each one’s character is written in', view: Personality },
  { id: 'ontology', label: 'Ontology and people', hint: 'the kinds of thing Theseus keeps track of, the entries under each, the people, and its guidance', view: lazy(() => import('@/views/Ontology')) },
  { id: 'standing', label: 'Standing rules', hint: 'the instructions Theseus carries into every turn', view: lazy(() => import('@/components/StandingRecords').then((m) => ({ default: m.StandingRecords }))) },
  { id: 'policy', label: 'Policy', hint: 'what Theseus may do without asking, and what tightened it', view: lazy(() => import('@/views/Policy')) },
  { id: 'approvals', label: 'Tools and approvals', hint: 'each tool’s posture, the approvals waiting and the wakes set', view: lazy(() => import('@/views/Actions')) },
  { id: 'system', label: 'Models and system', hint: 'the profiles, the catalog, the configuration file and who may open this page', view: lazy(() => import('@/views/Systems')) },
] as const

export default function SimpleConfig() {
  return <Hosted title="Configuration" sub="Everything about Theseus you can change while it runs." tabs={TABS} />
}
