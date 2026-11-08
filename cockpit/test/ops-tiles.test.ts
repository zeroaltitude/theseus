// The command bar's tiles are one single-select filter, and NEEDS YOU counts what the panel lists (theseus-emqbm).
// The owner clicked WORKING and NEEDS YOU stayed lit, could not turn it off, and read 1 beside a panel that said
// "0 asks · nothing waits for you". The filter held one bucket all along: the lit look was the attention glow, drawn
// as the same ring a selection wears. And the tile counted a blocked record that no queue item names.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { ConfirmRequest, ThreadAsk } from '../src/protocol.gen/index.ts'
import { BUCKETS, type Bucket } from '../src/ops/address.ts'
import { allCounters, boardRows, listedCards, needsOf, unaskedCards } from '../src/ops/board.ts'
import { waitingFacts } from '../src/ops/facts.ts'
import { queueOf } from '../src/ops/queue.ts'
import { tileLook, toggleBucket } from '../src/ops/tiles.ts'
import { confirm, HOUR, info, MIN, NOW, record, view } from './ops-fixtures.ts'

const read = (p: string) => readFileSync(new URL(`../src/${p}`, import.meta.url), 'utf8')

test('choosing a tile replaces the one chosen, and choosing the chosen one clears it', () => {
  const two = toggleBucket({ bucket: 'needs', thread: 'harbour-survey' }, 'working')
  assert.deepEqual(two, { bucket: 'working', thread: 'harbour-survey' }, 'WORKING replaces NEEDS YOU; the other chips stay')
  const cleared = toggleBucket(two, 'working')
  assert.deepEqual(cleared, { thread: 'harbour-survey' })
  assert.ok(!('bucket' in cleared), 'the key is gone, not set to nothing')
  assert.deepEqual(toggleBucket({}, 'needs'), { bucket: 'needs' })
  assert.deepEqual(toggleBucket(toggleBucket({}, 'needs'), 'needs'), {}, 'NEEDS YOU clears like the others')
})

test('one tile is selected, and only a selected tile is lit: the needs-you glow is not a second selection', () => {
  const counts: Record<Bucket, number> = { needs: 3, working: 2, paused: 1, still: 1, done: 4, spent: 0.5 }
  for (const selected of [...BUCKETS, undefined]) {
    const looks = BUCKETS.map((b) => ({ b, ...tileLook(b, counts[b], selected) }))
    assert.deepEqual(looks.filter((t) => t.pressed).map((t) => t.b), selected ? [selected] : [], `pressed, with ${selected ?? 'nothing'} chosen`)
    assert.deepEqual(looks.filter((t) => t.lit).map((t) => t.b), selected ? [selected] : [], `lit, with ${selected ?? 'nothing'} chosen`)
  }
  assert.equal(tileLook('needs', 3, 'working').attention, true, 'NEEDS YOU says something waits, on its number')
  assert.equal(tileLook('needs', 3, 'needs').attention, true, 'selected or not')
  assert.equal(tileLook('needs', 0, undefined).attention, false, 'and says nothing when nothing waits')
  assert.equal(tileLook('working', 2, undefined).attention, false, 'no other tile asks for attention')
})

test('every path that sets the bucket filter is toggleBucket: the tile, its key, and the empty inspector’s counts', () => {
  const bar = read('ops/CommandBar.tsx')
  const page = read('views/Ops.tsx')
  assert.match(bar, /toggleBucket\(f, c\.b\)/, 'the tile')
  assert.match(bar, /tileLook\(/, 'and its look')
  assert.match(page, /toggleBucket\(a\.f, b\)/, 'the keys and the inspector’s counts')
  assert.doesNotMatch(page, /bucket: b \}/, 'no path sets the bucket without the toggle')
  assert.doesNotMatch(bar, /ring-wait\/60/, 'the glow is no ring')
})

/** The owner's page: a blocked step no queue item names; a task with an approval waiting; a router ask no card owns; a
 *  record waiting on a person with its question gone from the list; a task whose list entry counts two pending questions
 *  the confirm list does not hold. */
function fixture() {
  const views = [view('exe_ak', 'task', 'running', { session_id: 'ses_ak' })]
  const tasks = [info(5, 'waiting', { waiting_on: 'confirm', pending_confirms: 2 })]
  const records = [
    record('tsk_blk', { title: 'Wait for the permit', state: 'blocked' }),
    record('tsk_wh', { title: 'Choose the paint', state: 'waiting_human' }),
  ]
  const confirms: ConfirmRequest[] = [confirm('act_c1', { session_id: 'ses_ak' })]
  const loose = { ask_id: 'ask_1', session_id: 'ses_elsewhere', question: 'Which harbour?', options: [], message: 'survey it', expires_ms: NOW + HOUR } as unknown as ThreadAsk
  const queue = queueOf({ confirms, views, records, asks: [{ ask: loose, heard_ms: NOW - 5 * MIN }] })
  return { views, tasks, records, queue }
}

test('NEEDS YOU counts what the panel lists: every card the tile counts is one the panel names, and nothing else', () => {
  const { views, tasks, records, queue } = fixture()
  assert.equal(queue.length, 2, 'the panel’s asks: the approval and the router’s question')
  const rows = boardRows({ views, tasks, sessions: [], records, queue, now: NOW, past: false })
  const tile = allCounters(rows, NOW, 0).needs
  assert.equal(tile, 5, 'five cards wait: the task with the approval, the router ask, the blocked step, the waiting record, the task with pending questions')
  const listed = listedCards(rows)
  assert.equal(listed.length, tile, 'the panel lists as many cards as the tile counts')
  assert.deepEqual(listed.map((r) => r.id).sort(), rows.filter((r) => r.counted && r.bucket === 'needs').map((r) => r.id).sort(), 'the same cards')
})

test('the page the owner read: nothing asks, a blocked step waits, and the tile and the panel agree', () => {
  const records = [record('tsk_blk', { title: 'Wait for the permit', state: 'blocked' })]
  const queue = queueOf({ confirms: [], views: [], records })
  assert.equal(queue.length, 0, '0 asks')
  const rows = boardRows({ views: [], tasks: [], sessions: [], records, queue, now: NOW, past: false })
  assert.equal(needsOf(rows), 1, 'the tile counts the blocked step')
  assert.deepEqual(listedCards(rows).map((r) => r.id), ['tsk_blk'], 'and the panel names it, so it never says nothing waits')
})

test('the panel lists the unasked cards after the asks, each with its reason, and is empty only when the tile is', () => {
  const { views, tasks, records, queue } = fixture()
  const rows = boardRows({ views, tasks, sessions: [], records, queue, now: NOW, past: false })
  const waiting = unaskedCards(rows)
  assert.deepEqual(waiting.map((r) => r.id).sort(), ['ses_t5', 'tsk_blk', 'tsk_wh'], 'the three cards no queue item names')
  const why = Object.fromEntries(waiting.map((r) => [r.id, waitingFacts(r, NOW).why]))
  assert.deepEqual(why, { ses_t5: '2 questions pending, none in the list', tsk_blk: 'its record is blocked', tsk_wh: 'its record waits on a person' })
  assert.equal(waitingFacts(rows.find((r) => r.id === 'tsk_blk')!, NOW).kind, 'blocked')
  assert.equal(waitingFacts(rows.find((r) => r.id === 'ses_t5')!, NOW).href, '/session/ses_t5')
  const none = boardRows({ views: [], tasks: [], sessions: [], records: [], queue: [], now: NOW, past: false })
  assert.equal(allCounters(none, NOW, 0).needs, 0)
  assert.deepEqual(listedCards(none), [], 'nothing waits, so nothing is listed, and only then does the panel say so')
})

test('the Needs-you panel draws the unasked cards and says nothing waits only when it has none', () => {
  const queueSrc = read('ops/Queue.tsx')
  const page = read('views/Ops.tsx')
  assert.match(queueSrc, /!items\.length && !waiting\.length/, 'the empty state needs both lists empty')
  assert.match(queueSrc, /waiting\.map\(\(r\) => <Waiting /, 'each unasked card is an entry')
  assert.match(page, /const waiting = useMemo\(\(\) => unaskedCards\(rows\)/, 'the page reads them from the counter’s own rows')
  assert.match(page, /waiting=\{waiting\} onRow=/, 'and hands them to the panel')
  assert.equal((page.match(/glow=\{waits\}/g) ?? []).length, 3, 'the header, the folded tab and the page glow for a waiting card as for an ask')
  assert.doesNotMatch(page, /glow=\{queue\.length > 0\}/, 'none glows for asks alone')
})
