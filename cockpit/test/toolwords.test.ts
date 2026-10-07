// A tool call in words (`src/lib/toolwords.ts`), run by `npm test`: the summaries, the approval previews, the L1
// pill's title, and a result's status (theseus-vm3n.6). Invented paths.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { byteWords, callSummary, decisionWords, diffLines, l1Words, looksLikeDiff, previewOf, resultWords, wireToName } from '../src/lib/toolwords.ts'

test('a call reads in the terms of its tool', () => {
  assert.equal(callSummary('proc.run', { argv: ['cargo', 'test'], cwd: '/w/app' }), 'cargo test   (in /w/app)')
  assert.equal(callSummary('fs.read', { path: 'a.txt', offset: 5, limit: 20 }), 'a.txt from line 5 (20 lines)')
  assert.equal(callSummary('fs.write', { path: 'b.txt', content: 'x'.repeat(2048) }), 'b.txt (2.0 KB)')
  assert.equal(callSummary('fs.grep', { pattern: 'todo', path: 'src', glob: '*.rs' }), '/todo/ in src (*.rs)')
  assert.equal(callSummary('web.search', { query: 'tide tables', count: 3 }), '"tide tables" (3 results)')
  assert.equal(callSummary('mystery.tool', { a: 1 }), '{"a":1}')
  assert.equal(wireToName('fs_read'), 'fs.read')
  assert.equal(byteWords(3 * 1024 * 1024), '3.0 MB')
})

test('an edit previews as a diff, a command as it would be typed, and an unknown tool has none', () => {
  const p = previewOf('fs.edit', { path: 'c.rs', old_string: 'a\nb', new_string: 'c' })
  assert.equal(p?.kind, 'diff')
  assert.equal(p?.text, '--- c.rs\n+++ c.rs\n@@ edit @@\n-a\n-b\n+c')
  assert.deepEqual(diffLines(p!.text).map((l) => l.kind), ['meta', 'meta', 'hunk', 'del', 'del', 'add'])
  assert.equal(previewOf('proc.run', { argv: ['echo', 'a b'], timeout_secs: 9 })?.text, "$ echo 'a b'\n  (timeout 9 s)")
  assert.equal(previewOf('fs.write', { path: 'd.txt', content: 'hi' })?.caption, 'd.txt · 2 B')
  assert.equal(previewOf('aws.call', { op: 'x' }), null)
  assert.equal(looksLikeDiff('@@ -1 +1 @@\n-a\n+b'), true)
  assert.equal(looksLikeDiff('plain text'), false)
})

test('the L1 pill says what the job may reach', () => {
  assert.match(l1Words(['example.test']), /egress: example\.test; what it brings back is outside text/)
  assert.match(l1Words([]), /no network/)
})

test('a result says it never ran, was stopped, or exited', () => {
  assert.deepEqual(resultWords({ status: 'declined' }), { status: 'not run', stoppedBy: null, exit: null, tone: 'warn' })
  assert.deepEqual(resultWords({ status: 'cancelled', meta: { stopped_by: 'ada' } }), { status: 'cancelled', stoppedBy: 'ada', exit: null, tone: 'muted' })
  assert.deepEqual(resultWords({ status: 'ok', meta: { exit_code: 0 } }), { status: 'ok', stoppedBy: null, exit: 0, tone: 'ok' })
  assert.equal(resultWords({ status: 'cancelled', meta: {} }).stoppedBy, null)
})

test('an approval leads with what it decides, in the daemon\'s own words, the path as the call named it', () => {
  const w = decisionWords('fs.write', { path: 'harbour/log.md', content: 'x' },
    'create /w/projects/harbour/log.md (38 bytes): fs.write — approve ([policy.tools] "fs.write" = approve)')
  assert.deepEqual(w, { what: 'create harbour/log.md, 38 bytes', why: 'fs.write — approve ([policy.tools] "fs.write" = approve)' })
  assert.equal(decisionWords('fs.write', { path: 'a.txt', content: 'x' }, 'replace /w/a.txt (1,204 bytes): fs.write — approve (floor: the state dir)').what,
    'replace a.txt, 1,204 bytes')
  assert.equal(decisionWords('fs.edit', { path: '/abs/c.rs' }, 'edit /abs/c.rs (all occurrences): fs.edit — approve (x)').what, 'edit /abs/c.rs (all occurrences)')
  assert.equal(decisionWords('proc.run', { argv: ['cargo', 'test'] }, 'run `cargo test` in /w/app: proc.run — approve (layer 1: a)').what, 'run `cargo test` in /w/app')
  // A reason with no plan in it: the call's own summary leads, and the reason is the why.
  assert.deepEqual(decisionWords('proc.run', { argv: ['ls'] }, 'the session read web text'), { what: 'proc.run ls', why: 'the session read web text' })
})
