// Extensions (M7 43b): the servers the model wrote and the operator acked, each loaded as `ext-<name>` in L1 from its
// frozen copy, with its manifest, digest, files, tests, who acked it, its calls and errors, and Revoke; then the
// proposals that are not loaded. A read of `extend.list`, only while it is on screen; Revoke is `extension.revoke`,
// confirmed first, which the core judges as the operator's.
import { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { Link } from 'react-router'
import { Puzzle } from 'lucide-react'
import type { ExtendInfo, ExtendListResult, ExtendLoadedInfo } from '@protocol'
import { call, useRpc } from '@/lib/rpc'
import { ago } from '@/lib/format'
import { stateTone } from '@/lib/taxonomy'
import { Empty, Field, Panel, Pill } from '@/components/ui'
import { networkWords, proposalsNotLoaded, shortDigest } from '@/lib/extensions'

export function ExtensionsCard({ now }: { now: number }) {
  const { data } = useRpc<ExtendListResult>('extend.list', undefined, 5000)
  const loaded = data?.loaded ?? []
  const proposals = data ? proposalsNotLoaded(data) : []
  return (
    <Panel title="Extensions" icon={<Puzzle size={13} />} bodyClassName="px-3.5 py-2.5">
      {!data && <Empty>reading extensions…</Empty>}
      {data && !loaded.length && !proposals.length && <Empty>No extension has been proposed.</Empty>}
      {loaded.map((l) => <Loaded key={l.name} l={l} now={now} manifest={data?.extensions.find((e) => e.name === l.name && e.digest === l.digest)} />)}
      {proposals.length > 0 && (
        <div className="mt-2">
          <div className="panel-title mb-1">proposals</div>
          {proposals.slice(0, 8).map((e) => <Proposal key={`${e.name}.${e.digest}`} e={e} now={now} />)}
        </div>
      )}
      <div className="mt-1.5 flex gap-3 text-[11px]">
        <Link to="/ledger?kind=extend.loaded" className="text-live hover:underline">loads in the ledger →</Link>
        <Link to="/ledger?kind=extend.revoked" className="text-live hover:underline">revokes →</Link>
      </div>
    </Panel>
  )
}

function Loaded({ l, manifest, now }: { l: ExtendLoadedInfo; manifest?: ExtendInfo; now: number }) {
  return (
    <div className="mb-2 border-b border-white/5 pb-2">
      <div className="flex items-center gap-2">
        <span className="font-semibold">{l.name}</span>
        <span className="num text-[11px] text-ink-dim" title={l.digest}>{shortDigest(l.digest)}</span>
        <Pill tone={stateTone(l.state)}>{l.state}</Pill>
        <span className="ml-auto"><Revoke name={l.name} /></span>
      </div>
      <Field label="server" mono>{l.server} · L1 · {networkWords(l.network)}</Field>
      <Field label="tools" mono>{l.tools.map((t) => t.split('/').pop()).join(', ')}</Field>
      <Field label="acked" mono>{l.acked_by} via {l.acked_via} · {ago(l.acked_at_ms, now)}</Field>
      {manifest && <Field label="manifest" mono>{manifest.passed} of {manifest.tests} tests passed · {manifest.description}</Field>}
      <Field label="command" mono>{l.command.join(' ')}</Field>
      {manifest && <Field label="files" mono>{manifest.files ?? 0} files · {(manifest.bytes ?? 0).toLocaleString()} bytes, frozen from {manifest.source}</Field>}
      <Field label="frozen" mono>{l.frozen}</Field>
      <Field label="calls · errors" mono><span className={l.errors ? 'text-wait' : ''}>{l.calls} · {l.errors}</span></Field>
      {l.replaced && <Field label="replaced" mono>{shortDigest(l.replaced)}</Field>}
      {l.last_error && <Field label="last error" mono><span className="text-fault">{l.last_error}</span></Field>}
    </div>
  )
}

function Proposal({ e, now }: { e: ExtendInfo; now: number }) {
  return (
    <Field label={`${e.name} ${shortDigest(e.digest)}`} mono>
      <Pill tone={stateTone(e.state)}>{e.state}</Pill>{' '}
      <span className="text-[11px] text-ink-dim">
        {e.passed} of {e.tests} tests · {networkWords(e.network)} · {ago(e.proposed_at_ms, now)}
        {e.state === 'proposed' && e.question && ` · theseus confirm ${e.question}`}
      </span>
    </Field>
  )
}

/** extension.revoke: its server stops and its tools are gone from the next turn. Confirmed first. */
function Revoke({ name }: { name: string }) {
  const qc = useQueryClient()
  const [busy, setBusy] = useState(false)
  const revoke = async () => {
    if (!window.confirm(`Revoke the extension ${name}? Its server stops, and its tools are gone from the next turn. The frozen copy stays.`)) return
    setBusy(true)
    try { await call('extension.revoke', { name }); await qc.invalidateQueries() } catch (e: any) { window.alert(e?.message ?? String(e)) } finally { setBusy(false) }
  }
  return (
    <button onClick={revoke} disabled={busy} className="rounded px-1.5 py-0.5 text-[10.5px] text-fault ring-1 ring-fault/30 hover:bg-fault/10 disabled:opacity-50">
      {busy ? '…' : 'Revoke'}
    </button>
  )
}
