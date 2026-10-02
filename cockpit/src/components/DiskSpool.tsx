// Health's disk and the spool's last sweep (theseus-51v8): free space under the state dir against health's warning
// and the floor where new jobs are refused (theseus-102), and what the last sweep of the jobs' raw output removed and
// kept, by why (theseus-2ij). The Systems view's card, and the status strip's dot and attention item.
import { Link } from 'react-router'
import { HardDrive } from 'lucide-react'
import type { DiskStatus, SpoolSweep } from '@protocol'
import { DISK_STATE, diskSummary, diskTone, mb } from '@/lib/disk'
import { ago, bytes, cn, ms } from '@/lib/format'
import { toneClass, toneHex, type Tone } from '@/lib/taxonomy'
import { Empty, Field, LiveDot, Panel, Pill } from './ui'

/** The Systems view's card: the disk, then the spool's last sweep. */
export function DiskSpoolCard({ disk, sweep, now }: { disk?: DiskStatus; sweep?: SpoolSweep; now: number }) {
  return (
    <Panel title="Disk · spool" icon={<HardDrive size={13} />} bodyClassName="px-3.5 py-2.5">
      {disk?.path ? (
        <>
          <div className="flex items-baseline gap-2">
            <Pill tone={diskTone(disk)}>{DISK_STATE[disk.state] ?? disk.state}</Pill>
            {disk.state !== 'unknown' && <>
              <span className="num text-[13px] text-ink">{mb(disk.free_mb)}</span>
              <span className="text-[11px] text-ink-faint">free of {mb(disk.total_mb)}</span>
            </>}
          </div>
          {disk.state !== 'unknown' && <FreeMeter d={disk} />}
          <Field label="under" mono>{disk.path}</Field>
          <Field label="health warns below" mono><Mark tone="wait" />{disk.warn_mb > 0 ? mb(disk.warn_mb) : 'never'}</Field>
          <Field label="new jobs refused below" mono><Mark tone="fault" />{disk.floor_mb > 0 ? mb(disk.floor_mb) : 'never'}</Field>
          {disk.error && <Field label="error">{disk.error}</Field>}
        </>
      ) : <Empty>this daemon doesn't report its disk</Empty>}
      <div className="panel-title mb-1 mt-3">spool · the last sweep</div>
      {sweep ? (
        <>
          <Field label="swept" mono>{ago(sweep.at_unix_ms, now)} · took {ms(sweep.took_ms)}</Field>
          <Field label="removed" mono>{count(sweep.removed, 'file')} · {bytes(sweep.removed_bytes)}</Field>
          <ByWhy m={sweep.removed_by} />
          <Field label="kept" mono>{count(sweep.kept, 'file')} · {bytes(sweep.kept_bytes)}</Field>
          <ByWhy m={sweep.kept_by} />
          <Link to="/ledger?kind=spool.swept" className="text-[11px] text-live hover:underline">sweeps in the ledger →</Link>
        </>
      ) : <div className="text-[12px] text-ink-faint">no sweep yet: the first runs as the daemon starts serving, then one every hour</div>}
      <div className="mt-2 border-t border-line/50 pt-1.5 text-[11px] text-ink-faint">
        The filesystem Linux reports. Under WSL that is the virtual disk, a file on the Windows drive, and the drive can fill
        first while this still shows room.
      </div>
    </Panel>
  )
}

/** Free space as a fuel gauge: the fill is what is free, in the state's tone, on a lighter track of the same tone;
 *  the lines stand where health warns and where new jobs are refused. On a large disk both sit near the empty end,
 *  which is what they are: a sliver of it. */
function FreeMeter({ d }: { d: DiskStatus }) {
  const c = toneHex[diskTone(d)]
  const at = (n: number) => `${Math.min(100, (100 * n) / Math.max(1, d.total_mb))}%`
  return (
    <div className="relative my-2.5 h-2" role="meter" aria-label="free space" aria-valuemin={0} aria-valuemax={d.total_mb}
      aria-valuenow={d.free_mb} aria-valuetext={`${mb(d.free_mb)} free of ${mb(d.total_mb)}`}>
      <div className="absolute inset-0 rounded" style={{ background: `${c}33` }} />
      <div className="absolute inset-y-0 left-0 rounded" style={{ width: at(d.free_mb), background: c }} />
      {d.warn_mb > 0 && <Line at={at(d.warn_mb)} tone="wait" title={`health warns below ${mb(d.warn_mb)} free`} />}
      {d.floor_mb > 0 && <Line at={at(d.floor_mb)} tone="fault" title={`new jobs are refused below ${mb(d.floor_mb)} free`} />}
    </div>
  )
}

function Line({ at, tone, title }: { at: string; tone: Tone; title: string }) {
  return <div className="absolute -inset-y-1 w-0.5 -translate-x-1/2 rounded-full ring-2 ring-hull" style={{ left: at, background: toneHex[tone] }} title={title} />
}

/** A line's key beside its figure. */
function Mark({ tone }: { tone: Tone }) {
  return <span className="mr-1.5 inline-block h-2.5 w-0.5 rounded-full align-[-1px]" style={{ background: toneHex[tone] }} />
}

/** Each why and its count. A delete that failed faults; a file whose job the store could not be read for waits. */
function ByWhy({ m }: { m: Record<string, number> }) {
  const e = Object.entries(m).filter(([, n]) => n > 0).sort(([a], [b]) => a.localeCompare(b))
  if (!e.length) return null
  return (
    <div className="mb-0.5 flex flex-wrap justify-end gap-1">
      {e.map(([why, n]) => <Pill key={why} tone={why === 'failed' ? 'fault' : why === 'unread' ? 'wait' : 'idle'}>{why} {n}</Pill>)}
    </div>
  )
}

const count = (n: number, one: string) => `${n.toLocaleString()} ${n === 1 ? one : `${one}s`}`

/** The status strip's attention item while health says low or below the floor; it opens the Systems view. */
export function DiskAttention({ disk }: { disk?: DiskStatus }) {
  const tone = diskTone(disk)
  if (!disk || (tone !== 'wait' && tone !== 'fault')) return null
  return (
    <Link to="/systems" title={diskSummary(disk)}
      className={cn('flex shrink-0 items-center gap-1.5 rounded-md px-2 py-1 text-[11px] ring-1', toneClass[tone])}>
      <LiveDot tone={tone} size={6} />
      <span className="font-semibold uppercase tracking-wider">{tone === 'fault' ? 'disk below the floor' : 'disk low'}</span>
      <span className="num">{tone === 'fault' ? 'jobs refused' : `${mb(disk.free_mb)} free`}</span>
    </Link>
  )
}
