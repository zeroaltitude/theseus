// How the cockpit colors and groups things: execution and action states, ledger kinds, narrative parts.
// One table, so every view speaks the same color language.

export type Tone = 'live' | 'ok' | 'wait' | 'fault' | 'model' | 'tool' | 'think' | 'money' | 'idle'

export const toneHex: Record<Tone, string> = {
  live: '#22d3ee',
  ok: '#34d399',
  wait: '#fbbf24',
  fault: '#fb7185',
  model: '#a78bfa',
  tool: '#38bdf8',
  think: '#e879f9',
  money: '#facc15',
  idle: '#9c907a',
}

export const toneClass: Record<Tone, string> = {
  live: 'text-live bg-live/10 ring-live/30',
  ok: 'text-ok bg-ok/10 ring-ok/30',
  wait: 'text-wait bg-wait/10 ring-wait/30',
  fault: 'text-fault bg-fault/10 ring-fault/30',
  model: 'text-model bg-model/10 ring-model/30',
  tool: 'text-tool bg-tool/10 ring-tool/30',
  think: 'text-think bg-think/10 ring-think/30',
  money: 'text-money bg-money/10 ring-money/30',
  idle: 'text-ink-faint bg-white/5 ring-white/10',
}

/** Execution states (the kernel's) and action states, to a tone. Unknown states read idle. */
export function stateTone(state: string | null | undefined): Tone {
  switch (state) {
    case 'running': case 'dispatched': case 'streaming': case 'connected': case 'ready': return 'live'
    case 'succeeded': case 'settled': case 'done': case 'confirmed': case 'trusted': case 'ok': return 'ok'
    case 'waiting': case 'queued': case 'planned': case 'authorized': case 'awaiting_confirm': case 'held':
    case 'pending': case 'confirming': case 'resolving': case 'connecting': return 'wait'
    case 'failed': case 'budget_exhausted': case 'interrupted': case 'cancelled': case 'refused': case 'denied':
    case 'error': case 'unknown': case 'quarantined': case 'not_trusted': return 'fault'
    default: return 'idle'
  }
}

export interface KindInfo { family: string; tone: Tone }

/** A ledger kind's family (its first segment, mostly) and tone. */
export function ledgerKind(kind: string): KindInfo {
  const fam = kind.split('.')[0]
  if (/failed|denied|refused|corrupt|error|lost|disconnected|quarantin/.test(kind)) return { family: fam, tone: 'fault' }
  switch (fam) {
    case 'provider': case 'model': case 'loop': return { family: fam, tone: 'model' }
    case 'action': case 'tool': return { family: fam, tone: 'tool' }
    case 'turn': case 'execution': case 'driver': return { family: fam, tone: 'live' }
    case 'context': return { family: fam, tone: 'think' }
    case 'budget': return { family: fam, tone: 'money' }
    case 'policy': case 'session': case 'approval': case 'web': return { family: fam, tone: 'wait' }
    case 'discord': return { family: fam, tone: 'ok' }
    default: return { family: fam, tone: 'idle' }
  }
}

export const partTone: Record<string, Tone> = {
  session: 'live', turn: 'live', loop: 'model', context: 'think', model: 'model', tool: 'tool',
  approval: 'wait', job: 'tool', config: 'idle',
}
