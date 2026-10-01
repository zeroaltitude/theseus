// The cockpit's frame: the nav rail, the heartbeat bar across the top, the view, and the activity river below.
import { useEffect, useMemo, useRef, useState } from 'react'
import { NavLink, Outlet, useNavigate } from 'react-router'
import { motion } from 'motion/react'
import { Command } from 'cmdk'
import {
  Activity, ArrowUpRight, BellOff, BellRing, CircleCheck, Coins, Command as CommandIcon, Cpu, Gauge, Layers, OctagonX, Pause, Radio,
  ScrollText, ShieldCheck, Ship,
} from 'lucide-react'
import type { ConfirmRequest, ExecutionInfo, Health, SessionInfo } from '@protocol'
import { call, useConn, useRpc, usePush } from '@/lib/rpc'
import { useLedger } from '@/lib/derive'
import { summarize } from '@/lib/summary'
import { cn, ms, short, tokens, uptime, usd, clock, stamp } from '@/lib/format'
import { ledgerKind, partTone, stateTone, toneHex } from '@/lib/taxonomy'
import { LiveDot, Spark } from './ui'
import { useHistory, useTick } from '@/lib/hooks'

const NAV = [
  { to: '/', label: 'Bridge', icon: Gauge, end: true },
  { to: '/fleet', label: 'Fleet', icon: Layers },
  { to: '/actions', label: 'Actions', icon: ShieldCheck },
  { to: '/ledger', label: 'Ledger', icon: ScrollText },
  { to: '/economics', label: 'Economics', icon: Coins },
  { to: '/systems', label: 'Systems', icon: Cpu },
] as const

const GO: Record<string, string> = { b: '/', f: '/fleet', a: '/actions', l: '/ledger', e: '/economics', s: '/systems' }

export function Shell() {
  const nav = useNavigate()
  const [palette, setPalette] = useState(false)
  const [river, setRiver] = useState(true)
  useEffect(() => {
    // Ctrl/Cmd+K opens the palette; "g" then a letter jumps to a view (g b, g f, g a, g l, g e, g s), unless
    // you are typing in a field.
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
        <main className="min-h-0 flex-1 overflow-auto px-4 pb-4 pt-3">
          <Outlet />
        </main>
        <ActivityRiver open={river} onToggle={() => setRiver((v) => !v)} />
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
    <nav className="flex w-[68px] shrink-0 flex-col items-center gap-1 border-r border-line bg-deck/80 py-3">
      <div className="mb-3 flex flex-col items-center">
        <div className="relative grid h-9 w-9 place-items-center rounded-xl bg-gradient-to-br from-live/25 to-model/25 ring-1 ring-live/30">
          <Ship size={18} className="text-live" />
          <span className="absolute -right-0.5 -top-0.5"><LiveDot tone={status === 'open' ? 'ok' : status === 'connecting' ? 'wait' : 'fault'} size={7} /></span>
        </div>
      </div>
      {NAV.map(({ to, label, icon: Icon, ...rest }) => (
        <NavLink
          key={to}
          to={to}
          end={'end' in rest}
          className={({ isActive }) => cn(
            'group relative flex w-14 flex-col items-center gap-0.5 rounded-lg py-2 text-[10px] font-medium transition-colors',
            isActive ? 'text-live' : 'text-ink-faint hover:bg-white/5 hover:text-ink',
          )}
        >
          {({ isActive }) => (
            <>
              {isActive && (
                <motion.span layoutId="nav-active" className="absolute inset-0 rounded-lg bg-live/10 ring-1 ring-live/25" transition={{ type: 'spring', stiffness: 400, damping: 32 }} />
              )}
              <Icon size={18} className="relative" />
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
        <a href="/" title="The classic Observatory" className="rounded-lg p-2 text-ink-faint hover:bg-white/5 hover:text-ink">
          <ArrowUpRight size={17} />
        </a>
      </div>
    </nav>
  )
}

function HeartbeatBar() {
  const { data: h, dataUpdatedAt } = useRpc<Health>('health', undefined, 2000)
  const conn = useConn()
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
  const running = h?.kernel.executions_by_state.running ?? 0
  const discord = h?.bindings?.find((b) => b.kind === 'discord')
  const usage = h?.usage_total
  const cacheHit = usage ? usage.cache_read_input_tokens / Math.max(1, usage.cache_read_input_tokens + usage.input_tokens + usage.cache_creation_input_tokens) : 0

  return (
    <header className="relative flex h-12 shrink-0 items-center gap-4 overflow-hidden whitespace-nowrap border-b border-line bg-deck/70 px-4 backdrop-blur">
      <div className="absolute inset-x-0 top-0 h-px live-sweep" />
      <div className="flex shrink-0 items-baseline gap-2" title={h ? `theseus ${h.version} · protocol ${h.protocol}` : undefined}>
        <span className="text-[13px] font-semibold tracking-[0.2em] text-ink">THESEUS</span>
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
      <Indicator label="model" tone="model" value={h ? h.model : '—'} title={h ? `${h.provider} · ${h.model} · profile ${h.profile}` : undefined} />
      <div className="ml-auto flex shrink-0 items-center gap-4">
        <div className="flex items-center gap-2.5 rounded-md bg-white/[0.03] px-2 py-1 ring-1 ring-line">
          <Dot label="kernel" tone={h?.kernel.accepting ? 'ok' : 'wait'} title={h ? (h.kernel.accepting ? 'kernel accepting' : 'kernel holding new turns') : ''} />
          <Dot label="discord" tone={stateTone(discord?.state)} title={discord ? `Discord ${discord.state}${discord.latency_ms ? ` · ${discord.latency_ms} ms` : ''}${discord.detail ? ` · ${discord.detail}` : ''}` : 'Discord'} />
          <Dot label="config" tone={stateTone(h?.config?.state)} title={`config ${h?.config?.state ?? '—'} (${h?.config?.source ?? '—'})`} />
          <Dot label="secrets" tone={stateTone(h?.secrets?.state)} title={`secrets ${h?.secrets?.state ?? '—'}`} />
          <Dot label="web" tone={webTone(h?.web)} title={webTitle(h?.web)} />
        </div>
        <Indicator label="cache" tone="think" value={usage ? `${(cacheHit * 100).toFixed(0)}%` : '—'} title={usage ? `${tokens(usage.cache_read_input_tokens)} input tokens read from cache` : undefined} />
        <Indicator label="spent" tone="money" value={h?.cost_usd_total !== undefined ? usd(h.cost_usd_total) : '—'} />
        <span className="num text-[12px] text-ink-dim">{clock(now)}</span>
      </div>
    </header>
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
function webTone(web: Health['web']): keyof typeof toneHex {
  if (!web) return 'idle'
  if (web.peer_unchecked) return 'fault'
  if (web.refused_peer === undefined || web.dev_origin || web.refused_host + web.refused_origin + web.refused_peer > 0) return 'wait'
  return 'ok'
}

function webTitle(web: Health['web']): string {
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
    return out.sort((a, b) => b.at - a.at).slice(0, 120)
  }, [narrative, ledger])
  return (
    <section className={cn('shrink-0 border-t border-line bg-deck/80 backdrop-blur transition-[height]', open ? 'h-44' : 'h-8')}>
      <button onClick={onToggle} className="flex h-8 w-full items-center gap-2 px-4 text-left">
        <Activity size={13} className="text-live" />
        <span className="panel-title">Activity</span>
        <span className="text-[11px] text-ink-faint">
          {on === false ? 'ledger only (the narrative is off in this daemon’s config)' : `narrative ${narrative.length} · ledger ${ledger?.total ?? 0}`}
        </span>
        <span className="ml-1"><LiveDot tone="live" size={5} /></span>
        <span className="ml-auto text-[11px] text-ink-faint">{open ? 'hide' : 'show'}</span>
      </button>
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
                {l.session && <span className="num shrink-0 text-[11px] text-ink-faint">{short(l.session)}</span>}
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
      className="fixed left-1/2 top-[18%] z-50 w-[640px] -translate-x-1/2 overflow-hidden rounded-2xl border border-line-strong bg-hull/95 shadow-2xl backdrop-blur-xl"
      overlayClassName="fixed inset-0 z-40 bg-black/50 backdrop-blur-sm"
    >
      <Command.Input
        placeholder="Jump to a view, a session, an execution…"
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
        <span className="ml-auto"><span className="kbd">g</span> then <span className="kbd">b</span> <span className="kbd">f</span> <span className="kbd">a</span> <span className="kbd">l</span> <span className="kbd">e</span> <span className="kbd">s</span> jumps to a view</span>
      </div>
    </Command.Dialog>
  )
}
