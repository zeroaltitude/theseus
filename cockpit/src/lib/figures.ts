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
  return span(n)
}

/**
 * A span of time as people say it, flooring each unit: `40s`, `12m`, `2h 46m` (`2h` when no minutes remain), `1d 21h`
 * (`1d`), and from seven days only the days, `9d`. A negative or non-finite span reads as zero.
 */
export function span(ms: number): string {
  const s = Number.isFinite(ms) ? Math.max(0, Math.floor(ms / 1000)) : 0
  if (s < 60) return `${s}s`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m}m`
  const h = Math.floor(m / 60)
  if (h < 24) return m % 60 ? `${h}h ${m % 60}m` : `${h}h`
  const d = Math.floor(h / 24)
  if (d < 7) return h % 24 ? `${d}d ${h % 24}h` : `${d}d`
  return `${d}d`
}

export function us(n: number | null | undefined): string {
  return n === null || n === undefined ? '—' : ms(n / 1000)
}

export function uptime(secs: number): string {
  if (secs >= 86400) return span(secs * 1000)
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = Math.floor(secs % 60)
  if (h > 0) return `${h}h ${m}m ${s}s`
  if (m > 0) return `${m}m ${s}s`
  return `${s}s`
}

/** How long ago: `now` under five seconds, then `40s ago`, `12m ago`, `2h 46m ago`, `1d 21h ago`, `9d ago`. */
export function ago(at: number | null | undefined, now = Date.now()): string {
  if (!at) return '—'
  const d = Math.max(0, now - at)
  return d < 5000 ? 'now' : `${span(d)} ago`
}

/** How long until: `in 40s`, `in 2h 46m`, `in 1d 21h`. A time that has come reads as overdue: `due 3m ago`, or `due now`. */
export function until(at: number | null | undefined, now = Date.now()): string {
  if (!at) return '—'
  if (at > now) return `in ${span(at - now)}`
  return now - at < 5000 ? 'due now' : `due ${span(now - at)} ago`
}

// One formatter per format, made once: building an Intl.DateTimeFormat costs far more than formatting with one (a
// replay's rows spent about half a second of the main thread in making them, theseus-yal8).
/** Whether a Date can hold it: `Intl.DateTimeFormat.format` throws a RangeError where `toLocale*String` said "Invalid Date". */
const inRange = (at: number): boolean => Math.abs(at) <= 8.64e15
let clockFormat: Intl.DateTimeFormat | undefined
let monthDayFormat: Intl.DateTimeFormat | undefined

export function clock(at: number | null | undefined): string {
  if (!at || !inRange(at)) return '—'
  clockFormat ??= new Intl.DateTimeFormat([], { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' })
  return clockFormat.format(at)
}

/** `Oct 6`: a day's short month and number, in the viewer's locale. */
export function monthDay(at: number): string {
  if (!inRange(at)) return '—'
  monthDayFormat ??= new Intl.DateTimeFormat([], { month: 'short', day: 'numeric' })
  return monthDayFormat.format(at)
}

const DAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']
const two = (n: number): string => String(n).padStart(2, '0')

/**
 * A schedule-like time of day, in the viewer's zone, without seconds: `20:00` today, `tomorrow 08:00`, `yesterday 17:10`,
 * `Mon 09:00` within six days either way, and `Oct 30 09:00` beyond that. Log-like sites that need seconds use `clock`.
 */
export function when(at: number | null | undefined, now = Date.now()): string {
  if (!at || !inRange(at)) return '—'
  const t = new Date(at)
  const n = new Date(now)
  const days = Math.round((new Date(t.getFullYear(), t.getMonth(), t.getDate()).getTime() - new Date(n.getFullYear(), n.getMonth(), n.getDate()).getTime()) / 86_400_000)
  const hm = `${two(t.getHours())}:${two(t.getMinutes())}`
  if (days === 0) return hm
  if (days === 1) return `tomorrow ${hm}`
  if (days === -1) return `yesterday ${hm}`
  if (Math.abs(days) <= 6) return `${DAYS[t.getDay()]} ${hm}`
  return `${MONTHS[t.getMonth()]} ${t.getDate()} ${hm}`
}

export function stamp(at: number | null | undefined): string {
  if (!at || !inRange(at)) return '—'
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
