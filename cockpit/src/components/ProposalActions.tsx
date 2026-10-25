// Reject and Never on the proposals panel (theseus-648f8), in a file of their own beside `People.tsx`: one row's
// controls, the picked rows' bulk pair, and the nevers kept (each with its Undo). Every write is confirmed first
// through the panel's `writeEach` and judged by the core; `busy` and `disabled` hold them as they hold the row's ✓/✗.
import { Ban, X } from 'lucide-react'
import type { OntologyNeverListResult, OntologyPersonProposals, OntologyProposal } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { Act } from '@/components/OntologyParts'
import { answerCall, confirmWords, neverLine, neverWords, undoWords, type Answer } from '@/lib/proposalActions'

export type WriteEach = <T>(ask: string, calls: readonly (readonly [string, unknown])[]) => Promise<T[] | null>
interface Held { disabled: boolean; busy: boolean; writeEach: WriteEach }

const small = 'px-1 text-ink-faint disabled:opacity-40'

/** A topic row's Reject and Never. An unnamed new topic's Never keeps its session. */
export function TopicActions({ p, disabled, busy, writeEach }: Held & { p: OntologyProposal }) {
  const go = (a: Answer) => void writeEach(confirmWords(a, [p], []), [answerCall(a, [p.judgment])])
  return (
    <td className="whitespace-nowrap px-1 text-right">
      <button type="button" disabled={disabled || busy} onClick={() => go('reject')} aria-label={`reject ${p.judgment}`} title="ontology.proposal.reject_all: labelled rejected, confirmed first" className={`${small} hover:text-fault`}><X size={12} /></button>
      <button type="button" disabled={disabled || busy} onClick={() => go('never')} aria-label={`never ${p.judgment}`} title={`ontology.proposal.never: ${neverWords(p)}; undo it under nevers`} className={`${small} hover:text-wait`}><Ban size={12} /></button>
    </td>
  )
}

/** A person row's Never, beside its ✓ and ✗. */
export function PersonNever({ g, disabled, busy, writeEach }: Held & { g: OntologyPersonProposals }) {
  return (
    <button type="button" disabled={disabled || busy} onClick={() => void writeEach(confirmWords('never', [], [g]), [answerCall('never', g.judgments)])} aria-label={`never ${g.name}`} title={`ontology.proposal.never: never propose “${g.name}” again; undo it under nevers`} className={`${small} hover:text-wait`}><Ban size={12} /></button>
  )
}

/** The picked rows' Reject and Never. */
export function BulkActions({ topics, people, onDone, disabled, busy, writeEach }: Held & { topics: OntologyProposal[]; people: OntologyPersonProposals[]; onDone: () => void }) {
  const n = topics.length + people.length
  const go = async (a: Answer) => {
    const judgments = [...topics.map((p) => p.judgment), ...people.flatMap((g) => g.judgments)]
    if (await writeEach(confirmWords(a, topics, people), [answerCall(a, judgments)])) onDone()
  }
  return (
    <>
      <Act onClick={() => void go('reject')} busy={busy} off={disabled || n === 0} title="ontology.proposal.reject_all: confirmed first"><X size={12} /> Reject {n || ''}</Act>
      <Act onClick={() => void go('never')} busy={busy} off={disabled || n === 0} title="ontology.proposal.never: reject, and never propose these again; undo under nevers"><Ban size={12} /> Never {n || ''}</Act>
    </>
  )
}

/** The nevers kept in the store, each with Undo, and the config's lists (kept in the file, not undone here). */
export function NeversList({ disabled, busy, writeEach }: Held) {
  const { data } = useRpc<OntologyNeverListResult>('ontology.never.list', {}, 15_000)
  const n = data?.nevers.length ?? 0
  const config = (data?.config_topics.length ?? 0) + (data?.config_people.length ?? 0)
  return (
    <details className="text-[11px]">
      <summary className="cursor-pointer text-ink-faint hover:text-ink">nevers · {data ? n : '…'}{config ? ` · ${config} in the config` : ''}</summary>
      <ul className="mt-1 flex flex-col gap-0.5">
        {data?.nevers.map((v) => (
          <li key={v.key} className="flex items-center gap-2 text-ink-dim">
            <span className="min-w-0 flex-1 truncate" title={`kept by ${v.who}`}>{neverLine(v)}</span>
            <button type="button" disabled={disabled || busy} onClick={() => void writeEach(undoWords(v), [['ontology.never.undo', { key: v.key }]])} aria-label={`undo never ${v.key}`} className="text-ink-faint hover:text-live disabled:opacity-40">undo</button>
          </li>
        ))}
        {data?.config_topics.map((t) => <li key={`t:${t}`} className="text-ink-faint">config not_topics · {t}</li>)}
        {data?.config_people.map((t) => <li key={`p:${t}`} className="text-ink-faint">config not_people · {t}</li>)}
        {data && n + config === 0 && <li className="text-ink-faint">none</li>}
      </ul>
    </details>
  )
}
