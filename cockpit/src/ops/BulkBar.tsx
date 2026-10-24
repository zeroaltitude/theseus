// The bulk bar (theseus-jid7t.4): each verb valid for at least one picked item, with its counts ("Pause 2 · resume 1",
// "Cancel 3"), greyed with the reason when none admits it. A bulk act is a client fan-out of single calls
// (`ops/selection.ts`'s `fanOut`, four at a time, each through `useOpsAct`'s `send`), so each item has its own ledger
// row and its own refusal. Stop, Cancel, Re-home and Decline ask once, with the list; Approve opens the review sheet of
// the owner's decision 4 (each approval with its tool, its own words, its session and its age, a box each, all ticked;
// a floor approval listed as asking one by one, never sent). The results land in one toast, ✓ or refused in the
// daemon's words, per item; the failed items stay picked. Under the time machine the bar is off, with the banner's words.
import { useEffect, useState, type ReactNode } from 'react'
import { ArrowRightLeft, CheckCheck, FolderPlus, ListChecks, OctagonX, X } from 'lucide-react'
import { ago, cn } from '@/lib/format'
import type { OpsRow } from './board'
import type { QueueItem } from './queue'
import { PAST_WORDS } from './keys'
import { useOpsAct } from './act'
import { useSelection } from './useSelection'
import { bulkVerbs, fanOut, type BulkItem, type BulkOffer, type BulkVerb, type RunResult } from './selection'
import { dragOf, dropOf, budgetWords, type DropTarget } from './rehome'

/** A modal sheet: what it will do, then one button that does it. */
export function Sheet({ title, icon, children, go, onGo, onClose, danger, disabled }: {
  title: string; icon?: ReactNode; children: ReactNode; go: string; onGo: () => void; onClose: () => void; danger?: boolean; disabled?: boolean
}) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.stopImmediatePropagation(); onClose() } }
    window.addEventListener('keydown', k, true)
    return () => window.removeEventListener('keydown', k, true)
  }, [onClose])
  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/50 p-4" onClick={onClose}>
      <div role="dialog" aria-label={title} className="flex max-h-[80vh] w-[min(560px,100%)] flex-col overflow-hidden rounded-xl bg-hull ring-1 ring-line-strong shadow-2xl" onClick={(e) => e.stopPropagation()}>
        <header className="flex items-center gap-2 border-b border-line px-4 py-2.5">
          <span className="text-gold">{icon}</span>
          <h2 className="font-display text-[13px] font-bold tracking-[0.04em] text-gold">{title}</h2>
          <button type="button" onClick={onClose} aria-label="close" className="ml-auto text-ink-faint hover:text-ink"><X size={14} /></button>
        </header>
        <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">{children}</div>
        <footer className="flex justify-end gap-2 border-t border-line px-4 py-2.5">
          <button type="button" onClick={onClose} className="rounded-md px-3 py-1 text-[12px] text-ink-dim ring-1 ring-line hover:text-ink">Keep things as they are</button>
          <button type="button" onClick={onGo} disabled={disabled} className={cn('rounded-md px-3 py-1 text-[12px] font-medium ring-1 disabled:cursor-not-allowed disabled:opacity-40', danger ? 'bg-fault/15 text-fault ring-fault/50' : 'bg-live/15 text-live ring-live/50')}>{go}</button>
        </footer>
      </div>
    </div>
  )
}

type Asking =
  | { verb: 'stop' | 'cancel' | 'decline'; items: BulkItem[]; keep: boolean }
  | { verb: 'approve'; items: BulkItem[]; floor: QueueItem[]; ticked: ReadonlySet<string>; keep: boolean }
  | { verb: 'rehome'; items: BulkItem[]; to: DropTarget | null; keep: boolean }
  | { verb: 'attach'; items: BulkItem[]; to: DropTarget | null; keep: boolean }

export interface BulkInput {
  rows: readonly OpsRow[]
  queue: readonly QueueItem[]
  /** Where a re-home may send a report: the conversations and the threads' lanes. */
  homes: readonly DropTarget[]
  projects: readonly DropTarget[]
  titleOf: (sessionId: string) => string
  now: number
  onResults: (r: RunResult[]) => void
}

/** The bulk acts' hand: `offers(ids)` is the bar's table, `start(verb, ids)` runs one (through its sheet when it asks),
 *  and `sheet` is the open sheet, if any. A run over the selection keeps its failed items picked; a right-click's run
 *  over one node leaves the selection alone. */
export function useBulk({ rows, queue, homes, projects, titleOf, now, onResults }: BulkInput) {
  const { send, settle, past } = useOpsAct()
  const keepFailed = useSelection((s) => s.keepFailed)
  const [asking, setAsking] = useState<Asking | null>(null)
  const offers = (ids: Iterable<string>): BulkOffer[] => {
    const set = new Set(ids)
    const os = bulkVerbs(rows.filter((r) => set.has(r.id)), queue)
    return past ? os.map((o) => ({ ...o, items: [], reason: PAST_WORDS })) : os
  }

  const run = async (verb: BulkVerb, items: BulkItem[], keep: boolean, refused: RunResult[] = []) => {
    const done = await fanOut(items.filter((i) => i.method), (i) => send(`bulk_${verb}`, i.method!, i.params), 4)
    const results = [...done, ...refused]
    void settle()
    onResults(results)
    if (keep) keepFailed(results)
  }
  const start = (verb: BulkVerb, ids: Iterable<string>, keep = true) => {
    const o = offers(ids).find((x) => x.verb === verb)
    if (!o || !o.items.length) return
    if (verb === 'pause') void run(verb, o.items, keep)
    else if (verb === 'approve') setAsking({ verb, items: o.items, floor: o.oneByOne ?? [], ticked: new Set(o.items.map((i) => i.key)), keep })
    else if (verb === 'rehome' || verb === 'attach') setAsking({ verb, items: o.items, to: null, keep })
    else setAsking({ verb, items: o.items, keep })
  }

  const close = () => setAsking(null)
  let sheet: ReactNode = null
  if (asking?.verb === 'stop' || asking?.verb === 'cancel' || asking?.verb === 'decline') {
    const a = asking
    const what = { stop: 'Stop the turn of', cancel: 'Cancel', decline: 'Decline' }[a.verb]
    sheet = (
      <Sheet title={`${what} ${a.items.length}?`} icon={<OctagonX size={14} />} danger go={`${what} ${a.items.length}`} onClose={close} onGo={() => { close(); void run(a.verb, a.items, a.keep) }}>
        <p className="mb-2 text-[12px] text-ink-dim">{a.verb === 'decline' ? 'Each is answered No, one call each.' : a.verb === 'stop' ? 'Each running turn stops, one call each.' : 'Each stops and reports that it was cancelled, one call each.'}</p>
        <ul className="space-y-1">{a.items.map((i) => <li key={i.key} className="truncate text-[12.5px] text-ink">· {i.label}</li>)}</ul>
      </Sheet>
    )
  } else if (asking?.verb === 'approve') {
    const a = asking
    const n = a.items.filter((i) => a.ticked.has(i.key)).length
    const tick = (k: string) => setAsking({ ...a, ticked: a.ticked.has(k) ? new Set([...a.ticked].filter((x) => x !== k)) : new Set([...a.ticked, k]) })
    sheet = (
      <Sheet title={`Review ${a.items.length} approval${a.items.length === 1 ? '' : 's'}`} icon={<ListChecks size={14} />} go={`Approve ${n}`} disabled={n === 0} onClose={close}
        onGo={() => { close(); void run('approve', a.items.filter((i) => a.ticked.has(i.key)), a.keep) }}>
        <p className="mb-2 text-[12px] text-ink-dim">Each ticked one is answered Yes, one call each. Untick what you want to read first.</p>
        <ul className="space-y-1.5" data-testid="approve-review">
          {a.items.map((i) => {
            const c = i.item!.confirm!
            return (
              <li key={i.key}>
                <label className="flex cursor-pointer items-start gap-2 rounded-md px-2 py-1.5 ring-1 ring-line hover:bg-white/[0.03]">
                  <input type="checkbox" checked={a.ticked.has(i.key)} onChange={() => tick(i.key)} className="mt-0.5 accent-gold" />
                  <span className="min-w-0 flex-1">
                    <span className="num text-[11px] text-live">{c.tool}</span>
                    <span className="block text-[12.5px] text-ink">{c.plain?.what ?? c.reason}</span>
                    <span className="num block text-[10.5px] text-ink-faint">{i.item!.owner} · asked {ago(c.requested_at_ms, now)}{c.plain?.risk ? ` · ${c.plain.risk}` : ''}</span>
                  </span>
                </label>
              </li>
            )
          })}
          {a.floor.map((i) => (
            <li key={i.key} className="flex items-start gap-2 rounded-md px-2 py-1.5 opacity-70 ring-1 ring-wait/30">
              <span className="mt-0.5 text-wait" aria-hidden>◆</span>
              <span className="min-w-0 flex-1">
                <span className="num text-[11px] text-wait">{i.confirm!.tool} · asks one by one</span>
                <span className="block text-[12.5px] text-ink">{i.confirm!.plain?.what ?? i.confirm!.reason}</span>
                <span className="block text-[10.5px] text-ink-faint">The floor asks at every posture: answer it on its own card in the queue. It is not sent.</span>
              </span>
            </li>
          ))}
        </ul>
      </Sheet>
    )
  } else if (asking?.verb === 'rehome' || asking?.verb === 'attach') {
    const a = asking
    const rehome = a.verb === 'rehome'
    const plan = a.to ? a.items.map((i) => ({ i, d: dropOf(dragOf(i.row!, titleOf), a.to!, past) })) : []
    const ok = plan.flatMap(({ i, d }) => ('refused' in d ? [] : [{ ...i, method: d.method, params: d.params }]))
    const refused = plan.flatMap(({ i, d }) => ('refused' in d ? [{ id: i.id, key: i.key, label: i.label, ok: false, words: d.refused }] : []))
    sheet = (
      <Sheet title={rehome ? `Re-home ${a.items.length}` : `Attach ${a.items.length} to a project`} icon={rehome ? <ArrowRightLeft size={14} /> : <FolderPlus size={14} />} danger={rehome}
        go={a.to ? `${rehome ? 'Re-home' : 'Attach'} ${ok.length} to ${a.to.title}` : 'Pick where'} disabled={!ok.length} onClose={close}
        onGo={() => { close(); void run(a.verb, ok, a.keep, refused) }}>
        <div className="mb-3 flex flex-wrap gap-1.5">
          {(rehome ? homes : projects).map((t) => (
            <button key={t.kind === 'project' ? t.id : t.kind === 'lane' ? t.slug : t.sessionId} type="button" onClick={() => setAsking({ ...a, to: t })}
              className={cn('rounded-full px-2.5 py-0.5 text-[11.5px] ring-1', a.to === t ? 'bg-gold/15 text-gold ring-gold/60' : 'text-ink-dim ring-line hover:text-ink')}>
              {t.kind === 'lane' ? `thread · ${t.title}` : t.title}
            </button>
          ))}
          {!(rehome ? homes : projects).length && <span className="text-[12px] text-ink-faint">{rehome ? 'no conversation or thread to send a report to' : 'no project yet: make one on the Timeline'}</span>}
        </div>
        <ul className="space-y-1">
          {(plan.length ? plan : a.items.map((i) => ({ i, d: null }))).map(({ i, d }) => (
            <li key={i.key} className="text-[12.5px]">
              <span className="text-ink">· {i.label}</span>
              {d && ('refused' in d ? <span className="block pl-3 text-[11px] text-fault">⊘ {d.refused}</span>
                : rehome && <span className="block pl-3 text-[11px] text-ink-faint">{budgetWords(dragOf(i.row!, titleOf).homeTitle)}</span>)}
            </li>
          ))}
        </ul>
      </Sheet>
    )
  }
  return { offers, start, sheet }
}

const ICON: Readonly<Partial<Record<BulkVerb, ReactNode>>> = { approve: <CheckCheck size={11} />, rehome: <ArrowRightLeft size={11} />, attach: <FolderPlus size={11} />, cancel: <OctagonX size={11} /> }

/** The bar over the canvas while something is picked. */
export function BulkBar({ count, offers, onVerb, onClear }: { count: number; offers: BulkOffer[]; onVerb: (v: BulkVerb) => void; onClear: () => void }) {
  const { on, past } = useOpsAct()
  return (
    <div role="toolbar" aria-label="bulk acts" data-testid="ops-bulk" className="flex flex-wrap items-center gap-1.5 rounded-xl bg-hull/90 px-3 py-1.5 ring-1 ring-gold/40 shadow-[0_0_18px_-8px_rgb(214_165_72_/_0.6)]">
      <span className="font-display text-[11.5px] font-bold uppercase tracking-[0.08em] text-gold">{count} picked</span>
      {past && <span className="text-[11.5px] text-wait">{PAST_WORDS}</span>}
      {offers.map((o) => {
        const off = !on(`bulk_${o.verb}`) || !o.items.length
        return (
          <button key={o.verb} type="button" data-verb={`bulk_${o.verb}`} disabled={off} onClick={() => onVerb(o.verb)} title={!on(`bulk_${o.verb}`) ? PAST_WORDS : o.reason ?? (o.asks ? 'asks once, with the list' : undefined)}
            className={cn('inline-flex items-center gap-1 rounded-md px-2 py-0.5 text-[11.5px] ring-1 disabled:cursor-not-allowed disabled:opacity-35',
              o.verb === 'cancel' || o.verb === 'stop' || o.verb === 'decline' ? 'text-fault ring-fault/40' : o.verb === 'approve' ? 'text-ok ring-ok/40' : 'text-live ring-live/35')}>
            {ICON[o.verb]}{o.label}
          </button>
        )
      })}
      <button type="button" onClick={onClear} className="ml-auto flex items-center gap-1 text-[11px] text-ink-faint hover:text-ink" title="put them all back (esc)"><span className="kbd">esc</span> clear</button>
    </div>
  )
}

/** One toast for a run: ✓ or refused in the daemon's words, per item; the refused stay picked. */
export function ResultsToast({ results, onClose }: { results: readonly RunResult[]; onClose: () => void }) {
  const bad = results.filter((r) => !r.ok).length
  useEffect(() => { if (bad) return; const t = window.setTimeout(onClose, 6000); return () => window.clearTimeout(t) }, [bad, onClose])
  return (
    <div role="status" data-testid="ops-results" className="fixed bottom-24 right-6 z-30 w-[min(420px,92vw)] rounded-xl bg-hull/95 p-3 text-[12px] shadow-2xl ring-1 ring-line-strong backdrop-blur">
      <div className="mb-1.5 flex items-center gap-2">
        <span className="font-display text-[11.5px] font-bold uppercase tracking-[0.08em] text-gold">{results.length - bad} done{bad ? ` · ${bad} refused` : ''}</span>
        {bad > 0 && <span className="text-[11px] text-ink-faint">the refused stay picked: retry targets exactly them</span>}
        <button type="button" onClick={onClose} aria-label="close" className="ml-auto text-ink-faint hover:text-ink"><X size={13} /></button>
      </div>
      <ul className="max-h-48 space-y-0.5 overflow-y-auto">
        {results.map((r) => (
          <li key={r.key} className="flex gap-1.5">
            <span className={r.ok ? 'text-ok' : 'text-fault'} aria-hidden>{r.ok ? '✓' : '✗'}</span>
            <span className="min-w-0 flex-1"><span className="text-ink">{r.label}</span>{(!r.ok || r.words !== 'done') && <span className={cn('block text-[11px]', r.ok ? 'text-ink-dim' : 'text-fault')}>{r.words}</span>}</span>
          </li>
        ))}
      </ul>
    </div>
  )
}
