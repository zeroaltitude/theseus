// The cockpit's frame: the nav rail, the heartbeat bar across the top, the view, and the activity river below.
import { useEffect, useMemo, useRef, useState } from 'react'
import { NavLink, Outlet, useLocation, useMatch, useNavigate } from 'react-router'
import { motion } from 'motion/react'
import { Command } from 'cmdk'
import { useQueryClient } from '@tanstack/react-query'
import {
  Activity, BellOff, BellRing, CircleCheck, Coins, Command as CommandIcon, Cpu, Crosshair, Gauge, Landmark, Layers, Navigation, OctagonX, Pause, Play, Radio,
  RefreshCw, Sailboat, ScrollText, ShieldCheck, ShieldHalf, Shapes, Zap,
} from 'lucide-react'
import type { ConfirmRequest, ExecutionInfo, Health, NodeInfo, ProfileList, SessionInfo } from '@protocol'
import { call, client, useConn, usePaused, useRpc, usePush } from '@/lib/rpc'
import { readNow } from '@/lib/history'
import { useLedger } from '@/lib/derive'
import { summarize } from '@/lib/summary'
import { cn, ms, short, tokens, uptime, usd, clock, stamp } from '@/lib/format'
import { ledgerKind, partTone, stateTone, toneHex } from '@/lib/taxonomy'
import { LiveDot, Spark } from './ui'
import { DiskAttention } from './DiskSpool'
import { Coin, PlankStrip } from './brass'
import { diskSummary, diskTone } from '@/lib/disk'
import { useHistory, useTick } from '@/lib/hooks'
import { useAsOf } from '@/lib/timemachine'
import { FOLDS } from '@/lib/world'
import { TimeMachine } from './TimeMachine'

const NAV = [
  { to: '/ship', label: 'Ship', icon: Sailboat },
  { to: '/bridge', label: 'Bridge', icon: Gauge },
  { to: '/fleet', label: 'Fleet', icon: Layers },
  { to: '/actions', label: 'Actions', icon: ShieldCheck },
  { to: '/boundaries', label: 'Bounds', icon: ShieldHalf },
  { to: '/ledger', label: 'Ledger', icon: ScrollText },
  { to: '/money', label: 'Money', icon: Landmark },
  { to: '/economics', label: 'Economics', icon: Coins },
  { to: '/speed', label: 'Speed', icon: Zap },
  { to: '/systems', label: 'Systems', icon: Cpu },
  { to: '/ontology', label: 'Ontology', icon: Shapes },
] as const

const GO: Record<string, string> = {
  h: '/ship', b: '/bridge', f: '/fleet', a: '/actions', o: '/boundaries', l: '/ledger', m: '/money', e: '/economics', w: '/speed', s: '/systems',
}

export function Shell() {
  const nav = useNavigate()
  const [palette, setPalette] = useState(false)
  // The Ship is full-bleed: the river starts folded there (and open elsewhere), and the main area has no margin.
  // The index redirects to the Ship, so it counts as the Ship.
  const shipRoute = useMatch('/ship')
  const indexRoute = useMatch({ path: '/', end: true })
  const onShip = !!shipRoute || !!indexRoute
  // On a short screen the river starts folded too: the views and the ship's log need the height.
  const [river, setRiver] = useState(!onShip && window.innerHeight >= 900)
  // A view showing the time machine's moment wears an amber frame. Only whether it is set: a scrub moves the moment
  // every frame, and the frame around the views must not draw again for each.
  const past = useAsOf((s) => s.t !== null)
  const loc = useLocation()
  const pastView = past && (onShip || FOLDS.some((p) => loc.pathname.startsWith(p)))
  useEffect(() => {
    // Ctrl/Cmd+K opens the palette; "g" then a letter jumps to a view (`GO`), unless you are typing in a field.
    let g = 0
    const k = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') { e.preventDefault(); setPalette((v) => !v); return }
      const t = e.target as HTMLElement | null
      if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return
      if (e.metaKey || e.ctrlKey || e.altKey) return
      if (e.key === 'g') { g = Date.now(); return }
      if (g && Date.now() - g < 900 && GO[e.key]) { e.preventDefault(); nav(GO[e.key]) }
      g = 0
    }
    window.addEventListener('keydown', k)
    return () => window.removeEventListener('keydown', k)
  }, [nav])

  return (
    <div className="cockpit-bg flex h-full">
      <NavRail onPalette={() => setPalette(true)} />
      <div className="flex min-w-0 flex-1 flex-col">
        <HeartbeatBar />
        <main className={cn(onShip ? 'relative min-h-0 flex-1 overflow-hidden' : 'min-h-0 flex-1 overflow-auto px-4 pb-4 pt-3', pastView && 'asof-frame')}>
          <Outlet />
        </main>
        <ActivityRiver open={river} onToggle={() => setRiver((v) => !v)} />
        <TimeMachine />
      </div>
      <Palette open={palette} onOpenChange={setPalette} />
    </div>
  )
}

/** Opt-in desktop notices: each approval that starts waiting is announced once, while this page is open. */
function useApprovalNotices(confirms: ConfirmRequest[] | undefined) {
  const [on, setOn] = useState(() => localStorage.getItem('cockpit.notify') === 'on' && typeof Notification !== 'undefined' && Notification.permission === 'granted')
  const seen = useRef<Set<string> | null>(null)
  useEffect(() => {
    if (!confirms) return
    const ids = new Set(confirms.map((c) => c.correlation_id))
    if (seen.current && on && typeof Notification !== 'undefined' && Notification.permission === 'granted') {
      for (const c of confirms) {
        if (!seen.current.has(c.correlation_id)) {
          new Notification(`Theseus: ${c.tool} waits for you`, { body: c.reason, tag: c.correlation_id })
        }
      }
    }
    seen.current = ids
  }, [confirms, on])
  const toggle = async () => {
    if (on) { setOn(false); localStorage.setItem('cockpit.notify', 'off'); return }
    if (typeof Notification === 'undefined') { window.alert('This browser has no desktop notifications.'); return }
    const p = Notification.permission === 'granted' ? 'granted' : await Notification.requestPermission()
    if (p === 'granted') { setOn(true); localStorage.setItem('cockpit.notify', 'on') }
  }
  return { on, toggle }
}

function NavRail({ onPalette }: { onPalette: () => void }) {
  const status = useConn((s) => s.status)
  // Approvals waiting, from every view: the Actions item carries the count, and pulses while anything waits.
  const { data: cl } = useRpc<{ confirms: ConfirmRequest[] }>('confirm.list', undefined, 2000)
  const waiting = cl?.confirms.length ?? 0
  const notify = useApprovalNotices(cl?.confirms)
  return (
    <nav className="brass-rail relative z-10 flex w-[68px] shrink-0 flex-col items-center gap-1 py-3">
      <div className="mb-3 flex flex-col items-center">
        <div className="relative" title="Theseus">
          <Coin size={42} className="drop-shadow-[0_0_10px_rgba(214,165,72,0.35)]" />
          <span className="absolute -right-0.5 top-0"><LiveDot tone={status === 'open' ? 'ok' : status === 'connecting' ? 'wait' : 'fault'} size={7} /></span>
        </div>
      </div>
      {NAV.map(({ to, label, icon: Icon, ...rest }) => (
        <NavLink
          key={to}
          to={to}
          end={'end' in rest}
          className={({ isActive }) => cn(
            'group relative flex w-[62px] flex-col items-center gap-0.5 rounded-lg py-2 font-display text-[8.5px] font-bold uppercase tracking-[0.03em] transition-colors',
            isActive ? 'text-live' : 'text-ink-faint hover:bg-gold/10 hover:text-ink',
          )}
        >
          {({ isActive }) => (
            <>
              {isActive && (
                <motion.span layoutId="nav-active" className="absolute inset-0 rounded-lg bg-live/10 shadow-[0_0_14px_-2px_rgba(34,211,238,0.45),inset_0_0_0_1px_rgba(34,211,238,0.35)]" transition={{ type: 'spring', stiffness: 400, damping: 32 }} />
              )}
              <Icon size={18} className={cn('relative', isActive && 'drop-shadow-[0_0_6px_rgba(34,211,238,0.8)]')} />
              <span className="relative">{label}</span>
              {to === '/actions' && waiting > 0 && (
                <span className="absolute right-1.5 top-1 grid h-4 min-w-4 place-items-center rounded-full bg-wait px-1 text-[9.5px] font-bold text-void shadow-[0_0_10px_#fbbf24] animate-pulse-soft">{waiting}</span>
              )}
            </>
          )}
        </NavLink>
      ))}
      <div className="mt-auto flex flex-col items-center gap-2">
        <button onClick={notify.toggle} title={notify.on ? 'Desktop notices for approvals: on' : 'Desktop notices for approvals: off'}
          className={cn('rounded-lg p-2 hover:bg-white/5', notify.on ? 'text-wait' : 'text-ink-faint hover:text-ink')}>
          {notify.on ? <BellRing size={17} /> : <BellOff size={17} />}
        </button>
        <button onClick={onPalette} title="Command palette (Ctrl+K)" className="rounded-lg p-2 text-ink-faint hover:bg-white/5 hover:text-ink">
          <CommandIcon size={17} />
        </button>
      </div>
    </nav>
  )
}

function HeartbeatBar() {
  const { data: h, dataUpdatedAt } = useRpc<Health>('health', undefined, 2000)
  const conn = useConn()
  const nav = useNavigate()
  // What needs you (theseus-in3): each session's attention, from the push-kept list, the longest waiting first; one press
  // opens that session, as the Observatory's 'N need you' did.
  const { data: sl } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 2000)
  const needsYou = useMemo(() => (sl?.sessions ?? [])
    .filter((s) => (s.attention ? s.attention.level === 'needs_you' : (s.pending_confirms ?? 0) > 0))
    .sort((x, y) => (x.attention?.since_ms ?? 0) - (y.attention?.since_ms ?? 0)), [sl])
  const firstWaiting = needsYou[0]
  const now = useTick()
  const rtt = useMemo(() => {
    const r = conn.rtts.slice(-12).sort((a, b) => a - b)
    return r.length ? r[Math.floor(r.length / 2)] : null
  }, [conn.rtts])
  const up = h ? h.uptime_secs + Math.max(0, (now - dataUpdatedAt) / 1000) : 0
  // Flow: ledger rows per second, from the total's growth between polls; the header's own heartbeat line.
  const { data: tail } = useLedger(1, 2000)
  const totals = useHistory(tail?.total, 60, 2000)
  const flow = totals.slice(1).map((t, i) => Math.max(0, (t - totals[i]) / 2))
  // Ledger rows of the last minute: one gold plank each (up to nine), in the strip under the bar.
  const lit = Math.round(flow.slice(-30).reduce((a, b) => a + b * 2, 0))
  const running = h?.kernel.executions_by_state.running ?? 0
  const discord = h?.bindings?.find((b) => b.kind === 'discord')
  const usage = h?.usage_total
  const cacheHit = usage ? usage.cache_read_input_tokens / Math.max(1, usage.cache_read_input_tokens + usage.input_tokens + usage.cache_creation_input_tokens) : 0

  return (
    <header className="brass-bar relative flex h-12 shrink-0 items-center gap-4 overflow-hidden whitespace-nowrap px-4">
      <div className="absolute inset-x-0 top-0 h-px live-sweep" />
      <div className="absolute inset-x-0 bottom-0" title="Each new plank is a ledger row of the last minute"><PlankStrip lit={lit} height={4} /></div>
      <div className="flex shrink-0 items-baseline gap-2" title={h ? `theseus ${h.version} · protocol ${h.protocol}` : undefined}>
        <span className="wordmark text-[15px]">THESEUS</span>
        <span className="num text-[11px] text-ink-faint">{h ? `v${h.version}` : '…'}</span>
      </div>
      <Indicator label="link" tone={conn.status === 'open' ? 'ok' : conn.status === 'connecting' ? 'wait' : 'fault'} value={conn.status === 'open' ? (rtt !== null ? ms(rtt) : 'open') : conn.status} />
      <Indicator label="up" tone="live" value={h ? uptime(up) : '—'} />
      <Indicator label="running" tone={running ? 'live' : 'idle'} value={String(running)} />
      <div className="flex shrink-0 items-center gap-1.5" title="ledger rows per second, last two minutes">
        <span className="text-[10px] font-semibold uppercase tracking-wider text-ink-faint">flow</span>
        <div className="w-20"><Spark data={flow.length > 1 ? flow : [0, 0]} tone="live" height={22} /></div>
        <span className="num w-11 text-[12px] text-ink">{(flow[flow.length - 1] ?? 0).toFixed(1)}/s</span>
      </div>
      <LiveProfile />
      <div className="ml-auto flex shrink-0 items-center gap-3">
        {firstWaiting && (
          <button type="button" onClick={() => nav(`/session/${firstWaiting.session_id}`)}
            title={`sessions that need you: a question, a failure, or a block; this opens the longest waiting${firstWaiting.attention ? ` (${firstWaiting.attention.label})` : ''}`}
            className="flex shrink-0 items-center gap-1.5 rounded-md bg-wait/10 px-2 py-1 text-[11px] text-wait ring-1 ring-wait/40 hover:bg-wait/20">
            <LiveDot tone="wait" size={6} />
            <span className="font-semibold uppercase tracking-wider">{needsYou.length} need{needsYou.length === 1 ? 's' : ''} you</span>
          </button>
        )}
        {!!h?.provider_errors && <Indicator label="provider errors" tone="fault" value={String(h.provider_errors)} title="model calls the provider failed since the daemon started; the ledger's provider.error rows say each" />}
        <DiskAttention disk={h?.disk} />
        <div className="flex items-center gap-2.5 rounded-md bg-white/[0.03] px-2 py-1 ring-1 ring-line">
          <Dot label="kernel" tone={h?.kernel.accepting ? 'ok' : 'wait'} title={h ? (h.kernel.accepting ? 'kernel accepting' : 'kernel holding new turns') : ''} />
          <Dot label="discord" tone={stateTone(discord?.state)} title={discord ? `Discord ${discord.state}${discord.latency_ms ? ` · ${discord.latency_ms} ms` : ''}${discord.detail ? ` · ${discord.detail}` : ''}` : 'Discord'} />
          <Dot label="config" tone={stateTone(h?.config?.state)} title={`config ${h?.config?.state ?? '—'} (${h?.config?.source ?? '—'})`} />
          <Dot label="secrets" tone={stateTone(h?.secrets?.state)} title={`secrets ${h?.secrets?.state ?? '—'}`} />
          <Dot label="web" tone={webTone(h?.web)} title={webTitle(h?.web)} />
          <Dot label="disk" tone={diskTone(h?.disk)} title={diskSummary(h?.disk)} />
        </div>
        <Indicator label="cache" tone="think" value={usage ? `${(cacheHit * 100).toFixed(0)}%` : '—'} title={usage ? `${tokens(usage.cache_read_input_tokens)} input tokens read from cache` : undefined} />
        <Indicator label="spent" tone="money" value={h?.cost_usd_total !== undefined ? usd(h.cost_usd_total) : '—'} />
        <PauseRefresh />
        <span className="num text-[12px] text-ink-dim">{clock(now)}</span>
      </div>
    </header>
  )
}

/** The live profile, picked in the bar (the Observatory's header picker, theseus-vm3n.6): what new turns run on
 *  unless they name their own. Confirmed first, as Systems' "make live" is; off while the time machine shows the past. */
function LiveProfile() {
  const qc = useQueryClient()
  const past = useAsOf((s) => s.t !== null)
  const { data: pl } = useRpc<ProfileList>('profile.list', undefined, 10_000)
  const [busy, setBusy] = useState(false)
  // Another surface's change shows at once.
  useEffect(() => {
    const off = client.onNotify((m) => { if (m === 'profile.changed') void qc.invalidateQueries({ queryKey: ['profile.list'] }) })
    return () => { off() }
  }, [qc])
  if (!pl) return <Indicator label="live" tone="model" value="—" />
  const use = async (name: string) => {
    const p = pl.profiles.find((x) => x.name === name)
    if (!p || p.live) return
    if (!window.confirm(`Make "${name}" (${p.provider} · ${p.model}) the live profile? New turns run on it unless they name their own.`)) return
    setBusy(true)
    try { await call('profile.use', { name }); await qc.invalidateQueries() } catch (e: any) { window.alert(e?.message ?? String(e)) } finally { setBusy(false) }
  }
  return (
    // The one item in the bar that gives way when it is narrow, so the clock stays in view.
    <label className="flex min-w-0 shrink items-center gap-1.5"
      title={`the live profile, from ${pl.live_source}: new turns run on it unless they name their own, and it persists across restarts${past ? ' · return to LIVE in the ship’s log to change it' : ''}`}>
      <LiveDot tone="model" pulse={false} size={5} />
      <span className="shrink-0 text-[10px] font-semibold uppercase tracking-wider text-ink-faint">live</span>
      <select value={pl.live} disabled={busy || past} onChange={(e) => void use(e.target.value)}
        className="num min-w-0 max-w-48 shrink cursor-pointer truncate rounded bg-transparent px-0.5 py-0.5 text-[12px] text-ink outline-none ring-1 ring-transparent hover:ring-line focus:ring-live/40 disabled:cursor-default disabled:opacity-60">
        {pl.profiles.map((p) => <option key={p.name} value={p.name} className="bg-deck text-ink">{p.name} · {p.provider}/{p.model}</option>)}
      </select>
    </label>
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

function Indicator({ label, value, tone, title }: { label: string; value: string; tone: keyof typeof toneHex; title?: string }) {
  return (
    <div className="flex shrink-0 items-center gap-1.5" title={title}>
      <LiveDot tone={tone} pulse={tone === 'live' || tone === 'ok'} size={5} />
      <span className="text-[10px] font-semibold uppercase tracking-wider text-ink-faint">{label}</span>
      <span className="num text-[12px] text-ink">{value}</span>
    </div>
  )
}

/** The web UI's door: a fault when any local user is served (no owner check), a wait while a dev page is let in,
 *  after a refusal, or on a build with no owner check; otherwise ok. A build with the check (theseus-3qf) always
 *  reports `refused_peer`. */
function webTone(web: Health['web'] | undefined): keyof typeof toneHex {
  if (!web) return 'idle'
  if (web.peer_unchecked) return 'fault'
  if (web.refused_peer === undefined || web.dev_origin || web.refused_host + web.refused_origin + web.refused_peer > 0) return 'wait'
  return 'ok'
}

function webTitle(web: Health['web'] | undefined): string {
  if (!web) return 'web UI: not reported'
  const parts = [`web UI refused ${web.refused_host} by address, ${web.refused_origin} by page, ${web.refused_peer ?? 0} by user`]
  if (web.refused_peer === undefined) parts.push('no owner check in this build: any local user is served')
  if (web.peer_unchecked) parts.push(`owner check off: ${web.peer_unchecked}`)
  if (web.dev_origin) parts.push(`dev origin open: ${web.dev_origin} (${web.dev_origin_served ?? 0} served)`)
  return parts.join(' · ')
}

/** A system state as a labeled dot; the details are in its tooltip. */
function Dot({ label, tone, title }: { label: string; tone: keyof typeof toneHex; title: string }) {
  return (
    <span className="flex items-center gap-1" title={title}>
      <LiveDot tone={tone} pulse={false} size={6} />
      <span className="text-[10px] font-medium text-ink-faint">{label}</span>
    </span>
  )
}

interface RiverLine { key: string; at: number; part: string; tone: keyof typeof toneHex; session: string | null; text: string }

function ActivityRiver({ open, onToggle }: { open: boolean; onToggle: () => void }) {
  const nav = useNavigate()
  // On a session's deck the river can narrow to that session, as the Observatory's Narrative 'this session' did.
  const here = useMatch('/session/:id')?.params.id ?? null
  const [onlyHere, setOnlyHere] = useState(false)
  const narrative = usePush((s) => s.narrative)
  const on = usePush((s) => s.narrativeOn)
  const { data: ledger } = useLedger(120, 2500)
  // The narrative tells this process's story in sentences; the ledger is the durable record. Merge them, newest
  // first, so the river has history after a restart and prose while turns run.
  const lines = useMemo<RiverLine[]>(() => {
    const out: RiverLine[] = narrative.slice(-80).map((l) => ({
      key: `n${l.seq}`, at: l.at_unix_ms, part: l.part, tone: partTone[l.part] ?? 'idle', session: l.session_id ?? null, text: l.text,
    }))
    for (const r of ledger?.rows ?? []) {
      if (r.kind === 'hook.site') continue
      out.push({ key: `l${r.position}`, at: r.at_unix_ms, part: r.kind, tone: ledgerKind(r.kind).tone, session: r.session_id, text: summarize(r) })
    }
    return out.filter((l) => !(onlyHere && here) || l.session === here).sort((a, b) => b.at - a.at).slice(0, 120)
  }, [narrative, ledger, onlyHere, here])
  return (
    <section className={cn('shrink-0 border-t border-line bg-deck/80 backdrop-blur transition-[height]', open ? 'h-44' : 'h-8')}>
      <div className="flex h-8 w-full items-center gap-2 px-4">
        <button onClick={onToggle} className="flex h-8 min-w-0 flex-1 items-center gap-2 text-left">
          <Activity size={13} className="text-live" />
          <span className="panel-title">Activity</span>
          <span className="text-[11px] text-ink-faint">
            {on === false ? 'ledger only (the narrative is off in this daemon’s config)' : `narrative ${narrative.length} · ledger ${ledger?.total ?? 0}`}
          </span>
          <span className="ml-1"><LiveDot tone="live" size={5} /></span>
          <span className="ml-auto text-[11px] text-ink-faint">{open ? 'hide' : 'show'}</span>
        </button>
        {here && (
          <label className="flex shrink-0 cursor-pointer items-center gap-1 text-[11px] text-ink-faint hover:text-ink" title="only the session open here">
            <input type="checkbox" checked={onlyHere} onChange={(e) => setOnlyHere(e.target.checked)} /> this session
          </label>
        )}
      </div>
      {open && (
        <div className="h-36 overflow-auto px-4 pb-2">
          {/* New lines fade in; no layout animation, which overlapped rows when many arrived at once. */}
          <div>
            {lines.map((l) => (
              <motion.div
                key={l.key}
                initial={{ opacity: 0, backgroundColor: 'rgba(34,211,238,0.14)' }}
                animate={{ opacity: 1, backgroundColor: 'rgba(34,211,238,0)' }}
                transition={{ duration: 0.6 }}
                className="flex items-baseline gap-2 rounded px-1 py-[1px] text-[12px]"
              >
                <span className="num shrink-0 text-[11px] text-ink-faint">{stamp(l.at)}</span>
                <span className="num w-44 shrink-0 truncate text-[10.5px] font-medium" style={{ color: toneHex[l.tone] }}>{l.part}</span>
                {l.session && <button onClick={() => nav(`/session/${l.session}`)} title={`open session ${l.session}`} className="num shrink-0 text-[11px] text-ink-faint hover:text-live">{short(l.session)}</button>}
                <span className="min-w-0 truncate text-ink-dim">{l.text}</span>
              </motion.div>
            ))}
          </div>
        </div>
      )}
    </section>
  )
}

function Palette({ open, onOpenChange }: { open: boolean; onOpenChange: (v: boolean) => void }) {
  const nav = useNavigate()
  const { data } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 5000)
  const { data: cl } = useRpc<{ confirms: ConfirmRequest[] }>('confirm.list', undefined, 3000)
  const { data: el } = useRpc<{ executions: ExecutionInfo[] }>('execution.list', undefined, 3000)
  // The calls to fly to: read only while the palette is open.
  const { data: tc } = useRpc<{ nodes: NodeInfo[] }>('node.list', { session_id: null, kind: 'tool_call', n: 300 }, 0, { enabled: open })
  const go = (to: string) => { onOpenChange(false); nav(to) }
  const act = async (ask: string, method: string, params: unknown) => {
    onOpenChange(false)
    if (!window.confirm(ask)) return
    try { await call(method, params) } catch (e: any) { window.alert(e?.message ?? String(e)) }
  }
  const title = (sid: string) => { const s = data?.sessions.find((x) => x.session_id === sid); return s?.title || s?.label || short(sid) }
  const item = 'flex cursor-pointer items-center gap-2.5 rounded-lg px-2.5 py-2 text-[13px] text-ink data-[selected=true]:bg-live/10'
  const group = 'text-[11px] text-ink-faint [&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5'
  const running = (el?.executions ?? []).filter((e) => e.state === 'running' || e.state === 'queued')
  const held = (data?.sessions ?? []).filter((s) => s.external_text)
  return (
    <Command.Dialog
      open={open}
      onOpenChange={onOpenChange}
      label="Command palette"
      className="brass-card fixed left-1/2 top-[18%] z-50 w-[680px] -translate-x-1/2 overflow-hidden !p-0 shadow-2xl"
      overlayClassName="fixed inset-0 z-40 bg-black/50 backdrop-blur-sm"
    >
      <Command.Input
        placeholder="Jump to a view, fly to a session or a call, act…"
        className="w-full border-b border-line bg-transparent px-4 py-3.5 text-[15px] text-ink outline-none placeholder:text-ink-faint"
      />
      <Command.List className="max-h-[420px] overflow-auto p-2">
        <Command.Empty className="px-3 py-6 text-center text-sm text-ink-faint">Nothing matches.</Command.Empty>
        <Command.Group heading="Views" className="text-[11px] text-ink-faint [&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5">
          {NAV.map((n) => (
            <Command.Item key={n.to} onSelect={() => go(n.to)} className="flex cursor-pointer items-center gap-2.5 rounded-lg px-2.5 py-2 text-[13px] text-ink data-[selected=true]:bg-live/10 data-[selected=true]:text-live">
              <n.icon size={15} /> {n.label}
            </Command.Item>
          ))}
        </Command.Group>
        {(cl?.confirms.length ?? 0) > 0 && (
          <Command.Group heading="Waiting for you" className={group}>
            {cl!.confirms.map((c) => [
              <Command.Item key={`y-${c.correlation_id}`} value={`approve ${c.tool} ${title(c.session_id)}`} className={item}
                onSelect={() => act(`Approve ${c.tool} for ${title(c.session_id)}?\n\n${c.reason}`, 'action.confirm', { correlation_id: c.correlation_id, approve: true })}>
                <CircleCheck size={14} className="text-ok" /> Approve <span className="num text-tool">{c.tool}</span><span className="truncate text-ink-faint">· {title(c.session_id)}</span>
              </Command.Item>,
              <Command.Item key={`n-${c.correlation_id}`} value={`decline ${c.tool} ${title(c.session_id)}`} className={item}
                onSelect={() => act(`Decline ${c.tool} for ${title(c.session_id)}?`, 'action.confirm', { correlation_id: c.correlation_id, approve: false })}>
                <OctagonX size={14} className="text-fault" /> Decline <span className="num text-tool">{c.tool}</span><span className="truncate text-ink-faint">· {title(c.session_id)}</span>
              </Command.Item>,
            ])}
          </Command.Group>
        )}
        {(running.length > 0 || held.length > 0) && (
          <Command.Group heading="Act" className={group}>
            {running.map((e) => (
              <Command.Item key={`s-${e.execution_id}`} value={`stop ${title(e.session_id)}`} className={item}
                onSelect={() => act(`Stop the running turn of ${title(e.session_id)}?`, 'execution.stop', { execution_id: e.execution_id })}>
                <Pause size={14} className="text-wait" /> Stop <span className="truncate">{title(e.session_id)}</span>
              </Command.Item>
            ))}
            {held.map((s) => (
              <Command.Item key={`t-${s.session_id}`} value={`trust ${title(s.session_id)}`} className={item}
                onSelect={() => act(`Trust ${title(s.session_id)} again? Its calls that act stop waiting.`, 'policy.trust', { session_id: s.session_id })}>
                <ShieldCheck size={14} className="text-wait" /> Trust <span className="truncate">{title(s.session_id)}</span>
              </Command.Item>
            ))}
          </Command.Group>
        )}
        <Command.Group heading="Fly to a session" className={group}>
          {(data?.sessions ?? []).map((s) => (
            <Command.Item key={`f-${s.session_id}`} value={`fly ${s.title ?? ''} ${s.label ?? ''} ${s.session_id}`} onSelect={() => go(`/ship?fly=${s.session_id}`)} className={item}>
              <Navigation size={14} className="shrink-0 text-gold" />
              <span className="truncate">{s.title || s.label || 'untitled'}</span>
              <span className="num ml-auto text-[11px] text-ink-faint">{s.kind === 'task' ? 'task · ' : ''}{short(s.session_id)}</span>
            </Command.Item>
          ))}
        </Command.Group>
        {(tc?.nodes.length ?? 0) > 0 && (
          <Command.Group heading="Fly to a call" className={group}>
            {tc!.nodes.slice(0, 150).map((n) => {
              const d = (n.detail ?? {}) as { tool?: string; plan?: { summary?: string } }
              return (
                <Command.Item key={`c-${n.node_id}`} value={`fly call ${d.tool ?? ''} ${d.plan?.summary ?? ''} ${title(n.session_id)} ${n.node_id}`} onSelect={() => go(`/ship?fly=${n.node_id}`)} className={item}>
                  <Crosshair size={14} className="shrink-0 text-live" />
                  <span className="num shrink-0 text-tool">{d.tool ?? 'tool'}</span>
                  <span className="min-w-0 truncate text-ink-dim">{d.plan?.summary ?? ''}</span>
                  <span className="num ml-auto shrink-0 text-[11px] text-ink-faint">{title(n.session_id).slice(0, 24)} · {stamp(n.at_unix_ms)}</span>
                </Command.Item>
              )
            })}
          </Command.Group>
        )}
        <Command.Group heading="Sessions" className="text-[11px] text-ink-faint [&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5">
          {(data?.sessions ?? []).map((s) => (
            <Command.Item
              key={s.session_id}
              value={`${s.title ?? ''} ${s.label ?? ''} ${s.session_id}`}
              onSelect={() => go(`/session/${s.session_id}`)}
              className="flex cursor-pointer items-center gap-2.5 rounded-lg px-2.5 py-2 text-[13px] text-ink data-[selected=true]:bg-live/10"
            >
              <Radio size={14} style={{ color: toneHex[stateTone(s.execution_state)] }} />
              <span className="truncate">{s.title || s.label || 'untitled'}</span>
              <span className="num ml-auto text-[11px] text-ink-faint">{short(s.session_id)} · {s.turns} turns · {usd(s.cost_usd)}</span>
            </Command.Item>
          ))}
        </Command.Group>
      </Command.List>
      <div className="flex items-center gap-3 border-t border-line px-4 py-2 text-[11px] text-ink-faint">
        <span><span className="kbd">↑↓</span> move</span><span><span className="kbd">↵</span> open</span><span><span className="kbd">esc</span> close</span>
        <span className="ml-auto"><span className="kbd">g</span> then {Object.keys(GO).map((k) => <span key={k} className="kbd mr-0.5">{k}</span>)} jumps to a view</span>
      </div>
    </Command.Dialog>
  )
}
