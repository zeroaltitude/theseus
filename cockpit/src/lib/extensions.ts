// The Extensions card's pure parts (M7 43b): a digest as a card names it, an extension's network in the CLI's words,
// and the proposals that are not what runs.
import type { ExtendHandsInfo, ExtendListResult, ExtendUsage } from '@protocol'

/** A digest as a card names it: its first six characters. */
export function shortDigest(d: string): string {
  return d.slice(0, 6)
}

/** The network an extension may reach, in the CLI's words. */
export function networkWords(hosts: readonly string[]): string {
  return hosts.length ? `network to ${hosts.join(', ')}` : 'no network'
}

/** An extension's hands (theseus-pw1q.14) in the CLI's words: ExtendHandsInfo::line in theseus-protocol. */
export function handsWords(h: ExtendHandsInfo): string {
  const usd = (n: number) => `$${n.toFixed(2)}`
  return (
    `up to ${h.max_group} hand${h.max_group === 1 ? '' : 's'} at a time on ${h.backends.join(' or ')} · ` +
    `${usd(h.today_usd)} of ${usd(h.usd_cap)} today · ${usd(h.spent_usd)} in all · ${h.running} running`
  )
}

/** Every proposal but the versions loaded now, newest first as the list has them. */
export function proposalsNotLoaded(list: ExtendListResult) {
  const running = new Set((list.loaded ?? []).map((l) => `${l.name}.${l.digest}`))
  return list.extensions.filter((e) => !running.has(`${e.name}.${e.digest}`))
}

/** An extension's use over its window, in `ExtendUsage::line`'s words (theseus-pw1q.13). */
export function usageWords(u: ExtendUsage): string {
  const n = (k: number, one: string, many: string) => `${k} ${k === 1 ? one : many}`
  return `used ${n(u.calls, 'time', 'times')} in ${u.window_days} days, ${n(u.failures, 'failure', 'failures')}, p50 ${u.p50_ms} ms, p95 ${u.p95_ms} ms${u.truncated ? ' (at least)' : ''}`
}
