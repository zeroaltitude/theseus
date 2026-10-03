// The ledger's one-line words (`src/lib/summary.ts`), run by `npm test`: each row kind the Observatory said in words
// is said here too (theseus-vm3n.6). Invented ids and names.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { summarize } from '../src/lib/summary.ts'

const row = (kind: string, data: Record<string, unknown> = {}) =>
  ({ position: 1, at_unix_ms: 1, kind, session_id: null, turn_id: null, data }) as any

test('a budget question, a reset, and a changed limit read in dollars', () => {
  assert.equal(summarize(row('budget.asked', { spent_usd: 1, limit_usd: 1, needed_usd: 0.25 })),
    'at its limit: spent $1.00 of $1.00, the call needs $0.250')
  assert.equal(summarize(row('budget.reset', { by: 'cli', spent_before_usd: 0.99, limit_usd: 1, resets: 2 })),
    'spend reset to $0 by cli · it was $0.990 of $1.00 · reset 2')
  assert.match(summarize(row('budget.limit_changed', { from_usd: 1, to_usd: 2, spent_usd: 0.5, available_usd: 1.5, proceeds: true })),
    /limit \$1\.00 → \$2\.00 \(the config's\) · spent \$0\.500, \$1\.50 left · the waiting call proceeds/)
})

test('the gate and its undo: approvals refused, tightenings, notices', () => {
  assert.equal(summarize(row('approval.refused', { act: 'policy.tighten', tool: 'fs.write', who: 'mallory', via: 'discord', why: 'not a trusted user' })),
    'should have asked for fs.write · mallory via discord did not count: not a trusted user')
  assert.equal(summarize(row('policy.untightened', { tool: 'fs.write', posture: 'notify', setting: '[tools] fs.write', by: 'cli', via: 'cli', tightened_by: 'web' })),
    'fs.write back to notify ([tools] fs.write) · undone by cli via cli · tightened by web')
  assert.equal(summarize(row('tool.notified', { tool: 'fs.read', summary: 'a/b', setting: 'open', granted: 'TOKEN' })),
    'notified · fs.read · a/b · open · 🔑 TOKEN')
})

test('a provider error says its class and whether it may be tried again', () => {
  assert.equal(summarize(row('provider.error', { class: 'rate_limit', transient: true, usage_unknown: true, message: 'slow down' })),
    'rate_limit transient usage unknown · slow down')
})

test('a family with no case of its own reads by its outcome', () => {
  assert.equal(summarize(row('action.cancelled', { cancel: 'stopped', duration_ms: 12, execution_state: 'running' })),
    'stopped · 12 ms · execution running')
  assert.equal(summarize(row('completion.received', { producer: 'job', outcome: 'ok', seen: 2 })), 'job · ok · seen 2')
  assert.equal(summarize(row('reconcile', { woke_due: ['a'], marked_unknown: [], settled_from_evidence: ['b', 'c'], elapsed_us: 40 })),
    'woke 1 · unknown 0 · settled 2 · 40 µs')
})

test('discord rows name the place, the author, and a failed press', () => {
  assert.equal(summarize(row('discord.message.in', { place: 'ops', author: 'ada', chars: 12 })), '← ops · from ada · 12 chars')
  assert.equal(summarize(row('discord.confirm', { approve: true, by: 'ada', ok: false, error: 'late' })),
    'Discord press: approve by ada (failed: late)')
  assert.equal(summarize(row('discord.ignored', { author: 'bot', author_id: '7', reason: 'not bound' })), 'bot (7) · not bound')
})
