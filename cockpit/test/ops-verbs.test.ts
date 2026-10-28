// The verb table (theseus-jid7t.6): each row maps to its method and posture, every method is the protocol's, and every
// acting key of the key table names a verb.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { KEYS } from '../src/ops/keys.ts'
import { KEY_VERB, VERBS, verbById, verbsOn } from '../src/ops/verbs.ts'

/** The owner's table, row by row: `id: methods | posture`. */
const EXPECT: Record<string, string> = {
  open: 'nav|do', inspect: 'session.history node.list turn.inspect work.peek thread.show task.list|do', join: 'turn.submit|do', steer: 'turn.submit|do', queue: 'turn.submit|do',
  task_pause: 'task.pause task.resume|do', thread_pause: 'thread.pause thread.resume thread.done thread.reopen|do', routine_pause: 'routine.pause routine.resume|do',
  routine_run: 'routine.run_now|ask', routine_delete: 'routine.delete|ask', wake_snooze: 'wake.snooze|do', wake_pause: 'wake.pause wake.resume|do', wake_cancel: 'wake.cancel|ask',
  watch_cancel: 'wake.cancel|ask', stop: 'execution.stop|arm', stop_all: 'execution.stop|ask', cancel: 'task.cancel|arm', not_this: 'task.not_this|arm', job_stop: 'job.stop|arm', answer: 'work.answer|floor',
  trust: 'policy.trust|ask', unlabel: 'content.unlabel|ask', retry: 'task.retry|do', dismiss: 'execution.dismiss|do', new_task: 'task.start|do', thread_new: 'thread.new|do', thread_edit: 'thread.edit|do',
  thread_seal: 'thread.seal thread.unseal|ask', thread_merge: 'thread.merge thread.split|ask', thread_link: 'thread.link|do', thread_move: 'thread.move|do', attach: 'project.attach|do',
  mode: 'session.mode|do', mode_auto: 'session.mode|ask', nudge: 'session.nudge|do', recompile: 'session.recompile|do', fork: 'session.fork|do', retire: 'session.retire|ask',
  reopen: 'session.reopen|do', model_turn: 'turn.submit|do', model_sticky: 'session.profile|do', profile_live: 'profile.use|ask', rehome: 'task.rehome|ask', reschedule: 'task.reschedule|do',
}

test('each verb of the table maps to the owner’s method and posture, and nothing is added or missing', () => {
  assert.deepEqual(VERBS.map((v) => v.id).sort(), Object.keys(EXPECT).sort())
  for (const v of VERBS) assert.equal(`${v.methods.join(' ')}|${v.posture}`, EXPECT[v.id], v.id)
  assert.equal(new Set(VERBS.map((v) => v.id)).size, VERBS.length)
})

test('the reversible acts offer Undo: pause, snooze, the sticky model, reschedule', () => {
  for (const id of ['task_pause', 'wake_snooze', 'wake_pause', 'model_sticky', 'reschedule']) assert.equal(verbById(id)?.undo, true, id)
  for (const id of ['cancel', 'wake_cancel', 'retire', 'rehome']) assert.ok(!verbById(id)?.undo, id)
})

test('every method is one the protocol has, except the one it lacks, which the table says', () => {
  const proto = ['lib.rs', 'ledger.rs'].map((f) => readFileSync(new URL(`../../crates/theseus-protocol/src/${f}`, import.meta.url), 'utf8')).join('\n')
  const lacks: string[] = []
  // Since p2f-ops-act the daemon has job.stop, so no row lacks its method: the list is empty and a row that lacks one fails here.
  for (const v of VERBS) for (const m of v.methods) {
    if (m === 'nav') continue
    if (!proto.includes(`"${m}"`)) lacks.push(m)
    assert.ok(proto.includes(`"${m}"`), `${v.id}: ${m} is not the protocol's`)
  }
  assert.deepEqual(lacks, [])
})

test('every acting key names a verb of the table, and the palette reads the same table', () => {
  for (const k of KEYS.filter((x) => x.act)) assert.ok(verbById(KEY_VERB[k.verb] ?? ''), `${k.verb} names no verb`)
  for (const id of Object.values(KEY_VERB)) assert.ok(verbById(id), id)
  assert.ok(verbsOn('wake').every((v) => v.on.includes('wake')) && verbsOn('wake').length >= 3)
  assert.ok(verbsOn('job').some((v) => v.id === 'job_stop'))
})

test('the Inspector’s Stop on a job calls job.stop through useOpsAct, armed by a first press, and says the answer’s words', () => {
  const src = readFileSync(new URL('../src/ops/Inspector.tsx', import.meta.url), 'utf8')
  assert.match(src, /post<JobStopResult>\('job_stop', 'job\.stop', \{ id: r\.id \}\)/)
  assert.match(src, /\.then\(\(a\) => say\(a\.words\)\)/)
  assert.match(src, /if \(!armed\) \{ setArmed\(true\); return \}/)
  assert.doesNotMatch(src, /onClick=\{\(\) => \{\}\}/)
})
