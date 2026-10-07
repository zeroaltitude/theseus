// The instruments over the Ship's canvas (theseus-hnof.2): every element marked `data-ship-ui` (the cartouche, the
// controls, the watch, the console, the key, the depth gauge, the porthole, the selected vessel's card), as rectangles in
// the Ship's box. Cards (`placement.ts`) and labels stay off them.
import type { Rect } from './placement'

export function instrumentRects(box: HTMLElement, pad = 0): Rect[] {
  const o = box.getBoundingClientRect()
  return [...box.querySelectorAll<HTMLElement>('[data-ship-ui]')].map((u) => {
    const r = u.getBoundingClientRect()
    return { x0: r.left - o.left - pad, y0: r.top - o.top - pad, x1: r.right - o.left + pad, y1: r.bottom - o.top + pad }
  })
}
