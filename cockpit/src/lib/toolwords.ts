// A tool call in words: its one-line summary, the preview of what an approval would do, and the gate's small
// phrases (the L1 pill's title, a `/stop`'s note). The Observatory's transcript said these; the cockpit's does now
// (theseus-vm3n.6). Pure, with no import, so `node --test` runs its test (`test/toolwords.test.ts`).

type D = Record<string, any>

const str = (v: unknown): string => (typeof v === 'string' ? v : v == null ? '' : JSON.stringify(v))
const clip = (s: string, n: number): string => (s.length > n ? `${s.slice(0, n)}…` : s)

/** A byte count, short: `812 B`, `3.4 KB`, `1.2 MB`. */
export function byteWords(b: number): string {
  return b >= 1 << 20 ? `${(b / (1 << 20)).toFixed(1)} MB` : b >= 1024 ? `${(b / 1024).toFixed(1)} KB` : `${b} B`
}

/** `fs_read` → `fs.read`: the provider's tool-name alphabet has no dots. */
export const wireToName = (w: string): string => w.replace('_', '.')

/** One line saying what a tool call does, in the terms of that tool. */
export function callSummary(tool: string, input: unknown): string {
  const i = (input ?? {}) as D
  switch (tool) {
    case 'proc.run': return `${((i.argv as string[] | undefined) ?? []).join(' ')}${i.cwd ? `   (in ${str(i.cwd)})` : ''}`
    case 'fs.read': return `${str(i.path)}${i.offset ? ` from line ${str(i.offset)}` : ''}${i.limit ? ` (${str(i.limit)} lines)` : ''}`
    case 'fs.write': return `${str(i.path)} (${byteWords(str(i.content).length)})`
    case 'fs.edit': return str(i.path)
    case 'fs.patch': return `patch of ${str(i.patch).split('\n').length} lines`
    case 'fs.glob': return `${str(i.pattern)}${i.path ? ` in ${str(i.path)}` : ''}`
    case 'fs.grep': return `/${str(i.pattern)}/${i.path ? ` in ${str(i.path)}` : ''}${i.glob ? ` (${str(i.glob)})` : ''}`
    case 'fs.list': return str(i.path) || '.'
    case 'git.diff': return `${str(i.repo) || str(i.path) || '.'}${i.rev ? ` ${str(i.rev)}` : ''}`
    case 'git.log': return `${str(i.repo) || str(i.path) || '.'}${i.n ? ` (${str(i.n)})` : ''}`
    case 'text.diff': return 'two texts'
    case 'http.fetch': return `${str(i.url)}${i.max_bytes ? ` (at most ${byteWords(Number(i.max_bytes))})` : ''}`
    case 'web.search': return `"${str(i.query)}"${i.count ? ` (${str(i.count)} results)` : ''}`
    default: return clip(JSON.stringify(input) ?? '', 120)
  }
}

/** Text that is a unified diff, so it draws with its additions and deletions marked. */
export const looksLikeDiff = (t: string): boolean => /(^|\n)@@ .* @@/.test(t) || /(^|\n)--- .*\n\+\+\+ /.test(t)

/** Each line of a diff and what it is: a file header, a hunk, an addition, a deletion, or context. */
export function diffLines(t: string): { line: string; kind: 'meta' | 'hunk' | 'add' | 'del' | 'ctx' }[] {
  return t.split('\n').map((line) => ({
    line,
    kind: line.startsWith('+++') || line.startsWith('---') ? 'meta' : line.startsWith('@@') ? 'hunk' : line.startsWith('+') ? 'add' : line.startsWith('-') ? 'del' : 'ctx',
  }))
}

export interface Preview { kind: 'diff' | 'text'; caption?: string; text: string }

/** What an approval would do, as the operator should read it: an edit as a diff, a write as its content, a patch as
 *  it is, a command as it would be typed. Any other tool has no preview (the card shows its input). */
export function previewOf(tool: string, input: unknown): Preview | null {
  const i = (input ?? {}) as D
  if (tool === 'fs.edit' && typeof i.old_string === 'string' && typeof i.new_string === 'string') {
    const text = [`--- ${str(i.path)}`, `+++ ${str(i.path)}`, '@@ edit @@',
      ...i.old_string.split('\n').map((l) => `-${l}`), ...i.new_string.split('\n').map((l) => `+${l}`)].join('\n')
    return { kind: 'diff', text }
  }
  if (tool === 'fs.write' && typeof i.content === 'string') {
    return { kind: 'text', caption: `${str(i.path)} · ${byteWords(i.content.length)}`, text: clip(i.content, 4000) }
  }
  if (tool === 'fs.patch' && typeof i.patch === 'string') return { kind: looksLikeDiff(i.patch) ? 'diff' : 'text', text: i.patch }
  if (tool === 'proc.run') {
    const argv = ((i.argv as string[] | undefined) ?? []).map((a) => (/\s/.test(a) ? `'${a}'` : a)).join(' ')
    return { kind: 'text', text: `$ ${argv}${i.cwd ? `\n  (in ${str(i.cwd)})` : ''}${i.timeout_secs ? `\n  (timeout ${str(i.timeout_secs)} s)` : ''}` }
  }
  return null
}

/** What an approval decides, to lead its card (theseus-hnof.5): the daemon's own plan, the head of the reason
 *  ("create /w/projects/harbour/log.md (38 bytes): fs.write — approve (…)"), with the path as the call named it and the
 *  size after a comma: "create harbour/log.md, 38 bytes". The rest of the reason is why the policy asks. A reason with
 *  no plan in it leads with the call's own summary, and the reason whole is the why. */
export function decisionWords(tool: string, input: unknown, reason: string): { what: string; why: string } {
  const at = reason.indexOf(`: ${tool} — `)
  if (at < 0) return { what: `${tool} ${callSummary(tool, input)}`.trim(), why: reason }
  let what = reason.slice(0, at)
  const rel = str(((input ?? {}) as D).path)
  if (rel && !rel.startsWith('/')) what = what.split(' ').map((w) => (w.startsWith('/') && w.endsWith(`/${rel}`) ? rel : w)).join(' ')
  what = what.replace(/ \(([\d,]+) bytes\)$/, ', $1 bytes')
  return { what, why: reason.slice(at + 2) }
}

/** The L1 pill's words (M4 17b, 18c): what an L1 job may reach. */
export function l1Words(egress: unknown): string {
  const hosts = Array.isArray(egress) ? egress.map(String) : []
  const reach = hosts.length > 0 ? `egress: ${hosts.join(', ')}; what it brings back is outside text` : 'no network'
  return `L1, the sandbox: ${reach}; no secret, an empty HOME; what it writes goes to scratch, and is discarded`
}

/** A call's result line, in words: its status (a call that never ran says so), a `/stop`'s note, its exit code. */
export function resultWords(detail: unknown): { status: string; stoppedBy: string | null; exit: number | null; tone: 'ok' | 'bad' | 'warn' | 'accent' | 'muted' } {
  const d = (detail ?? {}) as D
  const status = str(d.status)
  const meta = (d.meta ?? {}) as D
  // A `/stop` ended it: what the operator asked for, not a failure (theseus-4uw).
  const stoppedBy = status === 'cancelled' && meta.stopped_by != null ? str(meta.stopped_by) : null
  const exit = typeof meta.exit_code === 'number' ? meta.exit_code : null
  // A call that never ran; rows from before theseus-8az say `denied`.
  const never = status === 'declined' || status === 'denied'
  const tone = status === 'ok' ? 'ok' : status === 'error' ? 'bad' : never || status === 'unknown' ? 'warn' : status === 'background' ? 'accent' : 'muted'
  return { status: never ? 'not run' : status, stoppedBy, exit, tone }
}
