// The Configuration controls (issue 17, step 5): each verb is pinned to its method and posture, every act that destroys asks,
// every verb has a control that names it, and the words and params the forms build are what the daemon reads.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import type { OntologyProposal } from '../src/protocol.gen/index.ts'
import { CONFIG_VERBS, DESTROYS, configVerb } from '../src/simple/config/verbs.ts'
import { LIVE } from '../src/simple/config/settings.ts'
import {
  absolute, acceptWords, backfillWords, eraseWords, heldOf, isDay, linesOf, needsName, personaChanges, projectParams, proposalName, restartWords, trustWords,
} from '../src/simple/config/controls/logic.ts'

const ROOT = new URL('../..', import.meta.url).pathname
const CONTROLS = join(ROOT, 'cockpit/src/simple/config/controls')
const sources = readdirSync(CONTROLS).filter((f) => f.endsWith('.tsx')).map((f) => [f, readFileSync(join(CONTROLS, f), 'utf8')] as const)
const protocol = readdirSync(join(ROOT, 'crates/theseus-protocol/src')).filter((f) => f.endsWith('.rs')).map((f) => readFileSync(join(ROOT, 'crates/theseus-protocol/src', f), 'utf8')).join('\n')

const EXPECT: Record<string, string> = {
  project_trust: 'project.trust|ask', project_untrust: 'project.untrust|arm', project_create: 'project.create|ask',
  standing_add: 'standing.add|ask', standing_keep: 'standing.keep|ask', standing_dismiss: 'standing.dismiss|do', standing_answer: 'standing.answer|arm',
  mcp_restart: 'mcp.restart|ask', index_rebuild: 'index.rebuild|ask', profile_release: 'profile.release|ask', profile_unpin: 'profile.unpin|ask',
  import_erase: 'import.erase|ask', judge_learn: 'judge.learn|ask', judge_prove: 'judge.prove|do', judge_backfill: 'judge.backfill|ask',
  persona_preview: 'persona.import|do', persona_import: 'persona.import|ask', persona_set: 'persona.set|do',
  kind_retire: 'ontology.kind.retire|ask', proposal_accept: 'ontology.proposal.accept|arm', proposal_reject: 'ontology.proposal.reject|do',
  initiative_up: 'initiative.rate|do', initiative_down: 'initiative.rate|do',
  self_pause: 'self.pause|do', self_unpause: 'self.pause|arm', self_resume: 'self.resume|ask',
}

test('each verb is pinned to its method and posture, and no verb is left unpinned', () => {
  assert.deepEqual(CONFIG_VERBS.map((v) => v.id).sort(), Object.keys(EXPECT).sort())
  for (const v of CONFIG_VERBS) assert.equal(`${v.methods.join(' ')}|${v.posture}`, EXPECT[v.id], v.id)
})

test('every method is a method of the protocol crate, and Google’s, which the daemon answers only to its command line, are not here', () => {
  for (const v of CONFIG_VERBS) for (const m of v.methods) assert.ok(protocol.includes(`"${m}"`), m)
  assert.deepEqual(CONFIG_VERBS.filter((v) => v.methods.some((m) => m.startsWith('google.'))), [])
})

test('what destroys, cannot be undone or interrupts something running asks first', () => {
  for (const id of DESTROYS) assert.equal(configVerb(id).posture, 'ask', id)
  assert.ok(DESTROYS.includes('import_erase') && DESTROYS.includes('index_rebuild') && DESTROYS.includes('kind_retire') && DESTROYS.includes('mcp_restart'))
})

test('every verb has a control that names it, and the erase wants the import’s name typed', () => {
  const text = sources.map(([, t]) => t).join('\n')
  for (const v of CONFIG_VERBS) assert.ok(text.includes(`verb="${v.id}"`), `${v.id} has no control`)
  const imports = sources.find(([f]) => f === 'Imports.tsx')![1]
  assert.match(imports, /verb="import_erase"[^>]*confirmWord=\{t\.tag\}/)
})

test('every panel of the tab says in a literal what its help is, and the tab keeps no timer of its own', () => {
  for (const [f, t] of sources) {
    for (const m of t.matchAll(/<Panel\b[^>]*>/g)) assert.match(m[0], /help="simple\.config\.\w+"/, f)
    assert.doesNotMatch(t, /setInterval|setTimeout/, f)
  }
})

test('each live row that points at the controls tab names a panel that is there, with the methods its verbs call', () => {
  const ids = new Set(readFileSync(join(ROOT, 'cockpit/src/simple/config/Controls.tsx'), 'utf8').matchAll(/at\('(\w+)'/g).map((m) => m[1]))
  const called = new Set(CONFIG_VERBS.flatMap((v) => v.methods))
  const rows = LIVE.filter((s) => 'tab' in s.at && s.at.tab === 'controls')
  assert.equal(rows.length, 11)
  for (const s of rows) {
    assert.ok('tab' in s.at && s.at.section && ids.has(s.at.section), s.id)
    for (const m of s.methods) assert.ok(called.has(m), `${s.id}: ${m}`)
  }
})

test('forms: lines, absolute paths, the project’s params, days', () => {
  assert.deepEqual(linesOf(' a \n\n b\n  \n'), ['a', 'b'])
  assert.ok(absolute('/home/x/repo') && absolute('C:\\code') && !absolute('repo') && !absolute(' '))
  assert.deepEqual(projectParams(' T ', '', 'one\ntwo'), { title: 'T', acceptance: ['one', 'two'], milestones: [] })
  assert.deepEqual(projectParams('T', ' why ', ''), { title: 'T', objective: 'why', acceptance: [], milestones: [] })
  assert.ok(isDay('2026-10-01') && !isDay('10/01') && !isDay(''))
})

test('the words of an act say exactly what will happen', () => {
  assert.match(trustWords('/r/x'), /AGENTS\.md and CLAUDE\.md/)
  assert.match(eraseWords({ tag: 'old', sessions: 1, erased: 0 }), /still holds: 1 conversation and the memory entries in them.*cannot be undone/)
  assert.match(eraseWords({ tag: 'old', sessions: 1500, erased: 300 }), /1,200 conversations and/, 'the erased ones are not counted again')
  assert.equal(heldOf({ sessions: 5, erased: 9 }), 0)
  assert.match(restartWords({ name: 'docs', state: 'running', calls: 3 }), /“docs” \(it is running\).*cut off/)
  assert.match(backfillWords('loop.v1', '2026-10-01'), /costs money/)
})

test('a persona’s changes are only what differs, an emptied field clears, and nothing changed sends nothing', () => {
  const p = { name: 'Jev', emoji: '🦊', card: undefined, tts_voice: undefined, initiative: 'normal' }
  assert.equal(personaChanges(p, 'jev', {}), null)
  assert.equal(personaChanges(p, 'jev', { name: ' Jev ', initiative: 'normal' }), null)
  assert.deepEqual(personaChanges(p, 'jev', { name: 'Jevvy', initiative: 'quiet' }), { key: 'jev', name: 'Jevvy', initiative: 'quiet' })
  assert.deepEqual(personaChanges(p, 'jev', { emoji: '' }), { key: 'jev', emoji: '' })
})

test('a proposal with no name needs one typed before it can be accepted', () => {
  const base = { judgment: 'j1', session_id: 'ses_a', new_topic: false, confidence: 0.9, band: 'high', at_ms: 1 } as OntologyProposal
  assert.equal(needsName({ ...base, topic_name: 'Kitchen' }), false)
  assert.equal(needsName({ ...base, new_topic: true }), true)
  assert.equal(needsName({ ...base, new_topic: true, topic: 'top_1' }), false)
  assert.equal(proposalName({ ...base, topic_name: 'Kitchen' }), 'Kitchen')
  assert.match(acceptWords({ ...base, session_title: 'Cooking' }, 'Kitchen'), /“Kitchen” for Cooking/)
})
