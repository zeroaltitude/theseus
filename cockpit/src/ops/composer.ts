// The Join pane's composer, as a state (theseus-jid7t.6): `idle` (nothing runs), `running` (a turn does), `queued`
// (a message waits for the turn to end, as the TUI's queue holds one). Pure: `test/ops-composer.test.ts`.
//
//   ⏎ while a turn runs steers it: `turn.submit` reaches the running turn at its next step (`turn.steered`)
//   ⌘⏎ while a turn runs queues the message, sent when `turn.ended` comes
//   esc drops a queued message
//   ⏎ with nothing running is an ordinary send; ⌘⏎ then is the same
export type ComposerState =
  | { kind: 'idle' }
  | { kind: 'running' }
  | { kind: 'queued'; text: string }

export type ComposerEvent =
  | { kind: 'turn_started' }
  | { kind: 'turn_ended' }
  | { kind: 'enter'; text: string }
  | { kind: 'mod_enter'; text: string }
  | { kind: 'escape' }

/** What the pane does with the event: send now (as a plain send or a steer), or nothing. */
export type ComposerEffect = { send: string; steer: boolean } | null

export function composerStep(s: ComposerState, e: ComposerEvent): { state: ComposerState; effect: ComposerEffect } {
  const text = 'text' in e ? e.text.trim() : ''
  switch (e.kind) {
    case 'turn_started': return { state: s.kind === 'queued' ? s : { kind: 'running' }, effect: null }
    case 'turn_ended':
      // The held message goes now, to the session the turn left idle.
      return s.kind === 'queued' ? { state: { kind: 'idle' }, effect: { send: s.text, steer: false } } : { state: { kind: 'idle' }, effect: null }
    case 'enter':
      if (!text) return { state: s, effect: null }
      return { state: s, effect: { send: text, steer: s.kind === 'running' } }
    case 'mod_enter':
      if (!text) return { state: s, effect: null }
      return s.kind === 'running' ? { state: { kind: 'queued', text }, effect: null } : { state: s, effect: { send: text, steer: false } }
    case 'escape': return { state: s.kind === 'queued' ? { kind: 'running' } : s, effect: null }
  }
}

/** The line under the composer: what ⏎ and ⌘⏎ do now. */
export function composerHint(s: ComposerState): string {
  if (s.kind === 'running') return '⏎ steers the running turn · ⌘⏎ queues this after it · ⇧⇥ the mode'
  if (s.kind === 'queued') return 'queued: it goes when the turn ends · esc drops it'
  return '⏎ sends · ⇧⇥ the mode'
}
