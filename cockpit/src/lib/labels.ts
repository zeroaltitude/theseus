// A session's content labels (theseus-9ymz0): every item that enters a session is classified, and what is found stays on
// the session as a label until the owner removes it. `injection` freezes the session's writes, programs and sends;
// `secret`, `confidential` and `personal` keep it off China-origin models. The daemon sends the labels on `session.list`
// (`SessionInfo.labels`) and pushes `session.labelled` and `session.unlabelled`; the owner removes one with
// `content.unlabel`, which the daemon judges as it judges `policy.trust`. Pure: `npm test` runs it
// (test/labels.test.ts); the deck's chips are drawn in `views/SessionDeck.tsx`.
import type { ContentUnlabelParams, LabelView } from '@protocol'
import type { Tone } from '@/lib/taxonomy'

/** One chip: the label in words, the node it came from, and the tooltip that says what raised it. */
export interface LabelChip {
  flag: string
  /** `⚑ injection · node nod_e1`, or just `⚑ injection` for a label with no node. */
  text: string
  tone: Tone
  /** The kind of item, the tool and resource it named, how sure the classifier was, and the band it crossed. */
  title: string
  node?: string
}

/** What each flag does to the session, in the daemon's terms. An unknown flag (a newer daemon's) gets no sentence. */
export const FLAG_EFFECT: Readonly<Record<string, string>> = {
  injection: 'freezes the session’s writes, programs and sends until it is removed',
  secret: 'keeps the session off China-origin models',
  confidential: 'keeps the session off China-origin models',
  personal: 'keeps the session off China-origin models',
}

/** Injection is the one that stops work, so it takes the warning tone; the other three are the dimmer one. */
export const toneOf = (flag: string): Tone => (flag === 'injection' ? 'wait' : 'idle')

/** The chips for a session's labels, in the order the daemon sent them (the flags' order). A session with none gets none;
 *  a daemon that sends no `labels` at all is read as none. */
export function chipsOf(labels: readonly LabelView[] | undefined): LabelChip[] {
  return (labels ?? []).map((l) => {
    const effect = FLAG_EFFECT[l.flag]
    return {
      flag: l.flag,
      text: `⚑ ${l.flag}${l.node ? ` · node ${l.node}` : ''}`,
      tone: toneOf(l.flag),
      title: `${l.kind} · ${l.source} · ${Math.round(l.confidence * 100)}% · ${l.band}${effect ? `. It ${effect}.` : ''}`,
      ...(l.node ? { node: l.node } : {}),
    }
  })
}

/** What Remove asks first: what the removal lets the session do again, and that the ledger keeps the label. */
export const askOf = (flag: string, session: string): string => {
  const effect = FLAG_EFFECT[flag]
  return `Remove the ${flag} label from ${session}?${effect ? ` It ${effect}; removing it lifts that.` : ''} The label and its removal stay in the ledger.`
}

/** The call Remove makes: `content.unlabel` with the session and the flag, and the question to ask before it. */
export const unlabelCall = (session: string, flag: string): { method: 'content.unlabel'; params: ContentUnlabelParams; ask: string } =>
  ({ method: 'content.unlabel', params: { session_id: session, flag }, ask: askOf(flag, session) })
