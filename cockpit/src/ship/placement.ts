// Where a card goes beside the point it is about (theseus-hnof.2): a hover card beside the pointer, the tour's card beside
// its shape. Pure, so a test holds it. The card stays inside the Ship's box, and off the instruments over the canvas (the
// watch, the console, the key, the depth gauge, the porthole): it tries each side of the point, nearest first, and takes
// the first that covers none of them; when every side covers one, the one that covers least.

export interface Rect { x0: number; y0: number; x1: number; y1: number }

export interface Placed { x: number; y: number; side: 'below-right' | 'below-left' | 'above-right' | 'above-left' | 'right' | 'left' }

const area = (a: Rect, b: Rect) => Math.max(0, Math.min(a.x1, b.x1) - Math.max(a.x0, b.x0)) * Math.max(0, Math.min(a.y1, b.y1) - Math.max(a.y0, b.y0))

/**
 * @param at the point the card is about (CSS pixels in the box)
 * @param size the card's size
 * @param box the box's size
 * @param avoid the instruments' rectangles, in the box's pixels
 * @param gap how far the card stands off its point
 */
export function placeCard(at: { x: number; y: number }, size: { w: number; h: number }, box: { w: number; h: number }, avoid: Rect[], gap = 14): Placed {
  const { w, h } = size
  const m = 8
  const clampX = (x: number) => Math.max(m, Math.min(box.w - w - m, x))
  const clampY = (y: number) => Math.max(m, Math.min(box.h - h - m, y))
  const sides: [Placed['side'], number, number][] = [
    ['below-right', at.x + gap, at.y + gap],
    ['below-left', at.x - gap - w, at.y + gap],
    ['above-right', at.x + gap, at.y - gap - h],
    ['above-left', at.x - gap - w, at.y - gap - h],
    ['right', at.x + gap * 2, at.y - h / 2],
    ['left', at.x - gap * 2 - w, at.y - h / 2],
  ]
  let best: Placed | null = null
  let bestCost = Infinity
  for (const [side, x0, y0] of sides) {
    const x = clampX(x0)
    const y = clampY(y0)
    const r = { x0: x, y0: y, x1: x + w, y1: y + h }
    // A card pushed by the box's edge over its own point hides what it is about: that side is as bad as covering it all.
    const covers = at.x > r.x0 && at.x < r.x1 && at.y > r.y0 && at.y < r.y1 ? w * h : 0
    const cost = avoid.reduce((a, u) => a + area(r, u), 0) + covers
    if (cost === 0) return { x, y, side }
    if (cost < bestCost) { bestCost = cost; best = { x, y, side } }
  }
  return best!
}
