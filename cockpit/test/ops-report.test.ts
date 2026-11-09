// The inspector's report field says what it holds (theseus-emqbm). It showed the first line of a task's report under the
// bare word `report`, so the owner took one line for the whole thing and "could not find a report". The label now
// says which it is, a long report opens in full, and the transcript is a link beside `open`.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { boardRows } from '../src/ops/board.ts'
import { cardFacts, reportFacts } from '../src/ops/facts.ts'
import { reportLineOf, reportTextOf } from '../src/lib/taskboard.ts'
import { board, NOW } from './ops-fixtures.ts'

const LINE = 'Forty moorings, six free.'
const LONG = `${LINE}\n\nThe east quay holds twenty, the west fourteen, and the spit six.\n\nNext: the buoys.`

test('a report that is one line is labelled complete', () => {
  assert.deepEqual(reportFacts(LINE, LINE), { label: 'report (complete)', line: LINE, more: false })
  assert.equal(reportFacts(LINE, `  ${LINE}\n\n`)?.label, 'report (complete)', 'trailing blank lines are not more report')
})

test('a longer report says it is only the first line, and offers the whole', () => {
  assert.deepEqual(reportFacts(LINE, LONG), { label: 'report (first line)', line: LINE, more: true, full: LONG })
})

test('a report not read yet, or with no text in its session, is never called complete', () => {
  assert.deepEqual(reportFacts(LINE, undefined), { label: 'report (first line)', line: LINE, more: false })
  assert.deepEqual(reportFacts(LINE, null), { label: 'report (first line)', line: LINE, more: false })
  assert.equal(reportFacts(undefined, LONG), undefined, 'no line, no field: the card keeps its "no report line"')
})

test('a done task’s card links its deck and its transcript beside open, and names no forum post it cannot reach', () => {
  const rows = new Map(boardRows({ ...board(), now: NOW }).map((r) => [r.id, r]))
  const done = cardFacts(rows.get('ses_t4')!, NOW)
  assert.deepEqual(done.links, [
    { label: 'open', href: '/session/ses_t4', title: 'its deck' },
    { label: 'transcript', href: '/session/ses_t4', title: 'everything it said and did, in its deck' },
  ])
  const wake = cardFacts(rows.get('wak_r1')!, NOW)
  assert.ok(wake.links?.every((l) => l.label !== 'transcript'), 'a wake has no transcript of its own')
})

test('a task whose view carries a forum address links it as the third link, and a task without one has two', () => {
  const url = 'https://discord.com/channels/4242/777'
  const row = new Map(boardRows({ ...board(), now: NOW }).map((r) => [r.id, r])).get('ses_t4')!
  const linked = cardFacts({ ...row, view: { ...row.view!, forum_url: url } }, NOW)
  assert.equal(linked.links.length, 3)
  assert.deepEqual(linked.links[2], { label: 'on Discord', href: url, title: 'its forum post, or its thread when it has none', external: true })
  assert.deepEqual(linked.links.slice(0, 2).map((l) => l.label), ['open', 'transcript'])
  assert.equal(cardFacts(row, NOW).links.length, 2, 'no field, no claim')
})

test('the whole report is the last message the task ended with, of which the card’s line is the first', () => {
  const nodes = [
    { kind: 'assistant_message', text: 'Working on it.' },
    { kind: 'user_message', text: 'and the buoys?' },
    { kind: 'assistant_message', text: `\n${LONG}\n` },
    { kind: 'tool_call', text: 'ignored' },
  ]
  assert.equal(reportTextOf(nodes), LONG)
  assert.equal(reportLineOf(nodes), LINE, 'the line the card shows is the first line of that text')
  assert.equal(reportTextOf([{ kind: 'user_message', text: 'hello' }]), undefined)
  assert.equal(reportTextOf(undefined), undefined)
})

test('the inspector draws the labelled report, its full-report toggle and every link of the card', () => {
  const src = readFileSync(new URL('../src/ops/Inspector.tsx', import.meta.url), 'utf8')
  assert.match(src, /<ReportField r=\{r\} f=\{f\} \/>/, 'the report field')
  assert.match(src, /\{rf\.label\}/, 'under the label reportFacts gives it')
  assert.match(src, /'full report'/, 'with the whole report one click away')
  assert.match(src, /f\.links\.map\(/, 'and the card’s links, not one hard-wired deck link')
  assert.doesNotMatch(src, /\{f\.report && <>/, 'the card’s own line is no longer drawn under a bare `report` label')
})

test('a line from another source than the whole never fronts it: the report’s own first line is shown', () => {
  const reason = 'failed: out of budget'
  assert.deepEqual(reportFacts(reason, LONG), { label: 'report (first line)', line: LINE, more: true, full: LONG })
  assert.deepEqual(reportFacts(reason, LINE), { label: 'report (complete)', line: LINE, more: false })
})
