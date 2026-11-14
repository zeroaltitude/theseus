// The simple pages' frame: one bar across the top (the Theseus mark, the five sections, the creative takes, the look)
// and the page under it. It is the cockpit's front door: `/` is Ops here, and the Ship and its pages are one link
// away under "Creative takes", in the brass frame they have always had (`components/Shell.tsx`).
//
// While it is on screen <html> wears `simple` (`simple.css`), so everything drawn inside it, and every popover and
// sheet drawn at the end of <body>, takes the plain look. It shows the present only: the time machine lives in the
// Ship's frame, so arriving here returns to now.
import { Suspense, useEffect, useLayoutEffect } from 'react'
import { Link, NavLink, Outlet } from 'react-router'
import { Activity, Brain, FlaskConical, ListChecks, Moon, Sailboat, SlidersHorizontal, Sun, SunMoon } from 'lucide-react'
import type { ConfirmRequest } from '@protocol'
import { useConn, useRpc } from '@/lib/rpc'
import { useMode, type ModeChoice } from '@/lib/mode'
import { nextChoice } from '@/lib/daylight'
import { useAsOf } from '@/lib/timemachine'
import { Coin } from '@/components/brass'
import { LiveDot } from '@/components/ui'
import './simple.css'

/** The five sections, in the order an operator reaches for them. */
const SECTIONS = [
  { to: '/', label: 'Ops', icon: ListChecks, end: true },
  { to: '/bench', label: 'Benchmarks', icon: FlaskConical, end: false },
  { to: '/telemetry', label: 'Telemetry', icon: Activity, end: false },
  { to: '/config', label: 'Configuration', icon: SlidersHorizontal, end: false },
  { to: '/memories', label: 'Memories', icon: Brain, end: false },
] as const

const LINK_WORDS: Record<string, string> = { open: 'connected to Theseus', connecting: 'connecting to Theseus…', closed: 'not connected to Theseus' }

export function SimpleShell() {
  useLayoutEffect(() => {
    const html = document.documentElement
    html.classList.add('simple')
    return () => html.classList.remove('simple')
  }, [])
  // These pages show the present: a moment left set on the Ship's time machine would show them the past unmarked.
  useEffect(() => { useAsOf.getState().set(null) }, [])
  const status = useConn((s) => s.status)
  // What waits on the owner, on every page: the Ops link carries the count.
  const { data: cl } = useRpc<{ confirms: ConfirmRequest[] }>('confirm.list', undefined, 2000)
  const waiting = cl?.confirms.length ?? 0
  return (
    <div className="simple-page flex h-full flex-col">
      <header className="simple-bar relative z-20 flex h-14 shrink-0 items-center gap-6 px-5">
        <Link to="/" title="Theseus: back to the start" className="flex shrink-0 items-center gap-2.5">
          <Coin size={32} />
          <span className="simple-wordmark">THESEUS</span>
        </Link>
        <nav aria-label="Sections" className="flex min-w-0 items-center gap-5 overflow-x-auto [scrollbar-width:none]">
          {SECTIONS.map(({ to, label, icon: Icon, end }) => (
            <NavLink key={to} to={to} end={end} className="simple-nav-link shrink-0">
              <Icon size={15} aria-hidden />
              {label}
              {to === '/' && waiting > 0 && (
                <span className="num grid h-[18px] min-w-[18px] place-items-center rounded-full bg-wait/20 px-1 text-[11px] font-semibold text-wait" title={`${waiting} ${waiting === 1 ? 'thing waits' : 'things wait'} for you`}>{waiting}</span>
              )}
            </NavLink>
          ))}
        </nav>
        <div className="ml-auto flex shrink-0 items-center gap-3">
          <span className="simple-eyebrow hidden lg:inline">Creative takes</span>
          <Link to="/ship" className="simple-chip" title="The Ship: the cockpit as a sea chart, with every instrument page behind it">
            <Sailboat size={14} aria-hidden /> The Ship
          </Link>
          <span className="flex items-center" title={LINK_WORDS[status] ?? status} role="status" aria-label={LINK_WORDS[status] ?? status}>
            <LiveDot tone={status === 'open' ? 'ok' : status === 'connecting' ? 'wait' : 'fault'} pulse={status !== 'open'} size={8} />
          </span>
          <LookToggle />
        </div>
      </header>
      <main className="min-h-0 flex-1 overflow-auto">
        <Suspense fallback={<div className="p-8 text-[13px] text-ink-faint">Loading…</div>}>
          <Outlet />
        </Suspense>
      </main>
    </div>
  )
}

const LOOK: Record<ModeChoice, { Icon: typeof Sun; name: string }> = {
  light: { Icon: Sun, name: 'Light' },
  dark: { Icon: Moon, name: 'Dark' },
  system: { Icon: SunMoon, name: 'Follow the system' },
}

/** Light, dark, or the system's (`lib/mode.ts`): one button that steps through them; the choice is kept in this browser. */
function LookToggle() {
  const choice = useMode((s) => s.choice)
  const setChoice = useMode((s) => s.setChoice)
  const now = LOOK[choice]
  const next = LOOK[nextChoice(choice)]
  return (
    <button type="button" onClick={() => setChoice(nextChoice(choice))} aria-label={`The look: ${now.name}. Press for ${next.name.toLowerCase()}.`}
      title={`The look: ${now.name}. Press for ${next.name.toLowerCase()}.`}
      className="grid h-8 w-8 place-items-center rounded-lg text-ink-faint transition-colors hover:bg-white/[0.06] hover:text-ink">
      <now.Icon size={16} aria-hidden />
    </button>
  )
}
