// The ladder as the cockpit reads it (`src/lib/packs.ts`, M5 26a), run by `npm test`.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { modeWords, packLine, rowWords, share } from '../src/lib/packs.ts'

test("health's pack lines are a pack, its mode, and why", () => {
  assert.deepEqual(packLine('route.v1: live (owner: decision of 2026-10-04)'), {
    pack: 'route.v1', mode: 'live', why: '(owner: decision of 2026-10-04)',
  })
  assert.deepEqual(packLine('loop.v1: shadow'), { pack: 'loop.v1', mode: 'shadow', why: '' })
  assert.equal(packLine('security.v3: rolled back until 00:00 (notices_per_day)').mode, 'rolled back')
  assert.equal(packLine('loop.v1: canary 0.2 (owner: forced by the owner)').mode, 'canary')
})

test('a row says its move, who, why, and what it cites', () => {
  assert.equal(modeWords('canary', 0.2), 'canary 0.2')
  assert.equal(modeWords('rolled_back'), 'rolled back')
  assert.equal(
    rowWords({ mode: 'canary', from: 'shadow', share: 1, who: 'owner', via: 'cli', why: 'forced by the owner', forced: true, numbers: 'work_state: labeled 0 of 200', declined: false }),
    'shadow → canary 1.0 by owner (cli): forced by the owner · forced: work_state: labeled 0 of 200',
  )
  assert.equal(
    rowWords({ mode: 'rolled_back', from: 'live', who: 'system', via: 'ladder', why: 'rule notices_per_day', forced: false, rule: 'notices_per_day', words: '31 Jev notices', declined: false }),
    'live → rolled back by system (ladder): rule notices_per_day · notices_per_day: 31 Jev notices',
  )
})

test('a share is above 0 and at most 1', () => {
  assert.equal(share('0.2'), 0.2)
  assert.equal(share('1'), 1)
  for (const bad of ['0', '1.5', 'x', '-0.1']) assert.equal(share(bad), null)
})
