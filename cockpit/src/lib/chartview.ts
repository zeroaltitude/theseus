// The charts' view state (theseus-hnof): which charts show their table, kept in the address so a table view deep-links,
// and an element's width for a chart whose margins are measured rather than guessed.
import { useEffect, useRef, useState, type RefObject } from 'react'
import { useSearchParams } from 'react-router'

/** A chart's view, in the address: `?table=` lists the charts showing their table. */
export function useTableView(id: string): [boolean, (on: boolean) => void] {
  const [params, setParams] = useSearchParams()
  const on = (params.get('table') ?? '').split(',').includes(id)
  const set = (v: boolean) => setParams((p) => {
    const s = new Set((p.get('table') ?? '').split(',').filter(Boolean))
    if (v) s.add(id); else s.delete(id)
    if (s.size) p.set('table', [...s].join(',')); else p.delete('table')
    return p
  }, { replace: true })
  return [on, set]
}

/** An element's width in CSS pixels, followed as it resizes. */
export function useWidth<T extends HTMLElement>(): [RefObject<T | null>, number] {
  const ref = useRef<T>(null)
  const [w, setW] = useState(0)
  useEffect(() => {
    const el = ref.current
    if (!el) return
    const ro = new ResizeObserver(([e]) => setW(Math.round(e.contentRect.width)))
    ro.observe(el)
    return () => ro.disconnect()
  }, [])
  return [ref, w]
}

let measurer: CanvasRenderingContext2D | null = null

/** A text's width in CSS pixels in a canvas font (`11px <family>`): a label is placed where it fits, never guessed. */
export function textWidth(text: string, font: string): number {
  measurer ??= document.createElement('canvas').getContext('2d')
  if (!measurer) return text.length * 6.6
  measurer.font = font
  return measurer.measureText(text).width
}
