// The add-a-kind form's pure half (theseus-s430x, R7c): `src/lib/ontokinds.ts` builds the params of `ontology.kind.add` from
// the form's fields, mirrors the daemon's own checks (so a refusal is read before it is sent), and words the confirm with
// the exact JSON it sends. Run by `npm test` with node alone.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { buildKindAdd, EXAMPLE_MAX, EXAMPLES_MAX, KIND_ORIGINS, kindAddAsk, NO_KIND_FORM, type KindForm } from '../src/lib/ontokinds.ts'
import { EXPLAINERS } from '../src/lib/explainers.ts'

const form = (over: Partial<KindForm> = {}): KindForm => ({ ...NO_KIND_FORM, name: 'expertise', ...over })
const taken = ['channel', 'topic']

test('a name alone builds the smallest call: the assigners as the form shows them and an empty description, the rest left to the daemon', () => {
  const r = buildKindAdd(form(), taken)
  assert.deepEqual(r.errors, {})
  assert.deepEqual(r.params, { name: 'expertise', description: '', assigned_by: ['operator', 'jev'] })
  assert.deepEqual(buildKindAdd(form({ assignedBy: [] }), taken).params, { name: 'expertise', description: '' })
})

test('every field set is sent, in the protocol’s order, trimmed', () => {
  const r = buildKindAdd(form({
    name: ' expertise ', description: ' what the owner is good at ', assignedBy: ['jev', 'operator'], perSession: ' 2 ', parent: 'topic',
    precedence: '45', rule: 'intent_line', examples: ' rust \n\n async  \n',
  }), taken)
  assert.deepEqual(r.errors, {})
  assert.deepEqual(r.params, {
    name: 'expertise', description: 'what the owner is good at', assigned_by: ['jev', 'operator'], per_session: '2', parent: 'topic',
    precedence: 45, rule: 'intent_line', examples: ['rust', 'async'],
  })
  assert.deepEqual(Object.keys(r.params!), ['name', 'description', 'assigned_by', 'per_session', 'parent', 'precedence', 'rule', 'examples'])
})

test('a name is a lowercase letter, then up to 31 of lowercase letters, digits, underscore or hyphen', () => {
  for (const bad of ['', '   ', 'Expertise', '1kind', 'two words', 'kind!', 'x'.repeat(33), '_kind', '-kind'])
    assert.ok(buildKindAdd(form({ name: bad }), taken).errors.name, `${JSON.stringify(bad)} is refused`)
  for (const ok of ['a', 'x'.repeat(32), 'k1_a-b', 'expertise'])
    assert.equal(buildKindAdd(form({ name: ok }), taken).errors.name, undefined, `${JSON.stringify(ok)} is allowed`)
})

test('a kind already in the table is refused before it is sent, and the call is not built', () => {
  const r = buildKindAdd(form({ name: 'topic' }), taken)
  assert.match(r.errors.name ?? '', /already/)
  assert.equal(r.params, null)
})

test('per session is a positive whole number or many', () => {
  for (const bad of ['0', '-1', '1.5', 'two', '1e3', '00', ' 0 '])
    assert.ok(buildKindAdd(form({ perSession: bad }), taken).errors.perSession, `${JSON.stringify(bad)} is refused`)
  for (const ok of ['1', '3', '12', 'many', ' many '])
    assert.equal(buildKindAdd(form({ perSession: ok }), taken).errors.perSession, undefined, `${JSON.stringify(ok)} is allowed`)
  assert.equal(buildKindAdd(form({ perSession: 'many' }), taken).params?.per_session, 'many')
})

test('the rule is chain or intent_line, and left empty it is the daemon’s default', () => {
  assert.ok(buildKindAdd(form({ rule: 'both' }), taken).errors.rule)
  assert.equal(buildKindAdd(form({ rule: 'chain' }), taken).params?.rule, 'chain')
  assert.equal('rule' in buildKindAdd(form({ rule: '' }), taken).params!, false)
})

test('who may assign it: the operator and Jev by default, never the transport, no repeats', () => {
  assert.deepEqual(KIND_ORIGINS, ['operator', 'import', 'jev', 'sweep', 'dream'])
  assert.equal(NO_KIND_FORM.assignedBy.join(), 'operator,jev')
  assert.equal('assigned_by' in buildKindAdd(form({ assignedBy: [] }), taken).params!, false, 'none picked: the daemon’s own default')
  assert.ok(buildKindAdd(form({ assignedBy: ['transport'] }), taken).errors.assignedBy)
  assert.ok(buildKindAdd(form({ assignedBy: ['jev', 'jev'] }), taken).errors.assignedBy)
  assert.ok(buildKindAdd(form({ assignedBy: ['someone'] }), taken).errors.assignedBy)
})

test('the parent is a kind’s name; precedence a whole number', () => {
  assert.ok(buildKindAdd(form({ parent: 'Topic' }), taken).errors.parent)
  assert.equal(buildKindAdd(form({ parent: ' ' }), taken).errors.parent, undefined)
  assert.equal('parent' in buildKindAdd(form({ parent: ' ' }), taken).params!, false)
  for (const bad of ['-1', '1.5', 'first', '4294967296'])
    assert.ok(buildKindAdd(form({ precedence: bad }), taken).errors.precedence, `${JSON.stringify(bad)} is refused`)
  assert.equal(buildKindAdd(form({ precedence: '0' }), taken).params?.precedence, 0)
})

test('at most five examples of up to a hundred characters, one a line, blank lines dropped', () => {
  assert.equal(EXAMPLES_MAX, 5)
  assert.equal(EXAMPLE_MAX, 100)
  assert.equal(buildKindAdd(form({ examples: 'a\nb\nc\nd\ne' }), taken).errors.examples, undefined)
  assert.ok(buildKindAdd(form({ examples: 'a\nb\nc\nd\ne\nf' }), taken).errors.examples)
  assert.ok(buildKindAdd(form({ examples: 'x'.repeat(101) }), taken).errors.examples)
  assert.equal(buildKindAdd(form({ examples: 'x'.repeat(100) }), taken).errors.examples, undefined)
  assert.ok(buildKindAdd(form({ examples: 'a\u0007b' }), taken).errors.examples, 'a control character')
})

test('a description over two thousand characters is refused', () => {
  assert.ok(buildKindAdd(form({ description: 'x'.repeat(2001) }), taken).errors.description)
  assert.equal(buildKindAdd(form({ description: 'x'.repeat(2000) }), taken).errors.description, undefined)
})

test('errors name every field at fault, and the call is built only when there are none', () => {
  const r = buildKindAdd(form({ name: 'Bad', perSession: '0', rule: 'x' }), taken)
  assert.deepEqual(Object.keys(r.errors).sort(), ['name', 'perSession', 'rule'])
  assert.equal(r.params, null)
})

test('the confirm carries the exact JSON of the params it sends', () => {
  const p = buildKindAdd(form({ description: 'what the owner knows', perSession: 'many', examples: 'rust' }), taken).params!
  const ask = kindAddAsk(p)
  assert.match(ask, /^Add the kind “expertise” \(ontology\.kind\.add\)\?/)
  assert.ok(ask.includes(JSON.stringify(p, null, 2)), 'the params as sent, whole')
  assert.match(ask, /interpreted/)
})

test('the form is a panel with an explainer, wired to the write through the confirmed hook', () => {
  assert.ok(EXPLAINERS['ontology.kindadd'], 'an explainer for its panel')
  const v = readFileSync(new URL('../src/components/OntologyKindAdd.tsx', import.meta.url), 'utf8')
  assert.match(v, /help="ontology\.kindadd"/)
  assert.match(v, /'ontology\.kind\.add'/)
  assert.match(v, /kindAddAsk\(/)
  assert.match(v, /buildKindAdd\(/)
  const o = readFileSync(new URL('../src/views/Ontology.tsx', import.meta.url), 'utf8')
  assert.match(o, /<AddKind /)
})
