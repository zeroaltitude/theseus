// Health's binary in words (`src/lib/binary.ts`), run by `npm test`: the CLI's `binary:` line, in each state.
// Invented paths.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { BinaryStatus } from '@protocol'
import { binaryLine, binaryTone } from '../src/lib/binary.ts'

const b = (state: string, detail = ''): BinaryStatus => ({ path: '/invented/bin/theseusd', state, detail })

test('jobs that can write the binary are a fault, in the CLI\'s words', () => {
  const s = b('jobs_can_write', 'its directory is writable by this daemon\'s user')
  assert.equal(binaryTone(s), 'fault')
  assert.equal(
    binaryLine(s),
    'binary: JOBS CAN WRITE /invented/bin/theseusd: its directory is writable by this daemon\'s user (run the builder as its own user: theseusd install --separate)',
  )
})

test('a binary that could not be read waits, and one jobs cannot write is quiet', () => {
  assert.equal(binaryTone(b('unknown', 'no current_exe')), 'wait')
  assert.equal(binaryLine(b('unknown', 'no current_exe')), 'binary: unknown: no current_exe')
  assert.equal(binaryTone(b('ok')), 'ok')
  assert.equal(binaryLine(b('ok')), 'binary: ok: jobs cannot write /invented/bin/theseusd')
})

test('a daemon that does not report it is idle', () => {
  assert.equal(binaryTone(undefined), 'idle')
  assert.equal(binaryLine(undefined), 'binary: not reported by this daemon')
})
