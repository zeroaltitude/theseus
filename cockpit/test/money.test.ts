// What caching saved, net of the premium its writes paid (`src/lib/money.ts`), run by `npm test` (theseus-vm3n.6).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { cacheBy, netSaved, type Price } from '../src/lib/money.ts'

// $3 in, $15 out per million; reads $0.30; 5-minute writes $3.75; 1-hour writes $6.
const price: Price = { input: 3, output: 15, cacheRead: 0.3, cacheWrite: 3.75, cacheWrite1h: 6 }
const call = (session: string, usage: Record<string, number>) =>
  ({ session_id: session, model: 'm', usage: { input_tokens: 0, output_tokens: 0, cache_read_input_tokens: 0, cache_creation_input_tokens: 0, ...usage } }) as any

test('reads save the difference, and writes pay their premium back', () => {
  // 1M read saves $2.70; 1M written (all 5-minute) costs $0.75 over plain input.
  assert.ok(Math.abs(netSaved(call('a', { cache_read_input_tokens: 1e6 }), price) - 2.7) < 1e-9)
  assert.ok(Math.abs(netSaved(call('a', { cache_creation_input_tokens: 1e6 }), price) + 0.75) < 1e-9)
  // The same million written at the 1-hour price: premium $3.
  assert.ok(Math.abs(netSaved(call('a', { cache_creation_input_tokens: 1e6, cache_creation_1h_input_tokens: 1e6 }), price) + 3) < 1e-9)
  assert.equal(netSaved(call('a', { cache_read_input_tokens: 1e6 }), undefined), 0)
})

test('a profile shows its input, its read share, what it wrote, and what it saved; empty ones are left out', () => {
  const prices = new Map([['m', price]])
  const rows = cacheBy([
    call('s1', { input_tokens: 100, cache_read_input_tokens: 900 }),
    call('s2', { input_tokens: 100, cache_creation_input_tokens: 400 }),
    call('s3', {}),
  ], (c) => (c.session_id === 's1' ? 'sonnet' : c.session_id === 's2' ? 'sonnet' : 'glm'), prices)
  assert.equal(rows.length, 1)
  assert.deepEqual({ ...rows[0], saved: Number(rows[0].saved.toFixed(6)) }, { key: 'sonnet', sessions: 2, input: 1500, read: 900, written: 400, saved: Number(((900 * 2.7 - 400 * 0.75) / 1e6).toFixed(6)) })
})
