// The dock's `Models · profiles` strip grouped by model (theseus-yxkww): `sonnet` and `sonnethi` are one model, Sonnet
// 5.5, at two efforts, and the effort is a sub-line from what the turns really ran at (`route.decided`'s `effort_sent`
// or `effort_ran`), or the profiles' configured efforts, said to be configured, when no turn has been read.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { LedgerEntry } from '../src/protocol.gen/index.ts'
import { modelsStrip } from '../src/ops/live.ts'
import { groupByModel, modelLabel, routeEfforts } from '../src/ops/models.ts'

const SONNET = 'claude-sonnet-5-5'
const profiles = [
  { name: 'sonnet', model: SONNET, live: true, origin: 'us' },
  { name: 'sonnethi', model: SONNET, live: false, origin: 'us', effort: 'high' },
  { name: 'opus', model: 'claude-opus-5-5', live: false, origin: 'us' },
  { name: 'haiku', model: 'claude-haiku-5-5', live: false, origin: 'us', effort: 'low' },
  { name: 'haikuhi', model: 'claude-haiku-5-5', live: false, origin: 'us', effort: 'high' },
  { name: 'glm', model: 'glm-5.3-flash', live: false, origin: 'cn' },
]
const strip = (sessions: Array<{ profile?: string; retired?: unknown }> = [], calls: Array<{ model: string; rate?: { tokens_remaining: number; tokens_limit: number } }> = []) => modelsStrip(profiles, sessions, calls)
const ran = (model: string, ...levels: string[]) => levels.map((effort) => ({ model, effort }))

test('modelLabel: the model in the owner’s words, a date dropped and the version dotted', () => {
  assert.deepEqual(
    ['claude-sonnet-5-5', 'claude-opus-5-5', 'claude-haiku-5-5', 'claude-fable-5-1', 'claude-haiku-4-5-20251001', 'glm-5.3-flash', 'glm-5.3', 'deepseek-v4'].map(modelLabel),
    ['Sonnet 5.5', 'Opus 5.5', 'Haiku 5.5', 'Fable 5.1', 'Haiku 4.5', 'GLM 5.3 Flash', 'GLM 5.3', 'Deepseek V4'])
})

test('modelsStrip: a profile row carries the effort its profile is configured at', () => {
  assert.deepEqual(strip().map((r) => [r.name, r.effort ?? null]), [['sonnet', null], ['sonnethi', 'high'], ['opus', null], ['haiku', 'low'], ['haikuhi', 'high'], ['glm', null]])
})

test('groupByModel: sonnet and sonnethi are one model; its profiles stay under it, each with its sessions and live flag', () => {
  const g = groupByModel(strip([{ profile: 'sonnet' }, { profile: 'sonnet' }, { profile: 'sonnethi' }, { profile: 'glm' }, { profile: 'sonnet', retired: {} }]), [])
  assert.deepEqual(g.map((m) => m.label), ['Sonnet 5.5', 'Opus 5.5', 'Haiku 5.5', 'GLM 5.3 Flash'], 'one row a model, in the profile list’s order')
  const s = g[0]
  assert.equal(s.model, SONNET)
  assert.equal(s.sessions, 3, 'the sessions of every profile on it')
  assert.equal(s.live, true, 'live when any of its profiles is')
  assert.equal(s.origin, 'us')
  assert.deepEqual(s.profiles.map((p) => [p.name, p.live, p.sessions, p.effort]), [['sonnet', true, 2, null], ['sonnethi', false, 1, 'high']], 'each profile still reachable: the click filter and the ⌥-click use its name')
  assert.equal(g[3].origin, 'cn')
  assert.equal(g[1].live, false)
})

test('groupByModel: the effort sub-line is the share of the turns that ran at each effort, the biggest first', () => {
  const routes = [...ran(SONNET, 'medium', 'medium', 'medium', 'high', 'high', 'medium', 'medium', 'medium', 'medium', 'high'), ...ran('claude-opus-5-5', 'max')]
  const s = groupByModel(strip(), routes)[0]
  assert.deepEqual(s.effort, { basis: 'usage', text: 'medium 70% / high 30%', turns: 10 })
  assert.deepEqual(groupByModel(strip(), routes)[1].effort, { basis: 'usage', text: 'max 100%', turns: 1 })
})

test('groupByModel: a tie goes the lower effort first', () => {
  const s = groupByModel(strip(), ran(SONNET, 'high', 'low', 'high', 'low'))[0]
  assert.deepEqual(s.effort, { basis: 'usage', text: 'low 50% / high 50%', turns: 4 })
})

test('groupByModel: no turn read yet, the sub-line is the configured efforts and says so, never as usage', () => {
  const g = groupByModel(strip(), ran('claude-opus-5-5', 'high'))
  assert.deepEqual(g[0].effort, { basis: 'configured', text: 'configured: default / high', turns: 0 }, 'Sonnet: no turn of its own')
  assert.deepEqual(g[2].effort, { basis: 'configured', text: 'configured: low / high', turns: 0 })
  assert.deepEqual(g[3].effort, { basis: 'configured', text: 'configured: default', turns: 0 })
  assert.equal(g[1].effort?.basis, 'usage', 'Opus has a turn')
})

test('groupByModel: the headroom is the model’s, from its newest call', () => {
  const g = groupByModel(strip([], [{ model: SONNET, rate: { tokens_remaining: 50, tokens_limit: 100 } }]), [])
  assert.equal(g[0].headroom, '50% of tokens left')
  assert.equal(g[1].headroom, null)
})

const row = (kind: string, data: Record<string, unknown>, position = 1): LedgerEntry => ({ position, at_unix_ms: position, kind, session_id: null, turn_id: null, data } as unknown as LedgerEntry)

test('routeEfforts: the effort each routed turn’s request carried, from route.decided: sent when a hold kept it, else ran', () => {
  const rows = [
    row('route.decided', { model: SONNET, effort_ran: 'xhigh', effort_sent: 'high' }, 1),
    row('route.decided', { model: SONNET, effort_ran: 'medium' }, 2),
    row('route.decided', { model: SONNET, effort_ran: null }, 3),
    row('route.decided', { model: SONNET }, 4),
    row('provider.call', { model: SONNET, effort_ran: 'low' }, 5),
    row('route.decided', { effort_ran: 'low' }, 6),
    row('route.decided', { model: 'glm-5.3-flash', effort_ran: '' }, 7),
  ]
  assert.deepEqual(routeEfforts(rows), [{ model: SONNET, effort: 'high' }, { model: SONNET, effort: 'medium' }])
  assert.deepEqual(routeEfforts(rows, 1), [{ model: SONNET, effort: 'medium' }], 'the newest n')
})

test('the dock draws the grouped strip over the turns’ real efforts', () => {
  const src = readFileSync(new URL('../src/ops/Dock.tsx', import.meta.url), 'utf8')
  assert.match(src, /groupByModel\(modelsStrip\(/)
  assert.match(src, /routeEfforts\(/)
})
