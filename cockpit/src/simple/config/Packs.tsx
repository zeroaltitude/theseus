// Judging, the Configuration tab: how Theseus's small judging model is tuned while it runs. The ladder of each pack and
// its versions, with the promote and roll-back buttons: the Ship's own components (`PackLadder`, `PackVersions`), so
// the calls and their confirmations are the Judgment page's.
import { GitCompare, TrendingUp } from 'lucide-react'
import { PackLadder } from '@/components/PackLadder'
import { PackVersions } from '@/components/PackVersions'
import { Panel } from '@/components/ui'

export default function Packs() {
  return (
    <div className="flex flex-col gap-3">
      <Panel help="judgment.ladder" className="shrink-0" title="Ladder" icon={<TrendingUp size={14} />}>
        <PackLadder readOnly={false} />
      </Panel>
      <Panel help="judgment.versions" className="shrink-0" title="Versions" icon={<GitCompare size={14} />} bodyClassName="max-h-[360px] overflow-auto">
        <PackVersions readOnly={false} />
      </Panel>
    </div>
  )
}
