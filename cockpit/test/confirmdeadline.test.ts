// A question's deadline, on every cockpit surface, comes from `deadlineOf` and never from the raw `expires_at_ms`
// (theseus-lrc3m, F3). Since theseus-v2dnc a call's question is held until it is answered and that field carries the time
// of its FIRST NOTICE, not a deadline. Live, `confirm.list` zeroes it, so a reader of the raw field is accidentally
// right; under the time machine the questions come from the ledger fold (`lib/timemachine.ts:293`), which carries the
// field as recorded, and a raw reader then says "expired" about a question that never expired.
//
// Two readers did exactly that and were fixed -- `ops/queue.ts` (64275aad) and `ship/watch.ts` (68c00a2c) -- and
// `cockpit/test/watch.test.ts` pins the playback behaviour of the second. What neither pins is the NEXT reader: a third
// surface added tomorrow reads the raw field and nothing says so, which is the same shape as F1's third list. This file
// is that guard. The sources are read as text: these are React components and modules this runner does not import.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'

const ROOT = new URL('..', import.meta.url).pathname
const GEN = join(ROOT, 'src/protocol.gen')

/** The receivers that may read `expires_at_ms` directly, and why. A new reader belongs in this list only with a reason
 *  that says the value it reads is a real deadline — which a call's question's is not. */
const ALLOWED: Record<string, string> = {
  'src/lib/heldquestion.ts c':
    'deadlineOf itself: the one place the field is read, and it returns 0 for a held question',
  'src/ops/queue.ts v.question':
    "a work question view's own deadline (`work::Question`), not a call's confirm: an `owner_ask` does expire",
  'src/lib/timemachine.ts d':
    'the ledger row the fold reads, the producer of a ConfirmRequest rather than a reader of one',
}

/** Every `.ts`/`.tsx` under `src` except the generated directory. */
function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((f) => {
    const p = join(dir, f)
    if (statSync(p).isDirectory()) return p === GEN || f === 'node_modules' ? [] : sources(p)
    return /\.tsx?$/.test(p) ? [p] : []
  })
}

/** `receiver.expires_at_ms`, past an optional `?.` or `!.`, with the receiver's whole property chain. */
const READ = /([A-Za-z_$][\w$]*(?:[?!]?\.[A-Za-z_$][\w$]*)*?)[?!]?\.expires_at_ms/g

test('no cockpit surface reads a confirm\'s expires_at_ms: the deadline comes from deadlineOf', () => {
  const unlisted: string[] = []
  for (const p of sources(join(ROOT, 'src'))) {
    const text = readFileSync(p, 'utf8')
    // Only a module that has a ConfirmRequest in hand can read one's field; the watches' own expiry is a real deadline.
    if (!text.includes('ConfirmRequest')) continue
    const rel = relative(ROOT, p)
    for (const m of text.matchAll(READ)) {
      const key = `${rel} ${m[1]}`
      if (key in ALLOWED) continue
      unlisted.push(`${rel}:${text.slice(0, m.index).split('\n').length} reads ${m[1]}.expires_at_ms`)
    }
  }
  assert.deepEqual(
    unlisted,
    [],
    'a raw deadline reader: use deadlineOf(c) from lib/heldquestion, which is 0 for a question held until answered — the raw field is a held call\'s first notice and says "expired" in playback (theseus-lrc3m, F3)',
  )
})

test('the three surfaces that show a deadline all take it from deadlineOf', () => {
  for (const [rel, what] of [
    ['src/ship/watch.ts', 'the watch\'s waiting lines'],
    ['src/ops/queue.ts', 'the queue item\'s expiresAt'],
    ['src/components/ConfirmCard.tsx', 'the card\'s "left" and "expires in"'],
  ] as const) {
    const text = readFileSync(join(ROOT, rel), 'utf8')
    assert.match(text, /\bdeadlineOf\(/, `${what} (${rel}) no longer calls deadlineOf`)
  }
})

test('the watch sorts and labels by deadlineOf, and its comment still says the raw field is not a deadline', () => {
  const lines = readFileSync(join(ROOT, 'src/ship/watch.ts'), 'utf8').split('\n')
  const sort = lines.findIndex((l) => l.includes('qs.sort('))
  assert.ok(sort > 0, 'the waiting lines are still sorted in ship/watch.ts')
  assert.match(lines[sort], /deadlineOf\(a\)[\s\S]*deadlineOf\(b\)/, 'the sort reads the raw field again')
  // The comment F3 found stale said only "the soonest to expire, then the budget questions". It must now warn about the
  // field as well, or the next reader is misled exactly as that one was.
  const comment = lines.slice(Math.max(0, sort - 4), sort).filter((l) => l.trimStart().startsWith('//')).join(' ')
  assert.match(comment, /deadlineOf/, 'the comment above the sort no longer names deadlineOf')
  assert.match(comment, /expires_at_ms/, 'the comment above the sort no longer warns about the raw field')
  assert.match(comment, /hold until answered|holds until answered/, 'the comment no longer says what sorts with the deadlines')
})
