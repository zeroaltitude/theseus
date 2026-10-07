// Where the Ship's cards go (`src/ship/placement.ts`, theseus-hnof.2), run by `npm test`: beside their point, inside the
// Ship's box, and off the instruments over the canvas.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { placeCard } from '../src/ship/placement.ts'

const box = { w: 1850, h: 960 }
const size = { w: 400, h: 150 }
// The console at the foot and the watch at the right, as the Ship lays them out at 1920x1080.
const consoleRect = { x0: 760, y0: 770, x1: 1100, y1: 950 }
const watch = { x0: 1560, y0: 60, x1: 1840, y1: 720 }

test('a card goes below and right of its point when there is room', () => {
  assert.deepEqual(placeCard({ x: 500, y: 300 }, size, box, [consoleRect, watch]), { x: 514, y: 314, side: 'below-right' })
})

test('near the right edge or the watch it goes left; near the foot it rises to fit, beside its point', () => {
  assert.equal(placeCard({ x: 1300, y: 300 }, size, box, [consoleRect, watch]).side, 'below-left')
  const p = placeCard({ x: 300, y: 900 }, size, box, [consoleRect, watch])
  assert.ok(p.y + size.h <= box.h - 8 && p.x > 300, JSON.stringify(p))
})

test('it never sits on the console: an oar just above it takes its card up', () => {
  // Below the oar is the console; above is clear.
  const p = placeCard({ x: 940, y: 700 }, size, box, [consoleRect, watch])
  assert.ok(p.y + size.h <= consoleRect.y0 || p.x >= consoleRect.x1 || p.x + size.w <= consoleRect.x0, JSON.stringify(p))
})

test('it stays inside the box, and off its own point', () => {
  for (const at of [{ x: 4, y: 4 }, { x: 1846, y: 4 }, { x: 4, y: 956 }, { x: 1846, y: 956 }, { x: 925, y: 480 }]) {
    const p = placeCard(at, size, box, [])
    assert.ok(p.x >= 8 && p.y >= 8 && p.x + size.w <= box.w - 8 && p.y + size.h <= box.h - 8, JSON.stringify([at, p]))
    assert.ok(!(at.x > p.x && at.x < p.x + size.w && at.y > p.y && at.y < p.y + size.h), `covers ${JSON.stringify(at)}`)
  }
})

test('when every side covers an instrument, it takes the side that covers least', () => {
  const wall = { x0: 0, y0: 0, x1: 1850, y1: 960 }
  const p = placeCard({ x: 500, y: 300 }, size, box, [wall])
  assert.equal(p.side, 'below-right')
})
