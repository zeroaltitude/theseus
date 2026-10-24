// A selected calendar event, in the inspector (theseus-jid7t.5): its facts (title, time, calendar, attendee count, the
// owner's response, whether the owner organizes it, the call link, the place), the privacy rule of a sensitive
// calendar, and its verbs. Every string drawn here is `eventFacts`' (so redact mode masks it), and every verb's payload
// is `eventVerbPayload`'s (so a sensitive event's content never leaves the page through a verb):
//   Join the call, Open in Google   a link opened in a new tab; nothing is sent to the daemon
//   Remind me                       one `wake.create`, its note naming only the calendar and the time when sensitive
//   Start a prep task               the New task sheet, filled in; a sensitive event's sheet offers US models only
//   Ask Theseus                     a private chat (`session.open`, `session.profile` holding the pick, then its first
//                                   message), on a US model when sensitive
// Each goes through `useOpsAct`, so the time machine turns it off.
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { Bell, ExternalLink, Lock, MessageCircle, NotebookPen, Video } from 'lucide-react'
import type { CalendarEvent, CalendarWindow, ProfileListResult, SessionProfileResult } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { toneHex } from '@/lib/taxonomy'
import { askCalls, eventFacts, eventVerbPayload, PRIVACY_NOTE, profileRefusal, redactEvent, US_ONLY_WORDS, type EventVerb } from './calendar'
import { profileChoices } from './newtask'
import { NewTask } from './NewTask'
import { useOpsAct, PAST_WORDS } from './act'
import { ActBtn } from './ActBtn'

const LINK = 'inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-[11.5px] font-medium hover:brightness-125'

export function EventDetail({ ev, cal, redact }: { ev: CalendarEvent; cal: CalendarWindow; redact: boolean }) {
  const nav = useNavigate()
  const { send, settle, on, past } = useOpsAct()
  const { data: pl } = useRpc<ProfileListResult>('profile.list', undefined, 0)
  const [said, setSaid] = useState<{ ok: boolean; text: string } | null>(null)
  const [prep, setPrep] = useState(false)
  const f = eventFacts(ev, cal, redact)
  // The links come from the event as the page shows it: under redact a sensitive event has no call
  // link and no Google link to open.
  const shown = redactEvent(ev, cal, redact)
  const pay = (v: EventVerb) => eventVerbPayload(v === 'join' || v === 'google' ? shown : ev, cal, v)
  const join = pay('join')
  const google = pay('google')
  const prepSheet = pay('prep')
  const remind = async () => {
    const p = pay('remind')
    if (p.kind !== 'call') { setSaid({ ok: false, text: p.kind === 'none' ? p.why : 'nothing to do' }); return }
    const r = await send('event_remind', p.method, p.params)
    void settle()
    setSaid(r.ok ? { ok: true, text: `a reminder is set for ${new Date(p.params.when).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}` } : { ok: false, text: r.words })
  }
  const ask = async () => {
    const p = pay('ask')
    if (p.kind !== 'ask') return
    const { pick } = profileChoices(pl?.profiles ?? [], undefined, p.usOnly ? US_ONLY_WORDS : undefined, pl?.live)
    if (p.usOnly && !pick) { setSaid({ ok: false, text: `${US_ONLY_WORDS}, and no profile of US origin is known` }); return }
    const calls = askCalls('', p, pick)
    const o = await send<{ session_id: string }>('event_ask', 'session.open', calls[0].params)
    if (!o.ok) { setSaid({ ok: false, text: o.words }); return }
    const sid = o.result.session_id
    // The chosen profile is held on the session before the first turn, so every later turn keeps it; a refusal sends
    // no turn (the chat exists, and the deck opens), and no other profile is tried.
    let refused: string | undefined
    for (const c of askCalls(sid, p, pick).slice(1)) {
      if (refused) break
      if (c.method === 'session.profile') {
        const r = await send<SessionProfileResult>('event_ask', c.method, c.params)
        if (!r.ok) refused = r.words
        else refused = profileRefusal(r.result)
      } else void send('event_ask', c.method, c.params) // the first message: the deck follows the turn live
    }
    if (refused) setSaid({ ok: false, text: refused })
    nav(`/session/${sid}`)
  }
  return (
    <div className="flex flex-col gap-2 p-2.5" data-testid="event-detail">
      <div className="flex items-center gap-1.5 text-[10.5px]">
        <span className="inline-block h-2.5 w-4 rounded-[2px]" style={{ background: cal.color }} aria-hidden />
        <span className="text-ink-dim">{cal.label}</span>
        {cal.sensitive && <span className="flex items-center gap-1 rounded px-1 text-wait ring-1 ring-wait/40"><Lock size={9} /> sensitive</span>}
        {f.block === 'busy' && <span className="text-ink-faint">· free/busy only</span>}
      </div>
      <div className="text-[14px] font-semibold text-ink">{f.block}</div>
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-[11.5px]">
        {f.facts.map(([k, v]) => <div key={k} className="contents"><dt className="text-ink-faint">{k}</dt><dd className="num break-all text-ink-dim">{v}</dd></div>)}
      </dl>
      {cal.sensitive && <p className="rounded-md bg-black/25 px-2 py-1.5 text-[11px] leading-snug text-ink-dim ring-1 ring-wait/30"><Lock size={10} className="mr-1 inline text-wait" />{PRIVACY_NOTE}</p>}
      <div className="flex flex-wrap gap-1.5">
        {join.kind === 'link' && <a href={join.href} target="_blank" rel="noopener noreferrer" className={LINK} style={{ color: toneHex.live, background: `${toneHex.live}14`, boxShadow: `inset 0 0 0 1px ${toneHex.live}40` }}><Video size={11} /> Join the call</a>}
        {google.kind === 'link' && <a href={google.href} target="_blank" rel="noopener noreferrer" className={LINK} style={{ color: toneHex.idle, background: `${toneHex.idle}14`, boxShadow: `inset 0 0 0 1px ${toneHex.idle}40` }}><ExternalLink size={11} /> Open in Google</a>}
        <ActBtn verb="event_remind" on={on} tone="live" onClick={() => void remind()} title="a reminder ten minutes before it starts"><Bell size={11} /> Remind me</ActBtn>
        <span className="inline-flex items-center gap-1">
          <ActBtn verb="event_prep" on={on} tone="ok" onClick={() => setPrep(true)} title="the New task sheet, filled in"><NotebookPen size={11} /> Start a prep task</ActBtn>
          {cal.sensitive && <span className="rounded px-1 text-[9.5px] font-semibold uppercase tracking-wider text-wait ring-1 ring-wait/40" title={US_ONLY_WORDS}>US-only</span>}
        </span>
        <ActBtn verb="event_ask" on={on} tone="think" onClick={() => void ask()} title={cal.sensitive ? `a private chat; ${US_ONLY_WORDS}` : 'a private chat about it'}><MessageCircle size={11} /> Ask Theseus</ActBtn>
      </div>
      {past && <p className="text-[11px] text-wait">{PAST_WORDS}</p>}
      {said && <div role="status" className={said.ok ? 'text-[11.5px] text-ok' : 'text-[11.5px] text-fault'}>{said.text}</div>}
      {prep && prepSheet.kind === 'sheet' && (
        <NewTask brief={prepSheet.brief} title={prepSheet.title} usOnly={prepSheet.usOnly} onClose={() => setPrep(false)}
          onStarted={(id) => { setPrep(false); setSaid({ ok: true, text: `the prep task ${id.slice(-6)} started` }) }} />
      )}
    </div>
  )
}
