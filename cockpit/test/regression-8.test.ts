import { test } from 'node:test'
import assert from 'node:assert/strict'
import * as attach from '../src/ops/attach.ts'
test('oversized attachments are refused without reading and read errors are visible', async () => {
  assert.equal(typeof attach.readAttachment, 'function')
  let reads = 0
  const large = { name: 'large.bin', size: attach.MAX_ATTACH_BYTES + 1, arrayBuffer: async () => { reads++; throw new Error('must not read') } }
  assert.match((await attach.readAttachment(large)).not_read!, /32 MiB/)
  assert.equal(reads, 0)
  const bad = { ...large, size: 1, arrayBuffer: async () => { throw new Error('unreadable') } }
  assert.match((await attach.readAttachment(bad)).not_read!, /unreadable/)
  const good = { name: 'small.txt', size: 2, arrayBuffer: async () => new TextEncoder().encode('ok').buffer }
  assert.equal((await attach.readAttachment(good)).text, 'ok')
})
