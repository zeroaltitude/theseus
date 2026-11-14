// The simple pages' few shared pieces: a page's heading, its tabs (kept in the address), and a stat.
// Cards are the cockpit's own `Panel` (`components/ui.tsx`), which carries the "?" of every pane.
import type { ReactNode } from 'react'
import { useSearchParams } from 'react-router'
import { cn } from '@/lib/format'

/** A page: its name, one sentence of what it is for, and its own controls at the right. */
export function Page({ title, sub, actions, children, fill }: { title: string; sub: string; actions?: ReactNode; children: ReactNode; /** the page fills the window and scrolls inside itself */ fill?: boolean }) {
  return (
    <div className={cn('mx-auto flex w-full max-w-[1680px] flex-col gap-4 px-6 py-5', fill && 'h-full min-h-[560px]')}>
      <div className="flex flex-wrap items-end gap-x-4 gap-y-2">
        <div className="min-w-0">
          <h1 className="simple-h1">{title}</h1>
          <p className="simple-sub">{sub}</p>
        </div>
        {actions && <div className="ml-auto flex items-center gap-2">{actions}</div>}
      </div>
      {children}
    </div>
  )
}

export interface TabDef<T extends string> { id: T; label: string; hint?: string }

/** The tab a page is on, kept in the address as `?v=`, so a link reopens it. The first tab has no query. */
export function useTab<T extends string>(tabs: readonly TabDef<T>[]): [T, (t: T) => void] {
  const [params, setParams] = useSearchParams()
  const asked = params.get('v')
  const tab = tabs.find((t) => t.id === asked)?.id ?? tabs[0].id
  // A change of tab starts that tab clean: the last tab's own query does not follow it.
  const set = (t: T) => setParams(t === tabs[0].id ? {} : { v: t })
  return [tab, set]
}

export function Tabs<T extends string>({ tabs, value, onChange, label }: { tabs: readonly TabDef<T>[]; value: T; onChange: (t: T) => void; label: string }) {
  return (
    <div role="tablist" aria-label={label} className="flex flex-wrap items-center gap-1">
      {tabs.map((t) => (
        <button key={t.id} type="button" role="tab" aria-selected={t.id === value} title={t.hint} onClick={() => onChange(t.id)} className="simple-tab">{t.label}</button>
      ))}
    </div>
  )
}

/** One number that says where things stand; a button when it leads somewhere. */
export function Stat({ n, label, tone, onClick, title }: { n: ReactNode; label: string; tone?: 'wait' | 'live' | 'ok' | 'fault' | 'money'; onClick?: () => void; title?: string }) {
  const body = (
    <>
      <span className="simple-stat-n" style={tone ? { color: `var(--color-${tone})` } : undefined}>{n}</span>
      <span className="simple-stat-l">{label}</span>
    </>
  )
  return onClick
    ? <button type="button" onClick={onClick} title={title} className="simple-stat">{body}</button>
    : <div title={title} className="simple-stat">{body}</div>
}
