// The ladder (M5 26a; design §2.7, §2.13): each pack's mode, share and why, its rollback rules, and its last
// `pack.mode` rows, from `pack.list`, with promote and roll-back buttons. Each press is confirmed first, and is
// `pack.promote` or `pack.rollback`, judged by the core as any surface's (the owner, from a private place). A
// security pack's promotion is a card: the answer goes through the cockpit's questions, as every question's does.
import { useState } from 'react'
import type { PackListResult, PackModeRow, PackPromoteResult } from '@protocol'
import { call, useRpc } from '@/lib/rpc'
import { modeWords, rowWords, share } from '@/lib/packs'
import { cn } from '@/lib/format'
import { Empty, Pill } from '@/components/ui'

export function PackLadder({ readOnly }: { readOnly: boolean }) {
  const { data, error, refetch } = useRpc<PackListResult>('pack.list', undefined, 5000)
  const [busy, setBusy] = useState(false)
  const [said, setSaid] = useState<string | null>(null)
  const act = async (method: string, params: Record<string, unknown>, ask: string) => {
    if (!window.confirm(ask)) return
    setBusy(true)
    try {
      const r = await call<PackPromoteResult | PackModeRow>(method, params)
      setSaid('said' in r ? r.said : rowWords(r))
      void refetch()
    } catch (x: any) { window.alert(x?.message ?? String(x)) } finally { setBusy(false) }
  }
  const promote = (pack: string) => {
    const typed = window.prompt(`Promote ${pack}: a canary share (0 to 1), or "live".`, '0.2')
    if (!typed) return
    const live = typed.trim() === 'live'
    const s = live ? null : share(typed.trim())
    if (!live && s == null) { window.alert('a share is above 0 and at most 1, or "live"'); return }
    const to = live ? 'live' : 'canary'
    act('pack.promote', { pack, to, share: s }, `Promote ${pack} to ${modeWords(to, s)}? Short of the bar, it is written as forced, with the numbers.`)
  }
  if (error) return <Empty>{String((error as { message?: string }).message ?? error)}</Empty>
  if (!data) return <Empty>reading…</Empty>
  return (
    <div className="text-[12px]">
      <div className="num border-b border-line px-3 py-1.5 text-[11px] text-ink-faint">
        judge {data.enabled ? 'on' : 'off'} · max_mode {data.max_mode}{said && <span className="ml-2 text-live">{said}</span>}
      </div>
      {data.packs.map((p) => (
        <div key={p.pack} className="border-b border-line/60 px-3 py-1.5">
          <div className="flex items-baseline gap-2">
            <span className="num text-ink">{p.pack}</span>
            <Pill tone={p.mode === 'rolled_back' ? 'fault' : p.acts === 'live' || p.acts === 'canary' ? 'live' : p.acts === 'off' ? 'idle' : 'think'}>{modeWords(p.mode, p.share)}</Pill>
            {p.acts !== p.mode && p.mode !== 'rolled_back' && <span className="text-[11px] text-wait">acts as {p.acts} under the config</span>}
            <span className="min-w-0 flex-1 truncate text-[11px] text-ink-faint" title={p.why}>{p.why}</span>
            {!readOnly && (
              <>
                <button disabled={busy} onClick={() => promote(p.pack)} className="rounded px-1.5 text-[10.5px] text-ink-faint ring-1 ring-line hover:text-live disabled:opacity-50">promote</button>
                <button disabled={busy || p.mode === 'rolled_back'} onClick={() => act('pack.rollback', { pack: p.pack }, `Roll ${p.pack} back? It records in shadow and acts on nothing until a promotion.`)}
                  className="rounded px-1.5 text-[10.5px] text-ink-faint ring-1 ring-line hover:text-fault disabled:opacity-50">roll back</button>
              </>
            )}
          </div>
          {p.rules.length > 0 && <div className="num text-[10.5px] text-ink-faint">rules: {p.rules.join(', ')}</div>}
          {p.rows.map((r) => (
            <div key={r.position} className={cn('num text-[10.5px]', r.declined ? 'text-ink-faint' : 'text-ink')}>{rowWords(r)}</div>
          ))}
        </div>
      ))}
    </div>
  )
}
