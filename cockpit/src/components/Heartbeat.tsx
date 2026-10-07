// The heartbeat bar across the top of every view (theseus-hnof.5). It carried thirteen readouts in a row, and at
// 1366 px it ran out of room: the live profile, the one item that gave way, shrank to nothing, and the spend and the
// clock fell off the end. Now it reads in fewer, clearer instruments, each an engraved label over its value:
//
// - the wordmark and the version (the protocol in its tooltip);
// - the live profile, a chip whose menu lists every profile and makes one live (confirmed first);
// - up, and running;
// - what needs you, and the disk when it runs low;
// - the health lamps: the link, the kernel, the provider, Discord, the config, the secrets, the web UI, the binary and
//   the disk, each with its state in a word beside it (a narrow bar keeps the lamps and says the worst in words), and
//   a press opens their card, every lamp's whole sentence;
// - spent, with the cache's share; pause and refresh; the date and the clock.
// The flow of ledger rows moved to the activity strip's bar, beside the rows it counts; its planks stay under this bar.
//
// With the time machine set, the readouts the fold knows read the moment: the profile then, up then (or down then),
// running then, and spent then. Their labels say "then" in amber.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode, type RefObject } from 'react'
import { createPortal } from 'react-dom'
import { Link, useNavigate } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { Check, ChevronDown, Pause, Play, RefreshCw } from 'lucide-react'
import type { Health, ProfileList, SessionInfo } from '@protocol'
import { call, client, useConn, usePaused, useRpc } from '@/lib/rpc'
import { readNow } from '@/lib/history'
import { cn, tokens, uptime, usd, clock } from '@/lib/format'
import { binaryLine } from '@/lib/binary'
import { diskSummary } from '@/lib/disk'
import { useTick } from '@/lib/hooks'
import { useFlow } from '@/lib/flow'
import { useWorld } from '@/lib/world'
import { useAsOf, type World } from '@/lib/timemachine'
import {
  binaryLamp, configLamp, discordLamp, diskLamp, healthSummary, kernelLamp, linkLamp, providerLamp, secretsLamp, webLamp,
  type Lamp,
} from '@/lib/healthwords'
import type { Tone } from '@/lib/taxonomy'
import { LiveDot } from './ui'
import { DiskAttention } from './DiskSpool'
import { PlankStrip } from './brass'

/** A word in its lamp's tone: a state that is well reads in the ink, the rest in their tone. */
const WORD: Record<string, string> = { ok: 'text-ink-dim', live: 'text-ink-dim', wait: 'text-wait', fault: 'text-fault', idle: 'text-ink-faint' }

/** The bar. Live, it reads no history: only while the time machine is set does it fold the moment (useWorld follows
 *  the whole ledger, and would draw the bar again at every row). */
export function HeartbeatBar() {
  const past = useAsOf((s) => s.t !== null)
  return past ? <HeartbeatThen /> : <Heartbeat world={null} />
}

function HeartbeatThen() {
  return <Heartbeat world={useWorld()} />
}

function Heartbeat({ world }: { world: World | null }) {
  const { data: h, dataUpdatedAt } = useRpc<Health>('health', undefined, 2000)
  // The link's state, and the median of its last 12 round trips: the bar draws again when either changes, not at every
  // ping.
  const status = useConn((s) => s.status)
  const rtt = useConn((s) => median(s.rtts.slice(-12)))
  const nav = useNavigate()
  // What needs you (theseus-in3): each session's attention, from the push-kept list, the longest waiting first; one press
  // opens that session, as the Observatory's 'N need you' did.
  const { data: sl } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 2000)
  const needsYou = useMemo(() => (sl?.sessions ?? [])
    .filter((s) => (s.attention ? s.attention.level === 'needs_you' : (s.pending_confirms ?? 0) > 0))
    .sort((x, y) => (x.attention?.since_ms ?? 0) - (y.attention?.since_ms ?? 0)), [sl])
  const firstWaiting = needsYou[0]
  // Ledger rows of the last minute: one gold plank each (up to nine), in the strip under the bar.
  const { flow } = useFlow()
  const lit = Math.round(flow.slice(-30).reduce((a, b) => a + b * 2, 0))
  const g = world?.gauges
  const running = g ? g.running : h?.kernel.executions_by_state.running ?? 0
  const usage = g ? g.usageTotal : h?.usage_total
  const spent = g ? g.costTotal : h?.cost_usd_total
  const cacheHit = usage ? usage.cache_read_input_tokens / Math.max(1, usage.cache_read_input_tokens + usage.input_tokens + usage.cache_creation_input_tokens) : 0
  const lamps = useMemo<Lamp[]>(() => [
    linkLamp({ status, rtt }), kernelLamp(h), providerLamp(h), discordLamp(h), configLamp(h), secretsLamp(h), webLamp(h),
    binaryLamp(h?.binary, binaryLine(h?.binary)), diskLamp(h?.disk, diskSummary(h?.disk)),
  ], [h, status, rtt])

  return (
    <header className="brass-bar relative flex h-12 shrink-0 items-center gap-4 overflow-hidden whitespace-nowrap px-4">
      <div className="absolute inset-x-0 top-0 h-px live-sweep" />
      <div className="absolute inset-x-0 bottom-0" title="Each new plank is a ledger row of the last minute"><PlankStrip lit={lit} height={4} /></div>
      <div className="flex shrink-0 items-baseline gap-2" title={h ? `theseus ${h.version} · protocol ${h.protocol}` : undefined}>
        <span className="wordmark text-[15px]">THESEUS</span>
        <span className="num text-[11px] text-ink-faint">{h ? `v${h.version}` : '…'}</span>
      </div>
      <ProfileChip then={world ? { profile: g?.profile ?? null, t: world.t } : null} />
      {g ? (
        g.uptimeSecs !== null
          ? <Readout label="up" then tone="live" value={uptime(g.uptimeSecs)} title="how long the daemon had been up at the moment the ship's log shows" />
          : <Readout label="up" then tone="fault" value="down then" title="the daemon was down at the moment the ship's log shows: a stop with no start after it" />
      ) : <Readout label="up" tone="live" value={h ? <Uptime secs={h.uptime_secs} at={dataUpdatedAt} /> : '—'} title="how long the daemon has been up" />}
      <Readout label="running" then={!!g} tone={running ? 'live' : 'idle'} value={String(running)} title="executions running" />
      <div className="ml-auto flex shrink-0 items-center gap-3">
        {firstWaiting && (
          <button type="button" onClick={() => nav(`/session/${firstWaiting.session_id}`)}
            title={`sessions that need you: a question, a failure, or a block; this opens the longest waiting${firstWaiting.attention ? ` (${firstWaiting.attention.label})` : ''}`}
            className="flex shrink-0 items-center gap-1.5 rounded-md bg-wait/10 px-2 py-1 text-[11px] text-wait ring-1 ring-wait/40 hover:bg-wait/20">
            <LiveDot tone="wait" size={6} />
            <span className="font-semibold uppercase tracking-wider">{needsYou.length} need{needsYou.length === 1 ? 's' : ''} you</span>
          </button>
        )}
        <DiskAttention disk={h?.disk} />
        <HealthLamps lamps={lamps} />
        <Readout label={<>spent <span className="text-ink-faint/80">· cache</span></>} then={!!g} tone="money"
          value={<>{spent !== undefined ? usd(spent) : '—'}<span className="ml-1.5 text-[11px] text-ink-faint">{usage ? `${(cacheHit * 100).toFixed(0)}%` : '—'}</span></>}
          title={`every session's spend${g ? ' at the moment' : ''}; the cache: the share of input read from it${usage ? ` (${tokens(usage.cache_read_input_tokens)} input tokens read from cache)` : ''}`} />
        <PauseRefresh />
        <Clock />
      </div>
    </header>
  )
}

/** The middle of a few numbers, or null for none. */
function median(xs: number[]): number | null {
  if (!xs.length) return null
  const r = [...xs].sort((a, b) => a - b)
  return r[Math.floor(r.length / 2)]
}

/** The uptime, ticking each second between health's reads: only this draws again at each tick, not the bar. */
function Uptime({ secs, at }: { secs: number; at: number }) {
  const now = useTick()
  return <>{uptime(secs + Math.max(0, (now - at) / 1000))}</>
}

/** The date over the clock, ticking each second on its own. */
function Clock() {
  const now = useTick()
  return (
    <div className="flex shrink-0 flex-col items-end leading-none" title="this browser's local date and time">
      <span className="hdr-label">{new Date(now).toLocaleDateString('en-US', { weekday: 'short', month: 'short', day: 'numeric' })}</span>
      <span className="num mt-1 text-[12.5px] text-ink-dim">{clock(now)}</span>
    </div>
  )
}

/** An instrument of the bar: its engraved label over its value; "then" in amber while it reads the time machine's
 *  moment. */
function Readout({ label, value, tone, title, then }: { label: ReactNode; value: ReactNode; tone: Tone; title?: string; then?: boolean }) {
  return (
    <div className="flex shrink-0 flex-col justify-center leading-none" title={title}>
      <span className="hdr-label">{label}{then && <span className="text-wait"> · then</span>}</span>
      <span className="mt-1 flex items-center gap-1.5">
        <LiveDot tone={tone} pulse={!then && (tone === 'live' || tone === 'ok')} size={5} />
        <span className="num text-[12.5px] text-ink">{value}</span>
      </span>
    </div>
  )
}

/** Where a popover hangs: its anchor, and the anchor's box, read as it opens. */
function useAnchor<T extends HTMLElement>(open: boolean): [RefObject<T | null>, DOMRect | null] {
  const ref = useRef<T>(null)
  const [rect, setRect] = useState<DOMRect | null>(null)
  useLayoutEffect(() => { if (open && ref.current) setRect(ref.current.getBoundingClientRect()) }, [open])
  return [ref, rect]
}

/** A brass card hung under its anchor, outside the bar (whose overflow would clip it); a press outside it, Escape, or
 *  a resize closes it. */
function Popover({ open, onClose, anchorRef, rect, align, label, className, children }: {
  open: boolean; onClose: () => void; anchorRef: RefObject<HTMLElement | null>; rect: DOMRect | null; align: 'left' | 'right'
  label: string; className?: string; children: ReactNode
}) {
  const panel = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!open) return
    const down = (e: MouseEvent) => {
      const t = e.target as Node
      if (!panel.current?.contains(t) && !anchorRef.current?.contains(t)) onClose()
    }
    const key = (e: KeyboardEvent) => { if (e.key === 'Escape') { onClose(); anchorRef.current?.focus() } }
    document.addEventListener('mousedown', down)
    window.addEventListener('keydown', key)
    window.addEventListener('resize', onClose)
    return () => { document.removeEventListener('mousedown', down); window.removeEventListener('keydown', key); window.removeEventListener('resize', onClose) }
  }, [open, onClose, anchorRef])
  if (!open || !rect) return null
  const style = align === 'right' ? { top: rect.bottom + 8, right: Math.max(8, window.innerWidth - rect.right) } : { top: rect.bottom + 8, left: Math.max(8, rect.left) }
  return createPortal(
    <div ref={panel} role="dialog" aria-label={label} className={cn('brass-card hdr-pop fixed z-50 !p-0', className)} style={style}>{children}</div>,
    document.body,
  )
}

/** The live profile (the Observatory's header picker, theseus-vm3n.6): what new turns run on unless they name their
 *  own. Its menu lists every profile with its provider and model, and makes one live, confirmed first, as Systems'
 *  "make live" is. With the time machine set, it shows the profile live at the moment, and its menu only lists. */
function ProfileChip({ then }: { then: { profile: string | null; t: number } | null }) {
  const qc = useQueryClient()
  const { data: pl } = useRpc<ProfileList>('profile.list', undefined, 10_000)
  const [open, setOpen] = useState(false)
  const [busy, setBusy] = useState(false)
  const [cursor, setCursor] = useState(0)
  const [anchorRef, rect] = useAnchor<HTMLButtonElement>(open)
  const list = useRef<HTMLDivElement>(null)
  // Another surface's change shows at once.
  useEffect(() => {
    const off = client.onNotify((m) => { if (m === 'profile.changed') void qc.invalidateQueries({ queryKey: ['profile.list'] }) })
    return () => { off() }
  }, [qc])
  const profiles = pl?.profiles ?? []
  // The fold knows the profile from `profile.changed` rows; with none in the record, the live one has been live all along
  // (as the compass read it).
  const name = (then ? then.profile : null) ?? pl?.live ?? null
  const shown = profiles.find((p) => p.name === name)
  // The menu takes the keys as it opens.
  useEffect(() => { if (open && rect) list.current?.focus() }, [open, rect])
  const toggle = () => {
    if (!open) setCursor(Math.max(0, profiles.findIndex((p) => p.name === pl?.live)))
    setOpen(!open)
  }
  const makeLive = async (n: string) => {
    const p = profiles.find((x) => x.name === n)
    if (!p || p.live || then) return
    setOpen(false)
    if (!window.confirm(`Make "${n}" (${p.provider} · ${p.model}) the live profile? New turns run on it unless they name their own.`)) return
    setBusy(true)
    try { await call('profile.use', { name: n }); await qc.invalidateQueries() } catch (e: any) { window.alert(e?.message ?? String(e)) } finally { setBusy(false) }
  }
  const key = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') { e.preventDefault(); setCursor((c) => Math.min(profiles.length - 1, c + 1)) }
    if (e.key === 'ArrowUp') { e.preventDefault(); setCursor((c) => Math.max(0, c - 1)) }
    if ((e.key === 'Enter' || e.key === ' ') && profiles[cursor]) { e.preventDefault(); void makeLive(profiles[cursor].name) }
  }
  return (
    <>
      <button ref={anchorRef} type="button" disabled={!pl || busy} onClick={toggle} aria-haspopup="listbox" aria-expanded={open}
        title={pl ? `the live profile, from ${pl.live_source}: new turns run on it unless they name their own, and it persists across restarts${then ? ' · the ship’s log shows the profile live at its moment; return to LIVE to change it' : ' · press for every profile'}` : 'the live profile'}
        className="hdr-chip flex min-w-0 shrink items-center gap-2 text-left disabled:cursor-default">
        <span className="flex min-w-0 flex-col leading-none">
          <span className="hdr-label">{then ? <>live profile <span className="text-wait">· then</span></> : 'live profile'}</span>
          <span className="mt-1 flex min-w-0 items-center gap-1.5">
            <LiveDot tone="model" pulse={false} size={5} />
            <span className="truncate text-[12.5px] font-semibold text-ink">{name ?? '—'}</span>
            {shown && <span className="num hidden truncate text-[11px] text-ink-faint min-[1600px]:inline">{shown.provider}/{shown.model}</span>}
          </span>
        </span>
        <ChevronDown size={13} className={cn('shrink-0 text-ink-faint transition-transform', open && 'rotate-180')} />
      </button>
      <Popover open={open} onClose={() => setOpen(false)} anchorRef={anchorRef} rect={rect} align="left" label="Profiles" className="w-[380px]">
        <div className="border-b border-line px-3.5 py-2">
          <div className="panel-title">Profiles</div>
          <div className="mt-0.5 text-[11.5px] text-ink-faint">
            {then ? `as of ${clock(then.t)}, ${name ?? 'none'} was live; return to LIVE in the ship’s log to change it` : 'new turns run on the live one unless they name their own'}
          </div>
        </div>
        <div ref={list} role="listbox" aria-label="profiles" aria-activedescendant={profiles[cursor] ? `profile-${profiles[cursor].name}` : undefined} tabIndex={-1} onKeyDown={key} className="max-h-[340px] overflow-auto p-1.5 outline-none">
          {profiles.map((p, i) => (
            <div key={p.name} id={`profile-${p.name}`} role="option" aria-selected={p.name === name} aria-disabled={p.live || !!then}
              onMouseEnter={() => setCursor(i)} onClick={() => void makeLive(p.name)}
              className={cn('flex items-center gap-2.5 rounded-md px-2.5 py-1.5', i === cursor && 'bg-live/10', !p.live && !then && 'cursor-pointer')}>
              <span className="grid w-4 shrink-0 place-items-center">{p.name === name ? <Check size={13} className="text-model" /> : null}</span>
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[13px] font-semibold text-ink">{p.name}</span>
                <span className="num block truncate text-[11px] text-ink-faint">{p.provider}/{p.model}</span>
              </span>
              {p.live && <span className="shrink-0 rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-model ring-1 ring-model/40">live</span>}
            </div>
          ))}
        </div>
        <div className="flex items-center gap-2 border-t border-line px-3.5 py-2 text-[11px] text-ink-faint">
          <span className="min-w-0 flex-1 truncate">{pl ? `live from ${pl.live_source}; it persists across restarts` : ''}</span>
          <Link to="/systems" onClick={() => setOpen(false)} className="shrink-0 text-live hover:underline">Systems →</Link>
        </div>
      </Popover>
    </>
  )
}

/** The health lamps: on a wide bar each system's lamp with its name and its state in a word; on a narrow one the
 *  lamps alone and the worst of them in words. A press opens the card with every lamp's sentence. */
/** Whether the window is wide enough for every lamp's word: one row of lamps is drawn, never two with one hidden. */
function useWide(query = '(min-width: 1800px)'): boolean {
  const [wide, setWide] = useState(() => window.matchMedia(query).matches)
  useEffect(() => {
    const m = window.matchMedia(query)
    const on = () => setWide(m.matches)
    m.addEventListener('change', on)
    return () => m.removeEventListener('change', on)
  }, [query])
  return wide
}

function HealthLamps({ lamps }: { lamps: Lamp[] }) {
  const wide = useWide()
  const [open, setOpen] = useState(false)
  const [anchorRef, rect] = useAnchor<HTMLButtonElement>(open)
  const sum = healthSummary(lamps)
  return (
    <>
      <button ref={anchorRef} type="button" onClick={() => setOpen((v) => !v)} aria-haspopup="dialog" aria-expanded={open}
        title={`health: ${sum.words}; press for each system's card`} className="hdr-well flex shrink-0 items-center gap-3 px-2.5 py-1">
        {wide ? (
          // Wide: each lamp, its name and its word.
          <span className="flex items-center gap-3">
            {lamps.map((l) => (
              <span key={l.id} className="flex flex-col items-start leading-none" title={l.detail}>
                <span className="flex items-center gap-1"><LiveDot tone={l.tone} pulse={false} size={5} /><span className="hdr-label">{l.name}</span></span>
                <span className={cn('mt-1 text-[11px] font-medium', WORD[l.tone])}>{l.word}</span>
              </span>
            ))}
          </span>
        ) : (
          // Narrow: the lamps in a row, and the worst in words.
          <span className="flex items-center gap-2">
            <span className="flex flex-col items-start leading-none">
              <span className="hdr-label">health</span>
              <span className="mt-1.5 flex items-center gap-[5px]">{lamps.map((l) => <LiveDot key={l.id} tone={l.tone} pulse={false} size={5} />)}</span>
            </span>
            <span className={cn('max-w-[24ch] truncate text-[11px] font-medium', WORD[sum.tone])}>{sum.words}</span>
          </span>
        )}
      </button>
      <Popover open={open} onClose={() => setOpen(false)} anchorRef={anchorRef} rect={rect} align="right" label="Health" className="w-[460px]">
        <div className="flex items-baseline gap-2 border-b border-line px-3.5 py-2">
          <span className="panel-title">Health</span>
          <span className={cn('min-w-0 truncate text-[12px]', WORD[sum.tone])}>{sum.words}</span>
        </div>
        <div className="p-1.5">
          {lamps.map((l) => (
            <div key={l.id} className="grid grid-cols-[14px_76px_minmax(0,1fr)] items-baseline gap-x-2 rounded-md px-2.5 py-1.5">
              <LiveDot tone={l.tone} pulse={false} size={7} />
              <span className="hdr-label !text-[10px]">{l.name}</span>
              <span className="min-w-0">
                <span className={cn('block text-[12.5px] font-semibold', WORD[l.tone] === 'text-ink-dim' ? 'text-ink' : WORD[l.tone])}>{l.word}</span>
                <span className="block whitespace-normal text-[11.5px] leading-snug text-ink-faint">{l.detail}</span>
              </span>
            </div>
          ))}
        </div>
        <div className="flex justify-end border-t border-line px-3.5 py-2 text-[11px]">
          <Link to="/systems" onClick={() => setOpen(false)} className="text-live hover:underline">Systems has each one in full →</Link>
        </div>
      </Popover>
    </>
  )
}

/** Pause and refresh (the Observatory's live checkbox and its refresh, theseus-vm3n.6). Paused, no read runs on a
 *  timer and the ledger's follow waits, so the views hold still to be read; the push still comes, as it did there.
 *  Refresh reads everything on screen once, paused or not. */
function PauseRefresh() {
  const qc = useQueryClient()
  const paused = usePaused((s) => s.paused)
  const [reading, setReading] = useState(false)
  const refresh = () => {
    setReading(true)
    readNow()
    void qc.invalidateQueries().finally(() => setReading(false))
  }
  const toggle = () => {
    usePaused.setState({ paused: !paused })
    if (paused) refresh()
  }
  return (
    <div className="flex shrink-0 items-center gap-1">
      <button onClick={toggle}
        title={paused ? 'Paused: no read runs on a timer and the ledger’s follow waits; the push still comes. Press to go on.' : 'Pause: the views hold still to be read. No read runs on a timer and the ledger’s follow waits; the push still comes.'}
        className={cn('flex items-center gap-1 rounded-md px-1.5 py-1 text-[10px] font-semibold uppercase tracking-wider ring-1 transition-colors',
          paused ? 'bg-wait/15 text-wait ring-wait/50 shadow-[0_0_10px_-2px_#fbbf24]' : 'text-ink-faint ring-line hover:text-ink')}>
        {paused ? <><Play size={12} /> paused</> : <Pause size={12} />}
      </button>
      <button onClick={refresh} title="Refresh: read everything on screen again now" className="rounded-md p-1 text-ink-faint ring-1 ring-line hover:text-ink">
        <RefreshCw size={12} className={cn(reading && 'animate-spin')} />
      </button>
    </div>
  )
}
