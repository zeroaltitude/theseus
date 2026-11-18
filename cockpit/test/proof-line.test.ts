// The card's line about the second proof and whether it shows a code field (`src/lib/answers.ts`), run by `npm test`.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { goesThrough, needsCode, proofLine } from '../src/lib/answers.ts'

const card = (extra: any) => ({ correlation_id: 'act_1', needs_proof: true, ...extra }) as any

test('an open window of the place covers it: Approve goes through, there is no code field, and the daemon’s words stand whole', () => {
  const c = card({ proof: { code_needed: false, line: 'second proof: given here 3 minutes ago, good for 23 hours more' } })
  assert.equal(proofLine(c), 'second proof: given here 3 minutes ago, good for 23 hours more')
  assert.equal(goesThrough(c), true)
  assert.equal(needsCode(c), false)
})

test('with no window, or one that takes a fresh code every time, the field is shown and the daemon’s words stand', () => {
  const c = card({ proof: { code_needed: true, line: 'second proof: needed, enter the code from your authenticator app' } })
  assert.equal(proofLine(c), 'second proof: needed, enter the code from your authenticator app')
  assert.equal(needsCode(c), true)
  assert.equal(goesThrough(c), false)
})

test('a question that asks for no proof says nothing and shows no field', () => {
  const c = card({ needs_proof: false })
  assert.equal(proofLine(c), null)
  assert.equal(needsCode(c), false)
  assert.equal(goesThrough(c), false)
})
