import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { replySpeaker, REPLY_NAME } from '../src/lib/speaker.ts'

test('a reply is Theseus, with its model as metadata', () => {
  assert.deepEqual(replySpeaker('claude-sonnet-5-5'), { name: 'Theseus', model: 'sonnet-5-5' })
  assert.deepEqual(replySpeaker('glm-5.3-flash'), { name: 'Theseus', model: 'glm-5.3-flash' })
  assert.deepEqual(replySpeaker(undefined), { name: 'Theseus', model: '' })
  assert.equal(REPLY_NAME, 'Theseus')
})

test("the transcript's reply card labels the speaker through it, and no longer prints the bare model id", () => {
  const src = readFileSync(new URL('../src/components/Transcript.tsx', import.meta.url), 'utf8')
  assert.match(src, /replySpeaker\(d\.model\)/)
  assert.doesNotMatch(src, /\{d\.model && <span className="num text-model">\{d\.model\}<\/span>\}/)
})
