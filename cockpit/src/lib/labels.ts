// A session's content labels (theseus-9ymz0): every item that enters a session is classified, and what is found stays on
// the session as a label until the owner removes it. `injection` tells the owner that something tried to steer the
// session, and only `[content_based_security] injection = "refuse"` (not the default since 1.0.6) refuses its writes,
// programs and sends; `secret`, `confidential` and `personal` keep it off China-origin models. The daemon sends the
// labels on `session.list`
// (`SessionInfo.labels`) and pushes `session.labelled` and `session.unlabelled`; the owner removes one with
// `content.unlabel`, which is the re-trust of the session. Pure: `npm test` runs it
// (test/labels.test.ts); the deck's chips are drawn in `views/SessionDeck.tsx`.
import type { ContentUnlabelParams, JudgeLabelParams, LabelView } from '@protocol'
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
  /** The judgment that raised the label, which "wrong" labels; absent for a label raised before store format 53. */
  judgment?: string
}

/** What each flag does to the session, in the daemon's terms. An unknown flag (a newer daemon's) gets no sentence. */
export const FLAG_EFFECT: Readonly<Record<string, string>> = {
  injection:
    'tells the operator that something tried to steer the session; the call still runs, and only ' +
    '`[content_based_security] injection = "refuse"` refuses its writes, programs and sends',
  secret: 'keeps the session off China-origin models',
  confidential: 'keeps the session off China-origin models',
  personal: 'keeps the session off China-origin models',
}

/** What removing each flag gives up — its own sentence, per flag, because what a label does and what its removal gives up
 *  are not the same shape for every flag. `secret`'s restraint is applied in every configuration, so it is simply lifted;
 *  an `injection` refuses nothing at the shipped default (`[content_based_security] injection = "notify"`), so there is
 *  nothing there to lift and saying "removing it lifts that" would promise the operator a restraint he never had. A flag
 *  with an effect but no entry here says only the effect. */
export const FLAG_REMOVAL: Readonly<Record<string, string>> = {
  injection: 'Removing it takes the warning off the session, and gives up that refusal where `refuse` is set.',
  secret: 'Removing it lifts that.',
  confidential: 'Removing it lifts that.',
  personal: 'Removing it lifts that.',
}

/** The sentences Remove says about a label: what it does, then what removing it gives up. Empty for a flag the table
 *  lacks. `askOf` and the removal page both read this one composition, so the question the operator is asked and the line
 *  on the page he acts from cannot drift apart — they did, while the page wrote the template out a second time. */
export const effectWords = (flag: string): string => {
  const effect = FLAG_EFFECT[flag]
  if (!effect) return ''
  const removal = FLAG_REMOVAL[flag]
  return `It ${effect}.${removal ? ` ${removal}` : ''}`
}

/** Injection is the one that asks the operator to look, so it takes the warning tone; the other three are the dimmer one. */
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
      ...(l.judgment ? { judgment: l.judgment } : {}),
    }
  })
}

/** What Remove asks first: what the label does, what the removal gives up, and that the ledger keeps the label. */
export const askOf = (flag: string, session: string): string => {
  const words = effectWords(flag)
  return `Remove the ${flag} label from ${session}?${words ? ` ${words}` : ''} The label and its removal stay in the ledger.`
}

/** The call Remove makes: `content.unlabel` with the session and the flag, and the question to ask before it. */
export const unlabelCall = (session: string, flag: string): { method: 'content.unlabel'; params: ContentUnlabelParams; ask: string } =>
  ({ method: 'content.unlabel', params: { session_id: session, flag }, ask: askOf(flag, session) })

/** What "wrong" asks first: the label is miscategorised, and it stays. */
export const wrongAskOf = (flag: string, session: string): string =>
  `Mark the ${flag} label on ${session} as miscategorised? The item should not have been flagged ${flag}. The label stays on ${session}, and the classifier is taught.`

/** The call "wrong" makes: a `judge.label` row saying the raising judgment answered the flag's question `yes` when it should
 *  have been `no`, the row a Remove writes too, without removing the label. A label with no judgment (raised before store
 *  format 53) has nothing to teach, so it has no call. */
export const wrongCall = (session: string, l: Pick<LabelView, 'flag' | 'judgment'>): { method: 'judge.label'; params: JudgeLabelParams; ask: string } | null =>
  l.judgment
    ? {
        method: 'judge.label',
        params: { judgment: l.judgment, question: l.flag, label: 'no', note: 'marked wrong by the owner via the cockpit; the label stays' },
        ask: wrongAskOf(l.flag, session),
      }
    : null

// ---------------------------------------------------------------- the footer's removal link (theseus-emqbm)

/** The four flags the daemon knows. The page refuses anything else rather than sending it. */
export const FLAGS = ['injection', 'secret', 'confidential', 'personal'] as const

/** What the Ship makes of `/session/:id/unlabel/:flag`, the address a reply's footer links. */
export type UnlabelRoute =
  | { ok: true; session: string; flag: string; says?: string; ask: string; call: ReturnType<typeof unlabelCall> }
  | { ok: false; why: string }

/** Read the footer's address (theseus-emqbm). A reply's footer lists the labels its session carries, each a link here, so
 *  the owner can take one off at any time and not only from the one message that raised it.
 *
 *  **It plans the removal; it does not make it.** The page shows what the removal does and takes one click, because the
 *  address is a one-way door: `ContentFlags::raise_after` will not raise a flag again for anything that entered the session
 *  before the removal, and removing `injection` gives up the refusal `[content_based_security] injection = "refuse"`
 *  would otherwise apply. A page that acted on load would act on a prefetch, a reopened tab or a restored history
 *  entry, and nothing here could put it back. The footer still carries a link and no
 *  button, which is what the owner asked for. */
export function unlabelRoute(session: string | undefined, flag: string | undefined): UnlabelRoute {
  if (!session) return { ok: false, why: 'That address names no session.' }
  if (!flag || !(FLAGS as readonly string[]).includes(flag))
    return { ok: false, why: `“${flag ?? ''}” is not a content label. They are ${FLAGS.join(', ')}.` }
  const says = effectWords(flag)
  return { ok: true, session, flag, ...(says ? { says } : {}), ask: askOf(flag, session), call: unlabelCall(session, flag) }
}

/** What the page says once the daemon has answered, in the daemon's own voice (`/trust`'s: what the removal lets the
 *  session do again). `by` is who the daemon recorded. */
export const removedWords = (flag: string, by: string): string =>
  `Removed the ${flag} label (by ${by})${flag === 'injection' ? ': calls that act are refused by nothing now' : ''}. The label and its removal stay in the ledger.`

/** What the page says when the removal did not happen. A flag the session does not carry is an error from the daemon, and
 *  it is the common one: the owner opened the same link twice, or took the label off from somewhere else. It is said
 *  plainly, never as a failure. */
export function unlabelFailureWords(flag: string, session: string, message: string): string {
  if (message.includes('carries no')) return `The ${flag} label is already off ${session}. Nothing was changed.`
  if (message.includes('does not count')) return `That removal was refused: ${message}`
  return `That removal did not happen: ${message}`
}
