// Following a job's output in the inspector (theseus-jid7t.6): `work.peek` for the tail, then `action.output` for the
// chunks, and `action.unfollow` on leaving. Pure: a state machine over what the page tells it, so the rule "following
// unsubscribes on leave, and never twice" is a test (`test/ops-follow.test.ts`), not a habit.
//
//   select(job)  a different job (or none) than the one followed: unfollow it first, then peek the new one
//   peeked       the tail has come: follow from its `bytes`
//   leave        the selection left, the lens switched or the inspector unmounted: unfollow if following
export type Follow =
  | { kind: 'off' }
  | { kind: 'peeking'; job: string }
  | { kind: 'following'; job: string; from: number }

export type FollowEvent =
  | { kind: 'select'; job: string | null }
  | { kind: 'peeked'; job: string; bytes: number }
  | { kind: 'leave' }

export type FollowEffect = { op: 'peek' | 'follow' | 'unfollow'; job: string; from?: number }

export function followOf(s: Follow, e: FollowEvent): { state: Follow; effects: FollowEffect[] } {
  const was = s.kind === 'following' ? s.job : null
  const stop: FollowEffect[] = was ? [{ op: 'unfollow', job: was }] : []
  switch (e.kind) {
    case 'select':
      if (e.job && s.kind !== 'off' && s.job === e.job) return { state: s, effects: [] }
      return e.job ? { state: { kind: 'peeking', job: e.job }, effects: [...stop, { op: 'peek', job: e.job }] } : { state: { kind: 'off' }, effects: stop }
    case 'peeked':
      // A peek answered after the selection moved on is stale: nothing follows.
      return s.kind === 'peeking' && s.job === e.job ? { state: { kind: 'following', job: e.job, from: e.bytes }, effects: [{ op: 'follow', job: e.job, from: e.bytes }] } : { state: s, effects: [] }
    case 'leave': return { state: { kind: 'off' }, effects: stop }
  }
}
