// A subagent's answer (issue 28): the words another model profile gave a turn that asked it one question
// (`profile.ask`), drawn as a message of its own. Mirrors theseus-protocol's `SubagentAnswer::footer_bits`, which
// Discord, the terminal and the terminal UI print: the model part names the profile and the model that ANSWERED,
// never the model of the turn that asked, then what it is and who asked, then the call's own cost and time.
import type { SubagentAnswer } from '@protocol'
import { usd } from './figures.ts'

/** The icon a subagent's message opens with, before its first word: theseus-protocol's `SUBAGENT_ICON` (a test there
 *  holds the two together). */
export const SUBAGENT_ICON = '🛰️'

/** The answer a tool-result node's detail holds (`meta.subagent`), when it holds one. */
export function subagentOf(detail: unknown): SubagentAnswer | null {
  const a = (detail as { meta?: { subagent?: SubagentAnswer } } | null | undefined)?.meta?.subagent
  return a && typeof a.text === 'string' && a.text.trim() !== '' ? a : null
}

/** Its words as the message opens: the icon, then the first word (`SubagentAnswer::opening`). */
export function subagentOpening(a: SubagentAnswer): string {
  return `${SUBAGENT_ICON} ${a.text.trim()}`
}

/** What a surface says of an answer whose turn did not end well: theseus-protocol's `ended_words` (a test there holds
 *  the two together). `ended` is what the node list says (`detail.subagent_ended`). */
export function subagentEnded(ended: unknown): string | null {
  if (ended === 'stopped') return 'the turn that asked was stopped'
  if (ended === 'failed') return 'the turn that asked then failed'
  return null
}

/** The footer's bits with the words for a turn that did not end well after them, from a result node's detail. */
export function subagentBits(a: SubagentAnswer, detail: unknown): string[] {
  const ended = subagentEnded((detail as { subagent_ended?: unknown } | null | undefined)?.subagent_ended)
  return ended === null ? subagentFooter(a) : [...subagentFooter(a), ended]
}

/** The footer's bits, in a reply's order. */
export function subagentFooter(a: SubagentAnswer): string[] {
  const bits = [a.profile, a.model, a.asked_by ? `subagent answer, asked by ${a.asked_by}` : 'subagent answer']
  if (a.cut) bits.push('cut at its cap')
  if (a.cost_usd != null) bits.push(usd(a.cost_usd))
  bits.push(`${(a.elapsed_ms / 1000).toFixed(1)} s`)
  return bits
}
