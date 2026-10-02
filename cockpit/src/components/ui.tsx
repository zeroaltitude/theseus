// Small instruments shared by every view: panels, state pills, live dots, animated numbers, sparklines.
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { animate, motion } from 'motion/react'
import { cn } from '@/lib/format'
import { stateTone, toneClass, toneHex, type Tone } from '@/lib/taxonomy'
import { Echart } from './Echart'
import type { EChartsOption } from '@/lib/chart'
import type { Attention } from '@protocol'

export function Panel({
  title, icon, actions, children, className, bodyClassName,
}: {
  title?: ReactNode; icon?: ReactNode; actions?: ReactNode; children: ReactNode; className?: string; bodyClassName?: string
}) {
  return (
    <section className={cn('panel flex min-h-0 flex-col', className)}>
      {(title || actions) && (
        <header className="flex items-center gap-2 border-b border-line px-3.5 py-2">
          {icon && <span className="text-ink-faint">{icon}</span>}
          <h2 className="panel-title truncate">{title}</h2>
          <div className="ml-auto flex items-center gap-1.5">{actions}</div>
        </header>
      )}
      <div className={cn('min-h-0 flex-auto', bodyClassName)}>{children}</div>
    </section>
  )
}

export function Pill({ tone = 'idle', children, className, title }: { tone?: Tone; children: ReactNode; className?: string; title?: string }) {
  return (
    <span
      title={title}
      className={cn(
        'inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] font-medium ring-1 ring-inset whitespace-nowrap',
        toneClass[tone], className,
      )}
    >
      {children}
    </span>
  )
}

export function StatePill({ state, className }: { state: string | null | undefined; className?: string }) {
  const tone = stateTone(state)
  return (
    <Pill tone={tone} className={className}>
      <LiveDot tone={tone} pulse={tone === 'live'} />
      {state ?? '—'}
    </Pill>
  )
}

/** Each level's mark and tone (theseus-in3): ● needs you, ◐ working, ○ ready, · idle, as every surface draws them. */
const LEVEL: Record<Attention['level'], { mark: string; tone: Tone }> = {
  needs_you: { mark: '●', tone: 'fault' },
  working: { mark: '◐', tone: 'live' },
  ready: { mark: '○', tone: 'ok' },
  idle: { mark: '·', tone: 'idle' },
}

/** What a session needs from you, as the server's one function says it (theseus-in3). A daemon that sends no
 * attention gets the state pill. */
export function AttentionPill({ a, state, className }: { a: Attention | null | undefined; state?: string | null; className?: string }) {
  if (!a) return <StatePill state={state ?? 'idle'} className={className} />
  const l = LEVEL[a.level]
  return (
    <Pill tone={l.tone} className={cn('max-w-[28ch] truncate', className)} title={`${a.label} · ${a.level.replace('_', ' ')} since ${new Date(a.since_ms).toLocaleTimeString()}`}>
      <span aria-hidden>{l.mark}</span>
      <span className="truncate">{a.label}</span>
    </Pill>
  )
}

export function LiveDot({ tone = 'live', pulse = true, size = 6 }: { tone?: Tone; pulse?: boolean; size?: number }) {
  const c = toneHex[tone]
  return (
    <span className="relative inline-flex" style={{ width: size, height: size }}>
      {pulse && (
        <span
          className="absolute inset-0 rounded-full animate-ping"
          style={{ background: c, opacity: 0.45, animationDuration: '1.8s' }}
        />
      )}
      <span className="relative rounded-full" style={{ width: size, height: size, background: c, boxShadow: `0 0 8px ${c}` }} />
    </span>
  )
}

/** A number that glides to its new value, so a change is seen, not just shown. */
export function AnimatedNumber({ value, format, className }: { value: number; format: (n: number) => string; className?: string }) {
  const [shown, setShown] = useState(value)
  const from = useRef(value)
  useEffect(() => {
    const ctl = animate(from.current, value, {
      duration: 0.6, ease: [0.22, 1, 0.36, 1],
      onUpdate: (v) => setShown(v),
    })
    from.current = value
    return () => ctl.stop()
  }, [value])
  return <span className={cn('num', className)}>{format(shown)}</span>
}

export function Spark({ data, tone = 'live', area = true, height = 34 }: { data: number[]; tone?: Tone; area?: boolean; height?: number }) {
  const c = toneHex[tone]
  const option = useMemo<EChartsOption>(() => ({
    grid: { left: 0, right: 0, top: 2, bottom: 0 },
    xAxis: { type: 'category', show: false, boundaryGap: false, data: data.map((_, i) => i) },
    yAxis: { type: 'value', show: false, min: 0 },
    tooltip: { show: false },
    animation: false,
    series: [{
      type: 'line', data, smooth: 0.35, symbol: 'none',
      lineStyle: { color: c, width: 1.5 },
      areaStyle: area ? {
        color: { type: 'linear', x: 0, y: 0, x2: 0, y2: 1, colorStops: [{ offset: 0, color: `${c}55` }, { offset: 1, color: `${c}00` }] },
      } : undefined,
    }],
  }), [data, c, area])
  return <div style={{ height }}><Echart option={option} /></div>
}

export function Kpi({
  label, value, format, tone = 'live', spark, hint, icon, onClick,
}: {
  label: string; value: number; format: (n: number) => string; tone?: Tone; spark?: number[]; hint?: ReactNode
  icon?: ReactNode; onClick?: () => void
}) {
  return (
    <motion.button
      type="button"
      onClick={onClick}
      whileHover={{ y: -2 }}
      className={cn('panel group relative overflow-hidden px-3.5 pb-2 pt-3 text-left', onClick && 'cursor-pointer')}
    >
      <div className="absolute inset-x-0 top-0 h-px" style={{ background: `linear-gradient(90deg, transparent, ${toneHex[tone]}, transparent)`, opacity: 0.6 }} />
      <div className="flex items-center gap-1.5 text-[11px] font-medium uppercase tracking-wider text-ink-faint">
        {icon}<span className="truncate">{label}</span>
      </div>
      <div className="mt-1 text-2xl font-semibold" style={{ color: toneHex[tone] }}>
        <AnimatedNumber value={value} format={format} />
      </div>
      <div className="mt-0.5 h-4 truncate text-[11px] text-ink-faint" title={typeof hint === 'string' ? hint : undefined}>{hint}</div>
      <div className="-mx-3.5 -mb-2 mt-1 h-[34px]">
        {spark && spark.length > 1
          ? <Spark data={spark} tone={tone} />
          : <div className="mx-3.5 mt-6 h-px" style={{ background: `linear-gradient(90deg, transparent, ${toneHex[tone]}44, transparent)` }} />}
      </div>
    </motion.button>
  )
}

/** A horizontal fill bar: budgets, headroom, shares. */
export function Meter({ value, max, tone = 'live', className }: { value: number; max: number; tone?: Tone; className?: string }) {
  const f = max > 0 ? Math.min(1, Math.max(0, value / max)) : 0
  return (
    <div className={cn('h-1.5 w-full overflow-hidden rounded-full bg-white/5', className)}>
      <motion.div
        className="h-full rounded-full"
        initial={false}
        animate={{ width: `${f * 100}%` }}
        transition={{ type: 'spring', stiffness: 120, damping: 20 }}
        style={{ background: toneHex[tone], boxShadow: `0 0 10px ${toneHex[tone]}66` }}
      />
    </div>
  )
}

/** An action button in a tone: approve (ok), stop or trust (wait), cancel (fault). */
export function Btn({ children, onClick, tone = 'live', busy, title }: { children: ReactNode; onClick: () => void; tone?: Tone; busy?: boolean; title?: string }) {
  return (
    <button onClick={onClick} disabled={busy} title={title}
      className="inline-flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-[12px] font-medium transition-[filter] hover:brightness-125 disabled:opacity-50"
      style={{ color: toneHex[tone], background: `${toneHex[tone]}14`, boxShadow: `inset 0 0 0 1px ${toneHex[tone]}40` }}>
      {busy ? '…' : children}
    </button>
  )
}

/** A compact segmented control for ranges and modes. */
export function Segmented<T extends string>({ value, options, onChange }: { value: T; options: readonly T[]; onChange: (v: T) => void }) {
  return (
    <div className="flex items-center rounded-md bg-white/[0.04] p-0.5 ring-1 ring-line">
      {options.map((o) => (
        <button
          key={o}
          onClick={() => onChange(o)}
          className={cn('num rounded px-1.5 py-0.5 text-[10px] font-medium transition-colors', o === value ? 'bg-live/15 text-live' : 'text-ink-faint hover:text-ink')}
        >
          {o}
        </button>
      ))}
    </div>
  )
}

export function Empty({ children }: { children: ReactNode }) {
  return <div className="flex h-full min-h-24 items-center justify-center text-sm text-ink-faint">{children}</div>
}

export function Field({ label, children, mono }: { label: string; children: ReactNode; mono?: boolean }) {
  return (
    <div className="flex items-baseline justify-between gap-3 py-0.5 text-[12px]">
      <span className="shrink-0 text-ink-faint">{label}</span>
      <span className={cn('min-w-0 truncate text-right text-ink', mono && 'num')}>{children}</span>
    </div>
  )
}
