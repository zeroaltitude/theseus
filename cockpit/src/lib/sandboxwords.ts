// L1 in words, as the CLI's `sandbox:` line says it (`crates/theseus/src/render/sandbox.rs`) and the Observatory's
// Sandbox section showed it: the last real launch since the start, the jobs by class, what an L1 job is given, and
// its egress (theseus-vm3n.6). Pure, so `node --test` runs its test (`test/sandboxwords.test.ts`).
import type { SandboxHealth, SandboxLaunch } from '@protocol'

/** How an L1 launch went, as the protocol's `launch_words` says it: `worked (start 8.1 ms; lo down)`, or
 *  `failed: <why> (an L1 call fails, and never runs at L0)`. */
export function launchWords(l: SandboxLaunch): string {
  if (!l.ok) return `failed: ${l.why ?? 'no reason given'} (an L1 call fails, and never runs at L0)`
  const found: string[] = []
  if (l.start_ms != null) found.push(`start ${l.start_ms.toFixed(1)} ms`)
  if (l.sys === false) found.push('no sysfs: the kernel refused it')
  if (l.lo === false) found.push('lo down')
  return found.length ? `worked (${found.join('; ')})` : 'worked'
}

/** The line's head, and its tone: why L1 refuses every job here, that no L1 job has run since the start, or how
 *  the last real launch went. */
export function l1Head(s: SandboxHealth): { words: string; tone: 'ok' | 'fault' | 'idle' } {
  if (s.refuses) return { words: `L1 is unavailable: ${s.refuses}`, tone: 'fault' }
  if (!s.last_launch) return { words: 'no L1 job yet since start', tone: 'idle' }
  return { words: `the last L1 launch ${launchWords(s.last_launch)}`, tone: s.last_launch.ok ? 'ok' : 'fault' }
}

/** The jobs started since the daemon started, by class: `12 at L0, 3 in L1`. */
export const jobsWords = (s: SandboxHealth): string => `${s.jobs_l0} at L0, ${s.jobs_l1} in L1`

/** What an L1 job is given, `[sandbox]`'s limits: `512 processes, 1024 MB of scratch, files up to 64 MB`. */
export const givenWords = (s: SandboxHealth): string =>
  `${s.pids} processes, ${s.scratch_mb} MB of scratch, files up to ${s.output_mb} MB`

/** Bytes as the CLI counts them, in thousands: `812 B`, `1.2 KB`, `340.1 MB`. */
export function decimalBytes(n: number): string {
  return n < 1_000 ? `${n} B` : n < 1_000_000 ? `${(n / 1e3).toFixed(1)} KB` : `${(n / 1e6).toFixed(1)} MB`
}

/** The egress (M4 18c): `egress: 2 hosts listed (github.com:443, *.crates.io:443); 5 connections, 1.2 KB up,
 *  340.1 KB down; 1 refused (latest: …)`, or `no egress listed` while `[sandbox] egress` is empty. */
export function egressWords(s: SandboxHealth): string {
  const list = s.egress ?? []
  let out = list.length ? `egress: ${list.length} host${list.length === 1 ? '' : 's'} listed (${list.join(', ')})` : 'no egress listed'
  const conns = s.egress_connections ?? 0
  if (conns > 0) out += `; ${conns} connection${conns === 1 ? '' : 's'}, ${decimalBytes(s.egress_up ?? 0)} up, ${decimalBytes(s.egress_down ?? 0)} down`
  const refused = s.egress_refused ?? 0
  if (refused > 0) out += `; ${refused} refused${s.egress_last_refused ? ` (latest: ${s.egress_last_refused})` : ''}`
  return out
}

/** The whole line, as `theseus health` prints it after `sandbox: `. */
export function sandboxLine(s: SandboxHealth): string {
  const parts = [l1Head(s).words, `default ${s.default}`]
  if (s.l1_argv.length) parts.push(`always L1: ${s.l1_argv.join('; ')}`)
  parts.push(`jobs: ${jobsWords(s)}`, `an L1 job gets ${givenWords(s)}`, egressWords(s))
  const skipped = s.last_launch?.skipped ?? []
  if (skipped.length) parts.push(`ro_paths missing: ${skipped.join(', ')}`)
  return parts.join(' · ')
}
