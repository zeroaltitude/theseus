// The ship's log's marks (`src/lib/marks.ts`), run by `npm test` with node's own runner: no dependency.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { marksOf } from '../src/lib/marks.ts'

const row = (at: number, kind: string, data: Record<string, unknown> = {}) =>
  ({ at_unix_ms: at, kind, session_id: null, turn_id: null, data }) as any

const build = (version: string, commit?: string) => ({ build: commit ? { version, commit } : { version } })
const A = 'aaaaaaa0123456789abcdef0123456789abcdef0'
const B = 'bbbbbbb0123456789abcdef0123456789abcdef0'

const starts = (rows: any[]) => marksOf(rows).filter((m) => m.kind !== 'stop').map((m) => m.kind)

test('a restart of the same build is a start, and a new build is an install (theseus-9o5n)', () => {
  const rows = [
    row(1, 'server.started', build('0.1.0', A)),
    row(2, 'server.stopping', { signal: 'SIGTERM' }),
    row(3, 'server.started', build('0.1.0', A)),
    row(4, 'server.stopping', { signal: 'SIGTERM' }),
    row(5, 'server.started', build('0.1.0', B)),
  ]
  assert.deepEqual(starts(rows), ['start', 'start', 'install'])
  const install = marksOf(rows).at(-1)!
  assert.equal(install.at, 5)
  assert.match(install.label, /0\.1\.0 \(bbbbbbb\), after 0\.1\.0 \(aaaaaaa\), after a stop by SIGTERM/)
})

test('a new version of the same commit is an install too', () => {
  assert.deepEqual(starts([row(1, 'server.started', build('0.1.0', A)), row(2, 'server.started', build('0.2.0', A))]), ['start', 'install'])
})

test('the first start is a start, whatever its build', () => {
  assert.deepEqual(starts([row(1, 'server.started', build('0.1.0', A))]), ['start'])
})

test('a build after starts that named none is an install; starts that all name none are not', () => {
  assert.deepEqual(starts([row(1, 'server.started'), row(2, 'server.stopping'), row(3, 'server.started')]), ['start', 'start'])
  const rows = [row(1, 'server.started'), row(2, 'server.stopping'), row(3, 'server.started', build('0.1.0', A))]
  assert.deepEqual(starts(rows), ['start', 'install'])
  assert.match(marksOf(rows).at(-1)!.label, /after one that named none/)
})

test('a crash restart of the same build stays a crash; a crash onto a new build is an install that says so', () => {
  assert.deepEqual(starts([row(1, 'server.started', build('0.1.0', A)), row(2, 'server.started', build('0.1.0', A))]), ['start', 'crash'])
  const rows = [row(1, 'server.started', build('0.1.0', A)), row(2, 'server.started', build('0.1.0', B))]
  assert.deepEqual(starts(rows), ['start', 'install'])
  assert.match(marksOf(rows).at(-1)!.label, /no stop before it/)
})
