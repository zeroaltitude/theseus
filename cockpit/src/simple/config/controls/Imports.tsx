// Imports, the control: each import the owner has brought in (`import.list`), and Erase, which cannot be undone, so it asks
// in words that give the counts and wants the import's name typed before it will go.
import { Download } from 'lucide-react'
import type { ImportListResult } from '@protocol'
import { Empty, Panel } from '@/components/ui'
import { when } from '@/lib/explorer'
import { useRpc } from '@/lib/rpc'
import { ActButton } from '../ActButton'
import { eraseWords, heldOf } from './logic'

export function Imports() {
  const { data } = useRpc<ImportListResult>('import.list', undefined, 30_000)
  const live = (data?.tags ?? []).filter((t) => heldOf(t) > 0)
  return (
    <Panel help="simple.config.imports" title="What was imported" icon={<Download size={13} />} bodyClassName="p-4">
      {!data ? <Empty>reading…</Empty> : !live.length ? <p className="text-[13px] text-ink-faint">Nothing imported is still held, so there is nothing to erase.</p> : (
        <ul className="divide-y divide-line">
          {live.map((t) => (
            <li key={t.tag} className="flex flex-wrap items-center gap-x-3 gap-y-1 py-2">
              <span className="num min-w-[160px] text-[13.5px] font-semibold text-ink">{t.tag}</span>
              <span className="min-w-[240px] flex-1 text-[12px] text-ink-faint">{heldOf(t).toLocaleString('en-US')} conversations held{t.erased ? `, ${t.erased.toLocaleString('en-US')} already erased` : ''} · updated {when(t.updated_ms)}</span>
              <ActButton verb="import_erase" params={{ tag: t.tag }} label="Erase…" effect={eraseWords(t)} confirmWord={t.tag} tone="fault" submit="Erase it for good"
                words={(r) => `erased ${r.sessions} conversations and ${r.nodes} memory entries`} />
            </li>
          ))}
        </ul>
      )}
    </Panel>
  )
}
