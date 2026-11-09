// Taking one content label off one session (theseus-emqbm), at `/session/:id/unlabel/:flag`: the address every reply's
// footer links, one link per label the session carries, so the owner can take a label off at any time and not only from
// the one Discord message that raised it.
//
// **It asks once and does not act on load.** The owner asked for the link and for "immediately", and the link is what the
// footer carries: no button there. The page itself takes one click, because the removal is a one-way door — the classifier
// will not raise that flag again for anything that entered the session before the removal, and removing `injection`
// unfreezes a session — and a page that acted on load would act on a browser's prefetch, a reopened tab or a restored
// history entry. The reasoning is in `lib/labels.ts`'s `unlabelRoute`, which is this page's whole decision.
//
// It adds no route to the daemon: `theseusd`'s `web.rs` already falls back to the app shell for the cockpit's own
// client-side routes, and the removal is `content.unlabel` from this connection, which the core judges as it judges every
// other surface's (the owner, from a private place).
import { useState } from 'react'
import { useParams } from 'react-router'
import { Flag } from 'lucide-react'
import { Panel, Btn, Field } from '@/components/ui'
import { call } from '@/lib/rpc'
import { removedWords, unlabelFailureWords, unlabelRoute } from '@/lib/labels'
import type { UnlabelResult } from '@protocol'

export default function Unlabel() {
  const { id, flag } = useParams()
  const route = unlabelRoute(id, flag)
  const [busy, setBusy] = useState(false)
  const [said, setSaid] = useState<{ tone: 'ok' | 'no'; words: string } | null>(null)

  if (!route.ok) {
    return (
      <Panel help={false} title="That label address" icon={<Flag size={13} className="text-fault" />} bodyClassName="p-4">
        <div className="text-[13px] text-fault">{route.why}</div>
      </Panel>
    )
  }

  const remove = () => {
    setBusy(true)
    void call<UnlabelResult>(route.call.method, route.call.params)
      .then((r) => setSaid({ tone: 'ok', words: removedWords(r.flag, r.by) }))
      .catch((e: unknown) => setSaid({ tone: 'no', words: unlabelFailureWords(route.flag, route.session, e instanceof Error ? e.message : String(e)) }))
      .finally(() => setBusy(false))
  }

  return (
    <Panel help="session.unlabel" title={`Remove the ${route.flag} label`} icon={<Flag size={13} />} bodyClassName="p-4">
      <Field label="Session" mono>{route.session}</Field>
      <Field label="Label" mono>{route.flag}</Field>
      {route.effect && <div className="mt-3 text-[13px] text-ink-dim">It {route.effect}; removing it lifts that.</div>}
      <div className="mt-2 text-[13px] text-ink-dim">The label and its removal stay in the ledger. Nothing is removed until you press Remove.</div>
      {said ? (
        <div className={`mt-3 text-[13px] ${said.tone === 'ok' ? 'text-ink' : 'text-fault'}`}>{said.words}</div>
      ) : (
        <div className="mt-3 flex gap-2">
          <Btn busy={busy} onClick={remove}>Remove</Btn>
        </div>
      )}
    </Panel>
  )
}
