// A pack's versions (M5 25f; design §2.17): each lineage with a version the learning loop wrote, every version's
// mode and source (compiled in, or learned from your labels), the diff between any two, and promote and reject. A
// promotion is `pack.promote` (a learned version may also go to `shadow`, in its root's place); a reject is
// `pack.rollback { off: true }`, a `pack.mode` row to `off`, your act, judged by the core as any surface's. Each press
// is confirmed first. The pair compared is in the address (`va`, `vb`), so the view deep-links.
import { useState } from 'react'
import { useSearchParams } from 'react-router'
import type { PackInfo, PackListResult, PackModeRow, PackPromoteResult } from '@protocol'
import { call, useRpc } from '@/lib/rpc'
import { modeWords, rowWords, share } from '@/lib/packs'
import { changed, lineDiff, lineages } from '@/lib/versions'
import { cn } from '@/lib/format'
import { Empty, Pill } from '@/components/ui'

export function PackVersions({ readOnly }: { readOnly: boolean }) {
  const { data, error, refetch } = useRpc<PackListResult>('pack.list', undefined, 5000)
  const [params, setParams] = useSearchParams()
  const [busy, setBusy] = useState(false)
  const [said, setSaid] = useState<string | null>(null)
  if (error) return <Empty>{String((error as { message?: string }).message ?? error)}</Empty>
  if (!data) return <Empty>reading…</Empty>
  const ls = lineages(data.packs)
  if (ls.length === 0) return <Empty>no learned version yet: the learning loop writes one from your labels</Empty>
  const act = async (method: string, p: Record<string, unknown>, ask: string) => {
    if (!window.confirm(ask)) return
    setBusy(true)
    try {
      const r = await call<PackPromoteResult | PackModeRow>(method, p)
      setSaid('said' in r ? r.said : rowWords(r))
      void refetch()
    } catch (x: any) { window.alert(x?.message ?? String(x)) } finally { setBusy(false) }
  }
  const promote = (pack: string) => {
    const typed = window.prompt(`Promote ${pack}: "shadow" (in its root's place), a canary share (0 to 1), or "live".`, 'shadow')
    if (!typed) return
    const t = typed.trim()
    const to = t === 'live' || t === 'shadow' ? t : 'canary'
    const s = to === 'canary' ? share(t) : null
    if (to === 'canary' && s == null) { window.alert('"shadow", "live", or a share above 0 and at most 1'); return }
    act('pack.promote', { pack, to, share: s }, `Promote ${pack} to ${modeWords(to, s)}?`)
  }
  const pick = (k: 'va' | 'vb', v: string) => {
    const next = new URLSearchParams(params)
    next.set(k, v)
    setParams(next, { replace: true })
  }
  const byName = new Map<string, PackInfo>(data.packs.map((p) => [p.pack, p]))
  const a = byName.get(params.get('va') ?? '')
  const b = byName.get(params.get('vb') ?? '')
  return (
    <div className="text-[12px]">
      {said && <div className="num border-b border-line px-3 py-1.5 text-[11px] text-live">{said}</div>}
      {ls.map((l) => (
        <div key={l.root} className="border-b border-line/60 px-3 py-1.5">
          <div className="text-[11px] text-ink-faint">{l.root}'s lineage</div>
          {l.versions.map((v) => (
            <div key={v.pack} className="flex items-baseline gap-2">
              <span className={cn('num', v.standing ? 'text-ink' : 'text-ink-faint')}>{v.pack}</span>
              <Pill tone={v.acts === 'live' || v.acts === 'canary' ? 'live' : v.acts === 'off' ? 'idle' : 'think'}>{modeWords(v.mode, v.share)}</Pill>
              <span className="text-[10.5px] text-ink-faint">{v.source}{v.parent ? ` from ${v.parent}` : ''}{v.standing ? ' · stands at its point' : ''}</span>
              <span className="min-w-0 flex-1 truncate text-[10.5px] text-ink-faint" title={v.why}>{v.why}</span>
              <button onClick={() => pick('va', v.pack)} className={cn('rounded px-1 text-[10.5px] ring-1 ring-line', a?.pack === v.pack ? 'text-live' : 'text-ink-faint')}>A</button>
              <button onClick={() => pick('vb', v.pack)} className={cn('rounded px-1 text-[10.5px] ring-1 ring-line', b?.pack === v.pack ? 'text-live' : 'text-ink-faint')}>B</button>
              {!readOnly && v.source === 'learned' && (
                <>
                  <button disabled={busy} onClick={() => promote(v.pack)} className="rounded px-1.5 text-[10.5px] text-ink-faint ring-1 ring-line hover:text-live disabled:opacity-50">promote</button>
                  <button disabled={busy || v.mode === 'off'} onClick={() => act('pack.rollback', { pack: v.pack, off: true }, `Reject ${v.pack}? It goes off: it stands nowhere, and its root (or an earlier version) takes its place.`)}
                    className="rounded px-1.5 text-[10.5px] text-ink-faint ring-1 ring-line hover:text-fault disabled:opacity-50">reject</button>
                </>
              )}
            </div>
          ))}
        </div>
      ))}
      {a && b && (
        <div className="px-3 py-1.5">
          <div className="text-[11px] text-ink-faint">{a.pack} → {b.pack}</div>
          <pre className="num overflow-x-auto whitespace-pre-wrap text-[10.5px]">
            {changed(lineDiff(a.text, b.text)).map((d, k) => (
              <div key={k} className={d.op === '+' ? 'text-live' : d.op === '-' ? 'text-fault' : 'text-ink-faint'}>{d.op} {d.line}</div>
            ))}
          </pre>
        </div>
      )}
    </div>
  )
}
