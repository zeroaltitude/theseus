// The charts' tooltips as DOM for ECharts' formatter (theseus-hnof). Every name is set as text, never as HTML: a
// session's title, a model's name, and a profile are data, and ECharts would put a string in as markup. Values lead and
// labels follow; a row keys its series with a short stroke of its colour (a ring or a dot for a marker).
import { CHROME, FONTS } from './viz.ts'

export interface TipRow { value: string; label: string; color?: string; mark?: 'line' | 'rect' | 'dot' | 'ring' | 'triangle'; strong?: boolean }

function el(tag: string, css: Partial<CSSStyleDeclaration>, text?: string): HTMLElement {
  const e = document.createElement(tag)
  Object.assign(e.style, css)
  if (text !== undefined) e.textContent = text
  return e
}

function key(color: string | undefined, mark: TipRow['mark']): HTMLElement {
  if (!color) return el('span', { width: '12px' })
  if (mark === 'triangle') return el('span', { width: '9px', height: '8px', margin: '0 1px', background: color, clipPath: 'polygon(50% 0, 100% 100%, 0 100%)' })
  if (mark === 'dot' || mark === 'ring') {
    return el('span', { width: '8px', height: '8px', borderRadius: '50%', boxSizing: 'border-box', margin: '0 2px',
      background: mark === 'dot' ? color : 'transparent', border: `2px solid ${color}` })
  }
  return el('span', { width: '12px', height: mark === 'rect' ? '8px' : '2px', borderRadius: mark === 'rect' ? '2px' : '1px', background: color })
}

/** A tooltip: an optional head (what the reader pointed at), then a row per value, then an optional foot. */
export function tip(head: string | null, rows: TipRow[], foot?: string): HTMLElement {
  const c = CHROME.dark
  const root = el('div', { fontFamily: FONTS.sans, fontSize: '12px', lineHeight: '1.45', color: c.text, minWidth: '120px' })
  if (head) root.append(el('div', { color: c.muted, fontSize: '11px', marginBottom: '3px' }, head))
  const grid = el('div', { display: 'grid', gridTemplateColumns: 'auto auto 1fr', alignItems: 'center', columnGap: '7px' })
  for (const r of rows) {
    grid.append(
      key(r.color, r.mark ?? 'line'),
      el('span', { fontFamily: FONTS.mono, fontVariantNumeric: 'tabular-nums', textAlign: 'right', fontWeight: r.strong === false ? '400' : '600', color: c.text }, r.value),
      el('span', { color: c.secondary, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', maxWidth: '260px' }, r.label),
    )
  }
  root.append(grid)
  if (foot) root.append(el('div', { color: c.muted, fontSize: '11px', marginTop: '3px' }, foot))
  return root
}
