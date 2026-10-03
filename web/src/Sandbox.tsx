// L1, the sandbox (M4 17b, 18c), as health reports it: the CLI's `sandbox:` line, in the Observatory. The
// last real L1 launch since the start (theseus-gyin), the jobs by class, the egress list every L1 job may
// reach, and since the start the connections out and the refusals.
import type { SandboxHealth } from './protocol'

const bytes = (b: number) => b >= 1e6 ? `${(b / 1e6).toFixed(1)} MB` : b >= 1e3 ? `${(b / 1e3).toFixed(1)} KB` : `${b} B`
const n = (k: number, one: string, many: string) => `${k} ${k === 1 ? one : many}`

/** The Sandbox section's lines. A daemon older than 17b reports no sandbox, and gets one line that says so. */
export function SandboxLines({ s }: { s?: SandboxHealth | null }) {
  if (!s) return <div className="muted pad">this daemon reports no sandbox</div>
  const l = s.last_launch
  const egress = s.egress ?? []
  const refused = s.egress_refused ?? 0
  return (
    <div className="kv">
      <div>
        <span className="muted">L1</span>{' '}
        {s.refuses ? <span className="pill bad">unavailable: {s.refuses}</span>
          : !l ? <span className="muted">no L1 job yet since start</span>
          : l.ok ? <span className="pill ok">the last launch worked</span>
          : <span className="pill bad">the last launch failed: {l.why ?? 'no reason given'}</span>}
        {l?.start_ms != null && <span className="muted small"> · start {l.start_ms.toFixed(1)} ms</span>}
        {(l?.skipped ?? []).length > 0 && <span className="muted small"> · ro_paths missing: {l!.skipped!.join(', ')}</span>}
      </div>
      <div>
        <span className="muted">jobs</span> {s.jobs_l0} at L0, {s.jobs_l1} in L1 · default <code>{s.default}</code>
        {s.l1_argv.length > 0 && <span className="muted small"> · always L1: {s.l1_argv.join('; ')}</span>}
      </div>
      <div title="[sandbox] egress: the hosts every L1 job may reach through its proxy. A call that names more waits for approval, which reaches those hosts alone. What a job brings back from a host beyond this list is outside text, so its session waits after it.">
        <span className="muted">egress</span>{' '}
        {egress.length === 0
          ? <span className="muted">none listed: an L1 job has no network, unless its call names hosts and is approved</span>
          : egress.map((h) => <code key={h}>{h} </code>)}
      </div>
      <div>
        <span className="muted">since the start</span>{' '}
        {n(s.egress_connections ?? 0, 'connection', 'connections')} out, {bytes(s.egress_up ?? 0)} up, {bytes(s.egress_down ?? 0)} down ·{' '}
        <span className={refused > 0 ? 'warn' : 'muted'}>{refused} refused</span>
        {s.egress_last_refused && <span className="muted small"> (latest: {s.egress_last_refused})</span>}
      </div>
    </div>
  )
}
