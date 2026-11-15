// The machinery's controls: restart a tool server (`mcp.restart`, over health's list of them), rebuild the search index
// (`index.rebuild`, with the index's state beside it), and let go of pinned models (`profile.release`, `profile.unpin`,
// over `profile.pins`). Each interrupts something that is running, so each asks first, in words that say what stops.
import { Boxes, Cpu, Search } from 'lucide-react'
import type { Health, ProfilePinsResult } from '@protocol'
import { Empty, Panel, Pill } from '@/components/ui'
import { useRpc } from '@/lib/rpc'
import { ActButton } from '../ActButton'
import { REBUILD_WORDS, UNPIN_WORDS, releaseWords, restartWords } from './logic'

export function Servers({ h }: { h?: Health }) {
  const servers = h?.mcp ?? []
  return (
    <Panel help="simple.config.servers" title="Tool servers" icon={<Boxes size={13} />} bodyClassName="p-4">
      {!h ? <Empty>reading…</Empty> : !servers.length ? <p className="text-[13px] text-ink-faint">Theseus has no tool server configured.</p> : (
        <ul className="divide-y divide-line">
          {servers.map((s) => (
            <li key={s.name} className="flex flex-wrap items-center gap-x-3 gap-y-1 py-2">
              <span className="min-w-[160px] text-[13.5px] font-semibold text-ink">{s.name}</span>
              <Pill tone={s.state === 'running' || s.state === 'ready' ? 'ok' : s.state === 'disabled' ? 'idle' : 'wait'}>{s.state}</Pill>
              <span className="min-w-[200px] flex-1 text-[12px] text-ink-faint">{s.tools} tools · {s.calls} calls · {s.errors} errors · {s.crashes} crashes{s.last_error ? ` · last error: ${s.last_error}` : ''}</span>
              <ActButton verb="mcp_restart" params={{ name: s.name }} label="Restart" effect={restartWords(s)} disabled={s.state === 'disabled'} submit="Yes, restart it" words={(r) => `restarted (it was ${r.was})`} />
            </li>
          ))}
        </ul>
      )}
    </Panel>
  )
}

export function Index({ h }: { h?: Health }) {
  const ix = h?.index
  return (
    <Panel help="simple.config.index" title="The search index" icon={<Search size={13} />} bodyClassName="flex flex-wrap items-center gap-3 p-4">
      <span className="text-[13px] text-ink-dim">{ix ? <>The index is <span className="font-semibold text-ink">{ix.state}</span>{ix.why ? `: ${ix.why}` : ''}{ix.status ? `, answering by ${ix.status.mode.replace('_', ' ')}` : ''}.</> : 'reading…'}</span>
      <span className="ml-auto" />
      <ActButton verb="index_rebuild" params={{}} label="Rebuild the index" effect={REBUILD_WORDS} tone="fault" submit="Yes, rebuild it" words={(r) => (r.accepted ? 'the rebuild has started' : 'not accepted')} />
    </Panel>
  )
}

export function Pins() {
  const { data } = useRpc<ProfilePinsResult>('profile.pins', undefined, 10_000)
  return (
    <Panel help="simple.config.pins" title="Models fixed in place" icon={<Cpu size={13} />} bodyClassName="flex flex-col gap-3 p-4">
      {!data ? <Empty>reading…</Empty> : (
        <>
          <p className="text-[13px] text-ink-dim">The live model is <span className="num font-semibold text-ink">{data.live}</span> (from {data.live_source}). {data.pinned_sessions + data.pinned_tasks ? `${data.pinned_sessions} conversations and ${data.pinned_tasks} tasks are fixed to a model of their own.` : 'No conversation or task is fixed to a model of its own.'}</p>
          <div className="flex flex-wrap items-center gap-2">
            <ActButton verb="profile_release" params={{}} label="Release the live model" effect={releaseWords(data.live)} submit="Yes, release it" words={(r) => (r.released ? `released; the live model is now ${r.live}` : 'nothing was held')} />
            <ActButton verb="profile_unpin" params={{ all: true }} label="Unpin every model" effect={UNPIN_WORDS} disabled={!data.pinned_sessions && !data.pinned_tasks} submit="Yes, unpin them all" />
          </div>
        </>
      )}
    </Panel>
  )
}
