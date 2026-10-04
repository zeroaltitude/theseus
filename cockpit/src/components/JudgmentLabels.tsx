// A judgment's label buttons (M5 25c; design §2.9, §2.13): the operator's word on whether Jev was right, for the
// learning ledger. Each press is confirmed first, and is `judge.label`, judged by the core as any surface's (the owner,
// from a private place). A whole judgment takes right, wrong, or noise; a question takes right or wrong, a yes-or-no
// question true or false, and a choice the option that was right. A per-item answer (rerank.v1's `helps.3`, one
// note's, M6 32d) takes true or false, labeled by its own question, and the label keeps the item's key.
import { useState } from 'react'
import type { JudgeLabelResult } from '@protocol'
import { call } from '@/lib/rpc'
import { labelChoices } from '@/lib/learning'

type D = Record<string, any>

/** The label buttons for one judgment's answers. */
export function JudgmentLabels({ id, answers }: { id: string; answers: D[] }) {
  const [busy, setBusy] = useState(false)
  const [done, setDone] = useState<string[]>([])
  const label = async (question: string | null, value: unknown, words: string) => {
    if (!window.confirm(`Label ${words}? It is written to the learning ledger as yours, and never edited.`)) return
    setBusy(true)
    try {
      const r = await call<JudgeLabelResult>('judge.label', { judgment: id, question, label: value })
      setDone((d) => [...d, `${r.question ?? 'the whole judgment'}${r.about ? ` (${r.about})` : ''}: ${JSON.stringify(r.label)}`])
    } catch (x: any) { window.alert(x?.message ?? String(x)) } finally { setBusy(false) }
  }
  const btn = (question: string | null, value: unknown, text: string, words: string) => (
    <button key={`${question}:${text}`} disabled={busy} onClick={() => label(question, value, words)}
      className="rounded px-1.5 text-[10.5px] text-ink-faint ring-1 ring-line hover:text-live disabled:opacity-50">{text}</button>
  )
  const whole = answers.filter((a) => a.about == null)
  const items = answers.filter((a) => a.about != null)
  return (
    <div>
      <div className="panel-title mb-1">labels</div>
      <div className="mb-1 flex flex-wrap items-center gap-1.5 text-[11px]">
        <span className="w-40 shrink-0 text-ink-faint">the whole judgment</span>
        {(['right', 'wrong', 'noise'] as const).map((w) => btn(null, w, w, `the whole judgment ${w}`))}
      </div>
      {whole.map((a) => {
        const q = String(a.question)
        const { bools, options: opts } = labelChoices(a.answer)
        return (
          <div key={q} className="mb-1 flex flex-wrap items-center gap-1.5 text-[11px]">
            <span className="w-40 shrink-0 truncate text-ink-faint">{q}</span>
            {btn(q, 'right', 'right', `${q} right`)}
            {btn(q, 'wrong', 'wrong', `${q} wrong`)}
            {bools && <>{btn(q, true, 'true', `${q} true`)}{btn(q, false, 'false', `${q} false`)}</>}
            {opts.length > 0 && (
              <select disabled={busy} value="" onChange={(e) => e.target.value && label(q, e.target.value, `${q}: ${e.target.value}`)}
                className="rounded bg-transparent text-[10.5px] ring-1 ring-line">
                <option value="">was…</option>
                {opts.map((o) => <option key={o} value={o}>{o}</option>)}
              </select>
            )}
          </div>
        )
      })}
      {items.map((a) => {
        const q = String(a.question)
        const about = String(a.about)
        const p = typeof a.answer?.noul === 'number' ? ` · ${Math.round(a.answer.noul * 100)}%` : ''
        return (
          <div key={q} className="mb-1 flex flex-wrap items-center gap-1.5 text-[11px]">
            <span className="w-40 shrink-0 truncate text-ink-faint" title={about}>{q} · {about}{p}</span>
            {btn(q, true, 'true', `${q} (${about}) true`)}
            {btn(q, false, 'false', `${q} (${about}) false`)}
          </div>
        )
      })}
      {done.map((d) => <div key={d} className="num text-[10.5px] text-live">labeled {d}</div>)}
    </div>
  )
}
