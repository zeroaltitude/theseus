// The ledger's one-line words (`src/lib/summary.ts`), run by `npm test`: each row kind the Observatory said in words
// is said here too (theseus-vm3n.6). Invented ids and names.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { nodeSummary, summarize } from '../src/lib/summary.ts'

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

test('a family row with none of its fields falls back to its first fields', () => {
  assert.equal(summarize(row('action.cancel_verified', { correlation_id: 'act_1', state: 'verified' })), 'correlation_id=act_1 · state=verified')
})

test('discord rows name the place, the author, and a failed press', () => {
  assert.equal(summarize(row('discord.message.in', { place: 'ops', author: 'ada', chars: 12 })), '← ops · from ada · 12 chars')
  assert.equal(summarize(row('discord.confirm', { approve: true, by: 'ada', ok: false, error: 'late' })),
    'Discord press: approve by ada (failed: late)')
  assert.equal(summarize(row('discord.ignored', { author: 'bot', author_id: '7', reason: 'not bound' })), 'bot (7) · not bound')
})

test('a node reads in one line, as the Observatory\'s Nodes list said it', () => {
  const node = (kind: string, text: string, detail: Record<string, unknown>) =>
    ({ node_id: 'nod_1', session_id: 'ses_1', turn_id: null, kind, text, at_unix_ms: 1, position: 1, detail }) as any
  assert.equal(nodeSummary(node('user_message', 'list   the\nharbour files', {})), 'list the harbour files')
  assert.equal(nodeSummary(node('assistant_message', 'Reading them.', { model: 'm-1', stop_reason: 'tool_use', cost_usd: 0.002, tool_calls: [{}, {}] })),
    'm-1 · tool_use · $0.0020 · 2 tool calls · Reading them.')
  assert.equal(nodeSummary(node('tool_call', '', { tool: 'fs.list', input: { path: '.' }, decision: { posture: 'open' } })), 'fs.list {"path":"."} · open')
  assert.equal(nodeSummary(node('tool_result', 'a.txt', { tool: 'fs.list', status: 'ok', late: true })), 'fs.list · ok · late · a.txt')
})
