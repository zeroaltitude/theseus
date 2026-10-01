// The cockpit's frame: the nav rail, the heartbeat bar across the top, the view, and the activity river below.
import { useEffect, useMemo, useState } from 'react'
import { NavLink, Outlet, useNavigate } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { Command } from 'cmdk'
import {
  Activity, ArrowUpRight, Coins, Command as CommandIcon, Cpu, Gauge, Layers, Radio, ScrollText, ShieldCheck, Ship,
} from 'lucide-react'
import type { Health, SessionInfo } from '@protocol'
import { useConn, useRpc, usePush } from '@/lib/rpc'
import { useLedger } from '@/lib/derive'
import { summarize } from '@/lib/summary'
import { cn, ms, short, tokens, uptime, usd, clock, stamp } from '@/lib/format'
import { ledgerKind, partTone, stateTone, toneHex } from '@/lib/taxonomy'
import { LiveDot } from './ui'
import { useTick } from '@/lib/hooks'

const NAV = [
  { to: '/', label: 'Bridge', icon: Gauge, end: true },
  { to: '/fleet', label: 'Fleet', icon: Layers },
  { to: '/actions', label: 'Actions', icon: ShieldCheck },
  { to: '/ledger', label: 'Ledger', icon: ScrollText },
  { to: '/economics', label: 'Economics', icon: Coins },
  { to: '/systems', label: 'Systems', icon: Cpu },
] as const

export function Shell() {
  const [palette, setPalette] = useState(false)
  const [river, setRiver] = useState(true)
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') { e.preventDefault(); setPalette((v) => !v) }
    }
    window.addEventListener('keydown', k)
    return () => window.removeEventListener('keydown', k)
  }, [])

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

function NavRail({ onPalette }: { onPalette: () => void }) {
  const status = useConn((s) => s.status)
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
            </>
          )}
        </NavLink>
      ))}
      <div className="mt-auto flex flex-col items-center gap-2">
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
  const discord = h?.bindings?.find((b) => b.kind === 'discord')
  const usage = h?.usage_total
  const cacheHit = usage ? usage.cache_read_input_tokens / Math.max(1, usage.cache_read_input_tokens + usage.input_tokens + usage.cache_creation_input_tokens) : 0

  return (
    <header className="relative flex h-12 shrink-0 items-center gap-5 overflow-hidden border-b border-line bg-deck/70 px-4 backdrop-blur">
      <div className="absolute inset-x-0 top-0 h-px live-sweep" />
      <div className="flex items-baseline gap-2">
        <span className="text-[13px] font-semibold tracking-[0.2em] text-ink">THESEUS</span>
        <span className="num text-[11px] text-ink-faint">{h ? `v${h.version} · proto ${h.protocol}` : '…'}</span>
      </div>
      <Indicator label="link" tone={conn.status === 'open' ? 'ok' : conn.status === 'connecting' ? 'wait' : 'fault'} value={conn.status === 'open' ? (rtt !== null ? ms(rtt) : 'open') : conn.status} />
      <Indicator label="up" tone="live" value={h ? uptime(up) : '—'} />
      <Indicator label="kernel" tone={h?.kernel.accepting ? 'ok' : 'wait'} value={h ? (h.kernel.accepting ? 'accepting' : 'held') : '—'} />
      <Indicator label="model" tone="model" value={h ? `${h.provider} · ${h.model}` : '—'} />
      <Indicator label="discord" tone={stateTone(discord?.state)} value={discord ? `${discord.state}${discord.latency_ms ? ` · ${discord.latency_ms} ms` : ''}` : '—'} />
      <Indicator label="config" tone={stateTone(h?.config?.state)} value={h?.config?.state ?? '—'} />
      <Indicator label="secrets" tone={stateTone(h?.secrets?.state)} value={h?.secrets?.state ?? '—'} />
      <div className="ml-auto flex items-center gap-5">
        <Indicator label="cache" tone="think" value={usage ? `${(cacheHit * 100).toFixed(0)}% · ${tokens(usage.cache_read_input_tokens)}` : '—'} />
        <Indicator label="spent" tone="money" value={h?.cost_usd_total !== undefined ? usd(h.cost_usd_total) : '—'} />
        <span className="num text-[12px] text-ink-dim">{clock(now)}</span>
      </div>
    </header>
  )
}

function Indicator({ label, value, tone }: { label: string; value: string; tone: keyof typeof toneHex }) {
  return (
    <div className="flex min-w-0 items-center gap-1.5">
      <LiveDot tone={tone} pulse={tone === 'live' || tone === 'ok'} size={5} />
      <span className="text-[10px] font-semibold uppercase tracking-wider text-ink-faint">{label}</span>
      <span className="num truncate text-[12px] text-ink">{value}</span>
    </div>
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
          <AnimatePresence initial={false}>
            {lines.map((l) => (
              <motion.div
                key={l.key}
                layout
                initial={{ opacity: 0, x: -12, backgroundColor: 'rgba(34,211,238,0.12)' }}
                animate={{ opacity: 1, x: 0, backgroundColor: 'rgba(34,211,238,0)' }}
                transition={{ duration: 0.5 }}
                className="flex items-baseline gap-2 rounded px-1 py-[1px] text-[12px]"
              >
                <span className="num shrink-0 text-[11px] text-ink-faint">{stamp(l.at)}</span>
                <span className="num w-44 shrink-0 truncate text-[10.5px] font-medium" style={{ color: toneHex[l.tone] }}>{l.part}</span>
                {l.session && <span className="num shrink-0 text-[11px] text-ink-faint">{short(l.session)}</span>}
                <span className="min-w-0 truncate text-ink-dim">{l.text}</span>
              </motion.div>
            ))}
          </AnimatePresence>
        </div>
      )}
    </section>
  )
}

function Palette({ open, onOpenChange }: { open: boolean; onOpenChange: (v: boolean) => void }) {
  const nav = useNavigate()
  const { data } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 5000)
  const go = (to: string) => { onOpenChange(false); nav(to) }
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
      </div>
    </Command.Dialog>
  )
}
