// The cockpit's frame: the nav rail, the heartbeat bar across the top, the view, and the activity river below.
import { useEffect, useMemo, useRef, useState } from 'react'
import { NavLink, Outlet, useLocation, useMatch, useNavigate } from 'react-router'
import { motion } from 'motion/react'
import { Command, defaultFilter } from 'cmdk'
import {
  Activity, BellOff, BellRing, ChevronDown, ChevronUp, CircleCheck, Moon, Sun, SunMoon, Check, Coins, Command as CommandIcon, Cpu, Crosshair, FlaskConical, Gauge, Gavel, Landmark, Layers, Library, Navigation, OctagonX, Pause, Radio,
  Sailboat, Scale, ScrollText, ShieldCheck, ShieldHalf, Shapes, Telescope, Zap,
} from 'lucide-react'
import type { ConfirmRequest, ExecutionInfo, NodeInfo, SessionInfo } from '@protocol'
import { call, client, useConn, useRpc, usePush } from '@/lib/rpc'
import { onNewRows } from '@/lib/history'
import { Stir, STIR_CLASS } from '@/lib/stir'
import { useSoundCues } from '@/ship/useShipSound'
import { useLedger } from '@/lib/derive'
import { summarize } from '@/lib/summary'
import { cn, short, usd, clock, stamp } from '@/lib/format'
import { ledgerKind, partTone, stateTone, toneHex } from '@/lib/taxonomy'
import { LiveDot, Spark } from './ui'
import { Coin } from './brass'
import { HeartbeatBar } from './Heartbeat'
import { useFlow } from '@/lib/flow'
import { useAsOf } from '@/lib/timemachine'
import { FOLDS } from '@/lib/world'
import { useCalm } from '@/lib/calm'
import { useMode, type ModeChoice } from '@/lib/mode'
import { CHOICES, nextChoice } from '@/lib/daylight'
import { foldKey, foldRepeats, STRIP_KEY, stripOpen, type Fold, type StripLine } from '@/lib/activity'
import { TimeMachine } from './TimeMachine'
import { StateBadge } from './SessionLife'
import { paletteOrder, searchText, stateOf } from '@/lib/sessionState'

const NAV = [
  { to: '/ship', label: 'Ship', icon: Sailboat },
  { to: '/bridge', label: 'Bridge', icon: Gauge },
  { to: '/fleet', label: 'Fleet', icon: Layers },
  { to: '/actions', label: 'Actions', icon: ShieldCheck },
  { to: '/boundaries', label: 'Bounds', icon: ShieldHalf },
  { to: '/policy', label: 'Policy', icon: Gavel },
  { to: '/ledger', label: 'Ledger', icon: ScrollText },
  { to: '/money', label: 'Money', icon: Landmark },
  { to: '/economics', label: 'Economics', icon: Coins },
  { to: '/speed', label: 'Speed', icon: Zap },
  { to: '/benchmarks', label: 'Bench', icon: FlaskConical },
  { to: '/judgment', label: 'Judgment', icon: Scale },
  { to: '/systems', label: 'Systems', icon: Cpu },
  { to: '/ontology', label: 'Ontology', icon: Shapes },
  { to: '/context', label: 'Context', icon: Telescope },
  { to: '/books', label: 'Books', icon: Library },
] as const

const GO: Record<string, string> = {
  h: '/ship', b: '/bridge', f: '/fleet', a: '/actions', o: '/boundaries', p: '/policy', l: '/ledger', m: '/money', e: '/economics', w: '/speed', k: '/benchmarks', j: '/judgment', s: '/systems',
}

export function Shell() {
  useStir()
  const nav = useNavigate()
  const [palette, setPalette] = useState(false)
  // The Ship is full-bleed: the main area has no margin. The index redirects to the Ship, so it counts as the Ship.
  const shipRoute = useMatch('/ship')
  const indexRoute = useMatch({ path: '/', end: true })
  const onShip = !!shipRoute || !!indexRoute
  // The sound cues play on every page, still off until the operator turns them on (the Ship's Sound button).
  useSoundCues(onShip)
  // The activity strip starts folded, on the Ship and the data pages alike, and stays as this browser left it
  // (theseus-hnof.5): open by default, it took the foot of every data view at 1080 px, and it forgot a fold.
  const [river, setRiver] = useState(() => stripOpen(kept(STRIP_KEY)))
  const toggleRiver = () => { keep(STRIP_KEY, river ? 'closed' : 'open'); setRiver(!river) }
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
        <ActivityRiver open={river} onToggle={toggleRiver} />
        <TimeMachine />
      </div>
      <Palette open={palette} onOpenChange={setPalette} />
    </div>
  )
}

/** The page's endless decorations run while the daemon says something, and stand still between (`stir.ts`,
 *  theseus-jgme): a push, a new ledger row in the page's one copy, or the link changing stirs them. */
function useStir() {
  const status = useConn((s) => s.status)
  const stir = useRef<Stir | null>(null)
  useEffect(() => {
    const s = (stir.current = new Stir(
      { timer: (cb, ms) => window.setTimeout(cb, ms), cancelTimer: (id) => window.clearTimeout(id) },
      (on) => document.documentElement.classList.toggle(STIR_CLASS, on),
    ))
    const off = client.onNotify(() => s.poke())
    const offRows = onNewRows(() => s.poke())
    return () => {
      off()
      offRows()
      s.dispose()
      stir.current = null
      document.documentElement.classList.remove(STIR_CLASS)
    }
  }, [])
  useEffect(() => { stir.current?.poke() }, [status])
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
    <nav className="brass-rail relative z-10 flex w-[68px] shrink-0 flex-col items-center gap-1 py-3 [@media(max-height:860px)]:gap-0.5 [@media(max-height:860px)]:py-2">
      <div className="mb-3 flex flex-col items-center [@media(max-height:860px)]:mb-1.5">
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
            // The items and the rail's foot fit a short screen only with less air between them, measured with fifteen items
            // down to 1366×768 (the rail held thirteen at py-1 below 860 px).
            'group relative flex w-[62px] flex-col items-center gap-0.5 rounded-lg py-2 font-display text-[8.5px] font-bold uppercase tracking-[0.03em] transition-colors [@media(max-height:1000px)]:py-1 [@media(max-height:880px)]:py-0.5 [@media(max-height:820px)]:py-0',
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
        <ModeToggle />
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

/** This browser's kept value for `key`, or null (no storage, or none kept). */
function kept(key: string): string | null {
  try { return localStorage.getItem(key) } catch { return null }
}

/** Keep `value` under `key` in this browser; with no storage, it lasts as long as the page. */
function keep(key: string, value: string) {
  try { localStorage.setItem(key, value) } catch { /* the page's state stands */ }
}

function ActivityRiver({ open, onToggle }: { open: boolean; onToggle: () => void }) {
  const nav = useNavigate()
  const calm = useCalm((s) => s.calm)
  // On a session's deck the river can narrow to that session, as the Observatory's Narrative 'this session' did.
  const here = useMatch('/session/:id')?.params.id ?? null
  const [onlyHere, setOnlyHere] = useState(false)
  const narrative = usePush((s) => s.narrative)
  const on = usePush((s) => s.narrativeOn)
  // Read only what is on screen: open, the newest 120 rows; folded, the few that make its one line.
  const { data: ledger } = useLedger(open ? 120 : 12, 2500)
  const { flow, total } = useFlow()
  // The narrative tells this process's story in sentences; the ledger is the durable record. Merge them, newest
  // first, so the river has history after a restart and prose while turns run; then fold the lines that repeat.
  const folds = useMemo<Fold[]>(() => {
    const out: StripLine[] = narrative.slice(open ? -400 : -40).map((l) => ({
      key: `n${l.seq}`, at: l.at_unix_ms, part: l.part, tone: partTone[l.part] ?? 'idle', session: l.session_id ?? null, text: l.text,
    }))
    for (const r of ledger?.rows ?? []) {
      if (r.kind === 'hook.site') continue
      out.push({ key: `l${r.position}`, at: r.at_unix_ms, part: r.kind, tone: ledgerKind(r.kind).tone, session: r.session_id, text: summarize(r) })
    }
    const shown = out.filter((l) => !(onlyHere && here) || l.session === here).sort((a, b) => b.at - a.at)
    return foldRepeats(shown).slice(0, 120)
  }, [narrative, ledger, onlyHere, here, open])
  // The fold laid out, by its key: a click on a folded line shows every line it holds.
  const [laid, setLaid] = useState<string | null>(null)
  const rate = flow[flow.length - 1] ?? 0
  return (
    <section className={cn('shrink-0 border-t border-line bg-deck/80 backdrop-blur transition-[height]', open ? 'h-44' : 'h-8')}>
      <div className="flex h-8 w-full items-center gap-3 px-4">
        <button onClick={onToggle} aria-expanded={open} title={open ? 'fold the activity strip' : 'open the activity strip'}
          className="flex h-8 min-w-0 flex-1 items-center gap-3 text-left">
          <span className="flex shrink-0 items-center gap-2"><Activity size={13} className="text-live" /><span className="panel-title">Activity</span></span>
          {/* Folded, the bar still says the newest thing that happened, folded as the list folds it. */}
          <span className="min-w-0 flex-1 truncate">{!open && folds[0] && <FoldText f={folds[0]} />}</span>
          <span className="flex shrink-0 items-center gap-1.5" title="flow: ledger rows a second, the last two minutes">
            <span className="text-[10px] font-semibold uppercase tracking-wider text-ink-faint">flow</span>
            <span className="block w-16"><Spark data={flow.length > 1 ? flow : [0, 0]} tone="live" height={18} /></span>
            <span className="num w-10 text-[11px] text-ink">{rate.toFixed(1)}/s</span>
          </span>
          <span className="num shrink-0 text-[11px] text-ink-faint" title="the narrative's lines in this page, and the ledger's rows">
            {on === false ? 'ledger only (the narrative is off in this daemon’s config)' : `narrative ${narrative.length}`} · ledger {total ?? ledger?.total ?? 0}
          </span>
          <span className="flex shrink-0 items-center gap-0.5 text-[11px] text-ink-faint">{open ? <>hide <ChevronDown size={12} /></> : <>show <ChevronUp size={12} /></>}</span>
        </button>
        {here && (
          <label className="flex shrink-0 cursor-pointer items-center gap-1 text-[11px] text-ink-faint hover:text-ink" title="only the session open here">
            <input type="checkbox" checked={onlyHere} onChange={(e) => setOnlyHere(e.target.checked)} /> this session
          </label>
        )}
      </div>
      {open && (
        <div className="h-36 overflow-auto px-4 pb-2">
          {/* New lines fade in; no layout animation, which overlapped rows when many arrived at once. A fold that
              grows fades in again, so a repeat is seen. */}
          {folds.map((f) => {
            const fk = foldKey(f.line)
            const isLaid = laid === fk && f.count > 1
            return (
              <div key={fk}>
                <motion.div
                  key={f.line.key}
                  initial={calm ? false : { opacity: 0, backgroundColor: 'rgba(34,211,238,0.14)' }}
                  animate={{ opacity: 1, backgroundColor: 'rgba(34,211,238,0)' }}
                  transition={{ duration: 0.6 }}
                  className={cn('flex items-baseline gap-2 rounded px-1 py-[1px] text-[12px]', f.count > 1 && 'cursor-pointer hover:bg-gold/5')}
                  onClick={f.count > 1 ? () => setLaid(isLaid ? null : fk) : undefined}
                  title={f.count > 1 ? (isLaid ? 'fold these lines again' : `${f.count} lines say this; show each`) : undefined}
                >
                  <FoldText f={f} wide onSession={(sid) => nav(`/session/${sid}`)} />
                </motion.div>
                {isLaid && (
                  <div className="mb-1 ml-6 border-l border-line pl-2">
                    {f.members.slice(0, 200).map((m) => (
                      <div key={m.key} className="flex items-baseline gap-2 py-[1px] text-[11.5px]">
                        <span className="num shrink-0 text-[11px] text-ink-faint">{stamp(m.at)}</span>
                        {m.session && <button onClick={() => nav(`/session/${m.session}`)} title={`open session ${m.session}`} className="num shrink-0 text-[11px] text-ink-faint hover:text-live">{short(m.session)}</button>}
                        <span className="min-w-0 truncate text-ink-dim">{m.text}</span>
                      </div>
                    ))}
                    {f.count > 200 && <button onClick={() => nav('/ledger')} className="text-[11px] text-live hover:underline">and {f.count - 200} more: the ledger has every one →</button>}
                  </div>
                )}
              </div>
            )
          })}
        </div>
      )}
    </section>
  )
}

/** A fold as one line: its newest line's time, kind, session and words, and, when it folds more than one, how many,
 *  since when, and from how many sessions. */
function FoldText({ f, wide, onSession }: { f: Fold; wide?: boolean; onSession?: (sid: string) => void }) {
  const l = f.line
  const one = f.sessions.length === 1 ? f.sessions[0] : null
  return (
    <span className="flex min-w-0 items-baseline gap-2 text-[12px]">
      <span className="num shrink-0 text-[11px] text-ink-faint">{stamp(l.at)}</span>
      <span className={cn('num shrink-0 truncate text-[10.5px] font-medium', wide ? 'w-44' : 'max-w-44')} style={{ color: toneHex[l.tone as keyof typeof toneHex] ?? toneHex.idle }}>{l.part}</span>
      {one && (onSession
        ? <button onClick={(e) => { e.stopPropagation(); onSession(one) }} title={`open session ${one}`} className="num shrink-0 text-[11px] text-ink-faint hover:text-live">{short(one)}</button>
        : <span className="num shrink-0 text-[11px] text-ink-faint">{short(one)}</span>)}
      {f.sessions.length > 1 && <span className="num shrink-0 text-[11px] text-ink-faint">{f.sessions.length} sessions</span>}
      <span className="min-w-0 truncate text-ink-dim">{l.text}</span>
      {f.count > 1 && (
        <span className="num shrink-0 rounded px-1 text-[10.5px] font-semibold text-gold ring-1 ring-gold/40" title={`${f.count} lines say this, the first at ${stamp(f.first)}`}>
          ×{f.count}<span className="font-normal text-ink-faint"> since {clock(f.first)}</span>
        </span>
      )}
    </span>
  )
}

/** Each choice of mode (lib/mode.ts): its icon, its name, and what it is. */
const CHOICE_WORDS: Record<ModeChoice, { Icon: typeof Sun; name: string; what: string }> = {
  dark: { Icon: Moon, name: 'Night', what: 'the bridge on navy glass' },
  light: { Icon: Sun, name: 'Daylight', what: 'ivory and brass, for a bright room' },
  system: { Icon: SunMoon, name: 'Follow the system', what: 'daylight while the system is light, night while it is dark' },
}

/** Night, daylight, or the system's (lib/mode.ts, theseus-hnof.5 and theseus-001m): the bridge on navy glass, ivory and
 *  brass for a bright room, or whichever the system is. The button steps through them and shows where a press goes;
 *  the choice is kept in this browser. */
function ModeToggle() {
  const choice = useMode((s) => s.choice)
  const mode = useMode((s) => s.mode)
  const setChoice = useMode((s) => s.setChoice)
  const now = CHOICE_WORDS[choice]
  const next = CHOICE_WORDS[nextChoice(choice)]
  const following = choice === 'system' ? ` (${mode === 'light' ? 'light' : 'dark'} now)` : ''
  return (
    <button onClick={() => setChoice(nextChoice(choice))} aria-label={`The look: ${now.name}${following}. Press for ${next.name.toLowerCase()}.`}
      title={`${now.name}${following}: ${now.what}. Press for ${next.name.toLowerCase()}: ${next.what}. The Ship and the ship’s log track stay at night.`}
      className="rounded-lg p-2 text-ink-faint hover:bg-white/5 hover:text-ink">
      <next.Icon size={17} />
    </button>
  )
}

/** More words the palette finds each choice by. */
const CHOICE_SEARCH: Record<ModeChoice, string> = { dark: 'dark', light: 'light day', system: 'auto os light dark' }

/** cmdk's own score, a retired session's at a third (theseus-emqx): it is still found, after the others. */
const retiredAfter = (value: string, search: string, keywords?: string[]) =>
  defaultFilter(value, search, keywords) * (keywords?.includes('retired') ? 1 / 3 : 1)

function Palette({ open, onOpenChange }: { open: boolean; onOpenChange: (v: boolean) => void }) {
  const nav = useNavigate()
  const choice = useMode((s) => s.choice)
  const setChoice = useMode((s) => s.setChoice)
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
  const sessions = useMemo(() => paletteOrder(data?.sessions ?? []), [data])
  return (
    <Command.Dialog
      open={open}
      onOpenChange={onOpenChange}
      filter={retiredAfter}
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
        <Command.Group heading="Look" className={group}>
          {CHOICES.map((c) => {
            const { Icon, name, what } = CHOICE_WORDS[c]
            return (
              <Command.Item key={c} value={`${name.toLowerCase()} mode ${CHOICE_SEARCH[c]}`} onSelect={() => { onOpenChange(false); setChoice(c) }} className={item}>
                <Icon size={14} className="text-gold" /> {name}
                {c === choice && <Check size={13} className="text-gold" aria-label="chosen" />}
                <span className="ml-auto text-[11px] text-ink-faint">{what}</span>
              </Command.Item>
            )
          })}
        </Command.Group>
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
        {/* Every session, whatever the Ship's filter (theseus-emqx), the retired ones after the others, each with its
            badge; a hidden one flies in as a visitor. Its old titles match too. */}
        <Command.Group heading="Fly to a session" className={group}>
          {sessions.map((s) => (
            <Command.Item key={`f-${s.session_id}`} value={`fly ${searchText(s)}`} keywords={[stateOf(s)]} onSelect={() => go(`/ship?fly=${s.session_id}`)} className={item}>
              <Navigation size={14} className="shrink-0 text-gold" />
              <span className="truncate">{s.title || s.label || 'untitled'}</span>
              {s.title_was?.length ? <span className="truncate text-[11px] text-ink-faint">was: {s.title_was[0]}</span> : null}
              <span className="ml-auto flex shrink-0 items-center gap-2"><StateBadge state={s.state} retired={s.retired} />
                <span className="num text-[11px] text-ink-faint">{s.kind === 'task' ? 'task · ' : ''}{short(s.session_id)}</span></span>
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
          {sessions.map((s) => (
            <Command.Item
              key={s.session_id}
              value={searchText(s)}
              keywords={[stateOf(s)]}
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
