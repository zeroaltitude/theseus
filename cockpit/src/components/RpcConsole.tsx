// The raw protocol, read-only: pick a method, give its params as JSON, and browse the answer. Only methods that
// read are offered; everything that acts goes through the views, with their confirmations.
import { useState } from 'react'
import { Play, Terminal } from 'lucide-react'
import { call } from '@/lib/rpc'
import { ms } from '@/lib/format'
import { JsonView } from './JsonView'
import { Btn, Panel } from './ui'

const READS: Record<string, string> = {
  health: '',
  'session.list': '',
  'session.history': '{"session_id": "ses_…", "n": 50}',
  'execution.list': '',
  'action.list': '{"n": 20}',
  'confirm.list': '',
  'ledger.tail': '{"n": 50, "kind": "provider.call"}',
  'compilation.list': '{"session_id": "ses_…", "n": 5}',
  'node.list': '{"session_id": "ses_…", "n": 20}',
  'task.list': '',
  'wake.list': '',
  'tool.list': '',
  'profile.list': '',
  'catalog.list': '',
}

export function RpcConsole() {
  const [method, setMethod] = useState('health')
  const [params, setParams] = useState('')
  const [out, setOut] = useState<{ ok: boolean; value: unknown; ms: number } | null>(null)
  const [busy, setBusy] = useState(false)
  const run = async () => {
    let p: unknown = undefined
    if (params.trim()) {
      try { p = JSON.parse(params) } catch (e: any) { setOut({ ok: false, value: `params are not JSON: ${e.message}`, ms: 0 }); return }
    }
    setBusy(true)
    const t0 = performance.now()
    try { setOut({ ok: true, value: await call(method, p), ms: performance.now() - t0 }) }
    catch (e: any) { setOut({ ok: false, value: e, ms: performance.now() - t0 }) }
    finally { setBusy(false) }
  }
  return (
    <Panel title="Protocol console · read-only" icon={<Terminal size={13} />} bodyClassName="p-3">
      <div className="flex flex-wrap items-center gap-2">
        <select value={method} onChange={(e) => { setMethod(e.target.value); setParams(READS[e.target.value]) }}
          className="h-8 rounded-md bg-white/[0.04] px-2 font-mono text-[12px] text-ink outline-none ring-1 ring-line">
          {Object.keys(READS).map((m) => <option key={m} value={m}>{m}</option>)}
        </select>
        <input value={params} onChange={(e) => setParams(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') void run() }}
          placeholder="params as JSON (optional)"
          className="h-8 min-w-64 flex-1 rounded-md bg-white/[0.04] px-2.5 font-mono text-[12px] text-ink outline-none ring-1 ring-line placeholder:text-ink-faint focus:ring-live/40" />
        <Btn onClick={() => void run()} busy={busy}><Play size={13} /> Call</Btn>
      </div>
      {out && (
        <div className="mt-3">
          <div className="num mb-1 text-[11px] text-ink-faint">{out.ok ? 'result' : <span className="text-fault">error</span>} · {ms(out.ms)}</div>
          <JsonView value={out.value} maxHeight="420px" />
        </div>
      )}
    </Panel>
  )
}
