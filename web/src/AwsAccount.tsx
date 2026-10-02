// The AWS accounts Theseus owns (row 29, C1; the AWS design's account panel, §3.8): each bound account, as its
// check left it, with its region and its requests. The CLI's `aws:` lines, in the health panel. A daemon that binds
// no account, or is older than C1, reports none, and gets no line.
import type { AwsStatus } from './protocol'

const fmt = (n: number) => n.toLocaleString()

const ago = (ms: number, now: number) => {
  const s = Math.max(0, Math.round((now - ms) / 1000))
  return s < 120 ? `${s} s ago` : s < 7200 ? `${Math.round(s / 60)} min ago` : `${Math.round(s / 3600)} h ago`
}

/** Each state's words and class. */
const STATE: Record<string, [string, string]> = {
  bound: ['bound', 'ok'],
  failed: ['not bound: its calls fail closed', 'bad'],
  waiting: ['waiting for its key', 'warn'],
  checking: ['checking its key', 'muted'],
  unchecked: ['not checked yet', 'muted'],
}

/** One line per bound AWS account. */
export function AwsAccountLines({ aws, now }: { aws?: AwsStatus; now: number }) {
  if (!aws) return null
  return (
    <>
      {aws.accounts.map((a) => {
        const [words, cls] = STATE[a.state] ?? [a.state, 'warn']
        const more = a.regions.filter((r) => r !== a.region)
        return (
          <div key={a.account}><span className="muted">aws</span> <b>{a.account}</b> <b className={cls}>{words}</b>
            {a.arn && <span className="muted"> as <code>{a.arn}</code></span>}
            {a.checked_at_unix_ms !== undefined && <span className="muted"> · checked {ago(a.checked_at_unix_ms, now)}</span>}
            {a.error && <span className={a.state === 'failed' ? 'bad' : 'warn'}> · {a.error}</span>}
            <span className="muted"> · region</span> <b>{a.region}</b>
            {more.length > 0 && <span className="muted"> (a call may name {more.join(', ')})</span>}
            <span className="muted"> · requests</span> <b>{fmt(a.calls)}</b>
            {a.failed > 0 && <span className="warn"> ({fmt(a.failed)} failed)</span>}
            <span className="muted"> (its key is checked after serving: until STS names this account for it, no call of
              the account signs)</span>
          </div>
        )
      })}
    </>
  )
}
