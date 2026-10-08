// The pure half of the formatting: figures and times as words, with no import at all, so `node --test` runs it (and
// the modules built on it, like summary.ts) as it is. format.ts re-exports all of it.

export function usd(n: number | null | undefined, digits?: number): string {
  if (n === null || n === undefined || Number.isNaN(n)) return '—'
  const a = Math.abs(n)
  const d = digits ?? (a === 0 ? 2 : a < 0.01 ? 4 : a < 1 ? 3 : 2)
  return `$${n.toFixed(d)}`
}

export function tokens(n: number | null | undefined): string {
  if (n === null || n === undefined) return '—'
  const a = Math.abs(n)
  if (a >= 1e9) return `${(n / 1e9).toFixed(2)}B`
  if (a >= 1e6) return `${(n / 1e6).toFixed(a >= 1e7 ? 1 : 2)}M`
  if (a >= 1e3) return `${(n / 1e3).toFixed(a >= 1e4 ? 0 : 1)}k`
  return `${Math.round(n)}`
}

export function ms(n: number | null | undefined): string {
  if (n === null || n === undefined) return '—'
  if (n === 0) return '0 ms'
  if (n < 1) return `${(n * 1000).toFixed(0)} µs`
  if (n < 1000) return `${n < 10 ? n.toFixed(1) : Math.round(n)} ms`
  const s = n / 1000
  if (s < 60) return `${s.toFixed(s < 10 ? 2 : 1)} s`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m}m ${Math.floor(s % 60)}s`
  const h = Math.floor(m / 60)
  return `${h}h ${m % 60}m`
}

export function us(n: number | null | undefined): string {
  return n === null || n === undefined ? '—' : ms(n / 1000)
}

export function uptime(secs: number): string {
  const d = Math.floor(secs / 86400)
  const h = Math.floor((secs % 86400) / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = Math.floor(secs % 60)
  if (d > 0) return `${d}d ${h}h ${m}m`
  if (h > 0) return `${h}h ${m}m ${s}s`
  if (m > 0) return `${m}m ${s}s`
  return `${s}s`
}

export function ago(at: number | null | undefined, now = Date.now()): string {
  if (!at) return '—'
  const d = Math.max(0, now - at) / 1000
  if (d < 5) return 'now'
  if (d < 60) return `${Math.floor(d)}s ago`
  if (d < 3600) return `${Math.floor(d / 60)}m ago`
  if (d < 86400) return `${Math.floor(d / 3600)}h ago`
  return `${Math.floor(d / 86400)}d ago`
}

// One formatter per format, made once: building an Intl.DateTimeFormat costs far more than formatting with one (a
// replay's rows spent about half a second of the main thread in making them, theseus-yal8).
let clockFormat: Intl.DateTimeFormat | undefined
let monthDayFormat: Intl.DateTimeFormat | undefined

export function clock(at: number | null | undefined): string {
  if (!at) return '—'
  clockFormat ??= new Intl.DateTimeFormat([], { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' })
  return clockFormat.format(at)
}

/** `Oct 6`: a day's short month and number, in the viewer's locale. */
export function monthDay(at: number): string {
  monthDayFormat ??= new Intl.DateTimeFormat([], { month: 'short', day: 'numeric' })
  return monthDayFormat.format(at)
}

export function stamp(at: number | null | undefined): string {
  if (!at) return '—'
  return `${monthDay(at)} ${clock(at)}`
}

/** `ses_01a0ebfa…61b55e` → `ses·61b55e`: people name ids by their last six characters. */
export function short(id: string | null | undefined): string {
  if (!id) return '—'
  const i = id.indexOf('_')
  return i > 0 ? `${id.slice(0, i)}·${id.slice(-6)}` : id.slice(-6)
}

export function pct(n: number | null | undefined, digits = 0): string {
  return n === null || n === undefined || Number.isNaN(n) ? '—' : `${(n * 100).toFixed(digits)}%`
}

export function bytes(n: number | null | undefined): string {
  if (n === null || n === undefined) return '—'
  if (n < 1024) return `${n} B`
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(1)} KiB`
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MiB`
  return `${(n / 1024 ** 3).toFixed(2)} GiB`
}
