// Judging, the control: run the learning loop for a pack now (`judge.learn`), read the proof of the judging model's canary
// (`judge.prove`, which only reads), and judge past events the model missed (`judge.backfill`, which calls a model and so
// costs money, and which the daemon refuses unless the configuration file says the owner consents). The pack ladder and its
// promote and roll-back are the Judging tab's.
import { useState } from 'react'
import { GraduationCap } from 'lucide-react'
import type { JudgeProveResult, PackListResult } from '@protocol'
import { Panel } from '@/components/ui'
import { useRpc } from '@/lib/rpc'
import { ActButton, FIELD } from '../ActButton'
import { backfillWords, isDay, learnWords } from './logic'

export function Judging() {
  const { data } = useRpc<PackListResult>('pack.list', undefined, 30_000)
  const packs = data?.packs.map((p) => p.pack) ?? []
  const [pack, setPack] = useState('')
  const chosen = pack || packs[0] || ''
  const [since, setSince] = useState('')
  const [proof, setProof] = useState<JudgeProveResult | null>(null)
  return (
    <Panel help="simple.config.judging" title="The judging model: learn, prove, fill in" icon={<GraduationCap size={13} />} bodyClassName="flex flex-col gap-4 p-4">
      <section className="flex flex-wrap items-center gap-2">
        <select value={chosen} onChange={(e) => setPack(e.target.value)} aria-label="which pack" className={FIELD}>
          {packs.map((p) => <option key={p} value={p}>{p}</option>)}
        </select>
        <ActButton verb="judge_learn" params={{ pack: chosen }} label="Learn now" effect={learnWords(chosen)} disabled={!chosen} submit="Yes, run it" words={(r) => r.said || r.decision} />
        <span className="mx-1 text-ink-faint">·</span>
        <input value={since} onChange={(e) => setSince(e.target.value)} placeholder="since day, 2026-10-01" aria-label="the first day to judge again" className={`${FIELD} w-[190px]`} />
        <ActButton verb="judge_backfill" params={{ pack: chosen, since: since.trim() }} label="Judge past events again" effect={backfillWords(chosen, since.trim())} disabled={!chosen || !isDay(since)} submit="Yes, spend the money"
          words={(r) => `judged ${r.judged} of ${r.events} events, ${r.failed} failed, cost $${r.cost_usd.toFixed(2)} of a $${r.limit_usd.toFixed(2)} limit`} />
      </section>
      <section className="flex flex-col gap-2 border-t border-line pt-3">
        <div className="flex items-center gap-2">
          <h3 className="simple-eyebrow">The proof of the loop pack’s canary</h3>
          <ActButton verb="judge_prove" params={{ since: null, until: null, min_tasks: null, min_labeled: null, records: false }} label="Read the proof" effect="Theseus reads its record and says whether the canary version of the loop pack did better than the one in use. Nothing is changed." tone="plain" onDone={setProof} words={(r) => `verdict: ${String(r.verdict).replaceAll('_', ' ')}`} />
        </div>
        {proof && <pre className="max-h-[320px] overflow-auto whitespace-pre-wrap rounded-md p-3 text-[12px] leading-snug text-ink-dim ring-1 ring-line">{proof.markdown}</pre>}
      </section>
    </Panel>
  )
}
