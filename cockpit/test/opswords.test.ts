// The Operations page's nouns in the "?" words (theseus-0zraw): the explainers say conversation, thread, plan, step,
// task and job as the owner settled them, and carry none of the internal words a person has never met.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { EXPLAINERS, PARTS } from '../src/lib/explainers.ts'

/** Words no owner-facing line may use, each with the entries that may, and why the screen shows it. */
const BANNED = ['pack', 'packs', 'rung', 'rungs', 'task record', 'task records', 'task graph', 'plan item', 'plan items']
const ALLOWED: Record<string, string> = {}

const STATE_WORDS = ['needs you', 'working', 'to do', 'paused·queued', 'scheduled today', 'at rest', 'done']

const parts = Object.entries(EXPLAINERS).flatMap(([id, e]) => PARTS.map(([k]) => ({ id, k, text: e[k] })))
const whole = (w: string) => new RegExp(`(?<![\\p{L}\\p{N}_])${w}(?![\\p{L}\\p{N}_])`, 'iu')

test('no explainer uses a word a person has never met', () => {
  const found: string[] = []
  for (const { id, k, text } of parts) for (const w of BANNED) if (whole(w).test(text) && !(`${id}.${k}` in ALLOWED)) found.push(`${id}.${k}: ${w}`)
  assert.deepEqual(found, [])
})

test('the state words are the seven buckets, and nothing says “still today”', () => {
  for (const { id, k, text } of parts) assert.ok(!/still today/i.test(text), `${id}.${k} says “still today”`)
  const board = EXPLAINERS['ops.board'].read
  assert.match(board, /scheduled today/)
  for (const w of ['Needs you', 'Working', 'Paused·queued', 'Scheduled', 'Done']) assert.ok(board.includes(w), `ops.board names ${w}`)
  const graph = EXPLAINERS['ops.graph'].read
  for (const w of STATE_WORDS) assert.ok(graph.includes(w), `ops.graph names the state ${w}`)
})

test('ops.graph and ops.words say the glossary’s nouns', () => {
  const g = Object.values(EXPLAINERS['ops.graph']).join(' ')
  for (const w of ['thread', 'swimlane', 'plan', 'step', 'task', 'job', 'n of m']) assert.match(g, whole(w.replace(' ', '\\s+')), `ops.graph says ${w}`)
  const w = Object.values(EXPLAINERS['ops.words']).join(' ')
  for (const n of ['conversation', 'thread', 'plan', 'step', 'task', 'job']) assert.match(w, whole(n), `ops.words says ${n}`)
})

test('the Operations page’s entries use the new nouns for stored work', () => {
  for (const id of ['ops.board', 'ops.queue', 'ops.inspector', 'tasks.new', 'actions.tasks', 'actions.taskgraph', 'threads.list', 'threads.work', 'fleet.graph']) {
    const t = Object.values(EXPLAINERS[id]).join(' ')
    assert.ok(!/task record|task graph|plan item/i.test(t), id)
  }
  assert.match(EXPLAINERS['actions.taskgraph'].goal, /plans and steps/)
  assert.match(EXPLAINERS['ops.board'].read, /a plan, a step, a task/)
})

test('the Actions panel of stored work is headed “Plans and steps”', () => {
  const src = readFileSync(new URL('../src/components/TaskGraph.tsx', import.meta.url), 'utf8')
  const titles = [...src.matchAll(/help="actions\.taskgraph" title=\{<>([^<]*)<\/>\}/g)].map((m) => m[1])
  assert.equal(titles.length, 2)
  for (const t of titles) { assert.match(t, /^Plans and steps · /); assert.ok(!/Task graph/i.test(t)) }
})

test('the Graph’s help buttons are mounted', () => {
  const bar = readFileSync(new URL('../src/ops/CommandBar.tsx', import.meta.url), 'utf8')
  assert.match(bar, /<HelpButton id="ops\.graph"/)
  assert.match(bar, /<HelpButton id="ops\.words"/)
})
