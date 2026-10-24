// The Notices strip (theseus-g00vv): the sessions that hold external text without anything waiting on it, at the top of
// the queue's list and never counted in NEEDS YOU. Each notice opens to what was read, from where, which session, and what
// Trust again does. Trust again (policy.trust) asks first; Dismiss (policy.notice_seen) only marks the notice seen. The
// strip-level "Dismiss all from before notify" is the owner's click, one call per session after one question: nothing on
// load, nothing automatic. A dismissed notice leaves this strip and stays on the Actions page's holds and in the session deck.
import { useState } from 'react'
import { Link } from 'react-router'
import { BellRing, ChevronRight } from 'lucide-react'
import { ago, short } from '@/lib/format'
import { useOpsAct } from './act'
import { ActBtn } from './ActBtn'
import { NOTICE_LOOK } from './facts'
import { dismissAll, noticeFacts, seenCall, trustCall, type Hold } from './notices'

export function Notices({ notices, now, readOnly = false, redact = false }: { notices: readonly Hold[]; now: number; readOnly?: boolean; redact?: boolean }) {
  const { busy, act, send, settle, on } = useOpsAct()
  const [open, setOpen] = useState<string | null>(null)
  if (!notices.length) return null
  const all = dismissAll(notices)
  const dismissEvery = async () => {
    if (!window.confirm(all.ask)) return
    for (const c of all.calls) await send('mark', c.method, c.params)
    settle()
  }
  return (
    <section className="shrink-0 rounded-lg bg-white/[0.02] ring-1 ring-line" aria-label="Notices" data-testid="ops-notices">
      <h3 className="flex items-center gap-1.5 px-2.5 pt-2 font-display text-[10.5px] font-bold uppercase tracking-[0.08em] text-ink-dim" title="sessions that read external text; under notify nothing waits on it, so these are not counted as needing you">
        <BellRing size={11} /> Notices · {notices.length}
        {!readOnly && <span className="ml-auto"><ActBtn verb="mark" on={on} tone="idle" busy={busy === 'notice:all'} onClick={() => void dismissEvery()} title="mark every notice seen: they leave this strip, the holds stay">{all.label}</ActBtn></span>}
      </h3>
      <div className="flex flex-col p-1">
        {notices.map((h) => {
          const f = noticeFacts(h, redact)
          const isOpen = open === h.session_id
          return (
            <div key={h.session_id} data-notice={h.session_id} className="rounded px-1.5 py-1">
              <button type="button" onClick={() => setOpen(isOpen ? null : h.session_id)} aria-expanded={isOpen}
                className="flex w-full items-center gap-1.5 text-left text-[11.5px] text-ink-dim hover:text-ink">
                <ChevronRight size={11} className={isOpen ? 'rotate-90' : ''} aria-hidden />
                <span aria-hidden title={NOTICE_LOOK.word}>{NOTICE_LOOK.glyph}</span>
                <span className="min-w-0 flex-1 truncate">{f.title} read {f.tool} {f.what}</span>
                <span className="num shrink-0 text-[10px] text-ink-faint">{ago(f.since, now)}</span>
              </button>
              {isOpen && (
                <div className="mt-1 flex flex-col gap-1 pl-4 text-[11px] text-ink-dim">
                  <div>Read: {f.tool} {f.what} · node <Link to={`/session/${f.sessionId}#${f.nodeId}`} className="num text-live hover:underline">{short(f.nodeId)}</Link></div>
                  <div>From: {f.how}{f.fromSession ? <> · <Link to={`/session/${f.fromSession}`} className="num text-live hover:underline">{short(f.fromSession)}</Link></> : null}</div>
                  <div>Session: {f.title} · <Link to={`/session/${f.sessionId}`} className="num text-live hover:underline">{short(f.sessionId)}</Link></div>
                  <div className="text-ink-faint">Trust again: {f.trusts}.</div>
                  {!readOnly && (
                    <div className="flex gap-1.5">
                      <ActBtn verb="trust" on={on} tone="wait" busy={busy === `notice:${h.session_id}:trust`} onClick={() => { const c = trustCall(h); void act('trust', `notice:${h.session_id}:trust`, c.method, c.params, c.ask) }}>Trust again</ActBtn>
                      <ActBtn verb="mark" on={on} tone="idle" busy={busy === `notice:${h.session_id}:seen`} onClick={() => { const c = seenCall(h); void act('mark', `notice:${h.session_id}:seen`, c.method, c.params) }}>Dismiss</ActBtn>
                    </div>
                  )}
                </div>
              )}
            </div>
          )
        })}
      </div>
    </section>
  )
}
