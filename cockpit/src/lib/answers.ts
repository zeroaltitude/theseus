// A question's answers as the cockpit draws them (theseus-ds5os): the buttons come from the question's own `options`
// (the daemon words them once, `answer_options`), and none is drawn for a connection the place rule would refuse
// (`may_answer`). Every answer goes through `work.answer`. Pure: no DOM.
import type { AnswerOption, ConfirmRequest } from '@protocol'

export type Tone = 'ok' | 'wait' | 'fault'

export type AnswerButton = { id: string; label: string; tone: Tone; takesNote: boolean; title: string }

/** What a daemon that sends no options (an older one) means: the answers its kind has always had. */
export function legacyOptions(c: ConfirmRequest): AnswerOption[] {
  const yes = c.budget ? ['reset', 'Reset to $0'] : c.change ? ['accept', 'Accept'] : c.tool === 'extend.ack' ? ['ack', 'Acknowledge'] : ['approve', 'Approve']
  const out: AnswerOption[] = [{ id: yes[0], label: yes[1], style: 'primary', takes_note: false, effect: '' }]
  out.push({ id: 'decline', label: 'Decline', style: 'plain', takes_note: !c.budget, effect: '' })
  return out
}

/** The tone of a button: a decline is a fault's, and the rest approve. */
export function toneOf(o: AnswerOption): Tone {
  if (o.id === 'decline') return 'fault'
  return o.style === 'danger' ? 'fault' : 'ok'
}

/** Whether this connection may answer it. A question heard on the push says nothing, and reads yes: the daemon refuses
 *  an answer that does not count, and says where to answer instead. */
export const mayAnswer = (c: ConfirmRequest): boolean => c.may_answer !== false

/** The buttons: the question's options in order; none when this connection may not answer. */
export function buttonsOf(c: ConfirmRequest): AnswerButton[] {
  if (!mayAnswer(c)) return []
  const options = c.options && c.options.length > 0 ? c.options : legacyOptions(c)
  return options.map((o) => ({ id: o.id, label: o.label, tone: toneOf(o), takesNote: o.takes_note, title: o.effect }))
}

/** The `work.answer` params of pressing `option`, with the note only where the option takes one, and the code from the
 *  owner's authenticator app (issue 22) only on an approving answer to a question that needs the second proof: the
 *  daemon checks it and it goes nowhere else. A decline never carries one. */
export function answerParams(c: ConfirmRequest, option: string, note: string, code = ''): Record<string, unknown> {
  const takes = buttonsOf(c).find((b) => b.id === option)?.takesNote
  const out: Record<string, unknown> = { question_id: c.correlation_id, option, note: (takes && note.trim()) || undefined }
  if (c.needs_proof && option !== 'decline' && code.trim()) out.code = code.trim()
  return out
}

/** The card's line about the second proof, in the daemon's own words for this place: "second proof: needed, enter the
 *  code" or "second proof: given here 3 hours ago, good for 21 hours more". None for a question that asks for none. */
export function proofLine(c: ConfirmRequest): string | null {
  if (!c.needs_proof) return null
  return c.proof?.line ?? 'second proof: needed, enter the code from your authenticator app'
}

/** Whether the card shows the code field: the question needs the proof and no earlier one from this place covers it. */
export const needsCode = (c: ConfirmRequest): boolean => Boolean(c.needs_proof) && c.proof?.code_needed !== false

/** Whether an open window of this place covers the question, so Approve goes through with no code (issue 22). */
export const goesThrough = (c: ConfirmRequest): boolean => Boolean(c.needs_proof) && c.proof?.code_needed === false

/** A change to Theseus's own settings or files (`config.ack`) is a card the daemon built to be read as it is: its lines
 *  keep their breaks. */
export const isSettingsChange = (c: ConfirmRequest): boolean => c.tool === 'config.ack'

/** Whether the card has a whole text to read before the yes (`config.whole`): a prompt file's change, or a setting too
 *  long for the card. The daemon says so in the question's own arguments. */
export const hasWholeText = (c: ConfirmRequest): boolean =>
  isSettingsChange(c) && (c.input as { whole?: unknown } | null)?.whole === true
