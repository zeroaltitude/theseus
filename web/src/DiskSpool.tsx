// Health's disk and the spool's last sweep, plainly (theseus-51v8): the CLI's `disk:` and `spool:` lines in the
// health panel. The free space is a native meter whose low and high are the floor and the warning, so the browser
// colors it as health judges it.
import type { DiskStatus, SpoolStatus } from './protocol'

const fmt = (n: number) => n.toLocaleString()

/** Each state's words and class. */
const STATE: Record<string, [string, string]> = {
  ok: ['ok', 'ok'],
  low: ['low', 'warn'],
  below_floor: ['below the floor: new jobs are refused', 'bad'],
  unknown: ['unknown', 'warn'],
}

const ago = (ms: number, now: number) => {
  const s = Math.max(0, Math.round((now - ms) / 1000))
  return s < 120 ? `${s} s ago` : s < 7200 ? `${Math.round(s / 60)} min ago` : `${Math.round(s / 3600)} h ago`
}

/** `1 absorbed, 2 ended`, the whys in order. */
const byWhy = (m: Record<string, number>) =>
  Object.entries(m)
    .filter(([, n]) => n > 0)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([why, n]) => `${fmt(n)} ${why}`)
    .join(', ')

/** Two lines of the health panel: the disk under the state dir, and the spool's last sweep. A daemon older than
 *  theseus-102 reports neither, and gets neither line. */
export function DiskSpoolLines({ disk, spool, now }: { disk?: DiskStatus; spool?: SpoolStatus; now: number }) {
  const sweep = spool?.last_sweep
  const [words, cls] = STATE[disk?.state ?? ''] ?? [disk?.state ?? '', 'warn']
  const removed = sweep ? byWhy(sweep.removed_by) : ''
  const kept = sweep ? byWhy(sweep.kept_by) : ''
  return (
    <>
      {disk?.path && (
        <div><span className="muted">disk</span> <b className={cls}>{words}</b>
          {disk.state === 'unknown'
            ? <span className="muted"> under <code>{disk.path}</code>: {disk.error ?? 'not read'}</span>
            : <>
              {' '}<meter min={0} max={disk.total_mb} low={disk.floor_mb} high={disk.warn_mb > 0 ? disk.warn_mb : disk.floor_mb}
                optimum={disk.total_mb} value={disk.free_mb} style={{ width: '8em', verticalAlign: 'middle' }}
                title={`${fmt(disk.free_mb)} MB free of ${fmt(disk.total_mb)} MB`} />
              {' '}<b>{fmt(disk.free_mb)}</b> <span className="muted">MB free of {fmt(disk.total_mb)} MB under <code>{disk.path}</code>
                {disk.warn_mb > 0 && <> · health warns below {fmt(disk.warn_mb)} MB</>}
                {disk.floor_mb > 0 && <> · new jobs are refused below {fmt(disk.floor_mb)} MB</>}
                {' '}(the filesystem Linux reports: under WSL, the Windows drive that holds the virtual disk can fill first)</span>
            </>}
        </div>
      )}
      {spool && (
        <div><span className="muted">spool</span>{' '}
          {sweep ? <>
            <span className="muted">swept {ago(sweep.at_unix_ms, now)}: removed</span> <b>{fmt(sweep.removed)}</b>
            <span className="muted"> {sweep.removed === 1 ? 'file' : 'files'} ({fmt(sweep.removed_bytes)} bytes){removed && `: ${removed}`} · kept</span> <b>{fmt(sweep.kept)}</b>
            <span className="muted"> ({fmt(sweep.kept_bytes)} bytes){kept && `: ${kept}`}</span>
            {(sweep.kept_by.failed ?? 0) > 0 && <span className="bad"> · {fmt(sweep.kept_by.failed ?? 0)} could not be deleted</span>}
            <span className="muted"> (a job's raw output, removed once no result will absorb it)</span>
          </> : <span className="muted">no sweep yet (the first runs as the daemon starts serving, then one every hour)</span>}
        </div>
      )}
    </>
  )
}
