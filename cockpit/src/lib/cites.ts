// A reply's citations (theseus-ejdk): each handle the reply cites (`[m:a1b2c3]`) is a numbered mark with a hover card
// of its source, and the mark links to the session that holds it. The daemon resolves the handles
// (`session.history`'s `cites`), and leaves out any a shared place may not show: a handle with no citation stays text.
// The same rule as `theseus_protocol::cite` and the CLI's `render::cite`.
import type { Citation } from '@protocol'

const HANDLE = /\[(m:[0-9a-zA-Z_]{6,64})\]/g

/** The marker a cited handle's link carries in the markdown, so the renderer can tell it from any other link. */
export const CITE_HREF = '#cite:'

/** The handles a text holds, each once, in the order they first appear. */
export function handlesIn(text: string): string[] {
  const out: string[] = []
  for (const m of text.matchAll(HANDLE)) if (!out.includes(m[1])) out.push(m[1])
  return out
}

export interface Marked { text: string; order: Citation[] }

/** The reply's text with each cited handle as a link whose text is its number, `[[1]](#cite:m:a1b2c3)`, in the order
 *  the handles first appear, and the citations in that order. A handle with no citation is left as the model wrote it. */
export function markCites(text: string, cites: readonly Citation[] | undefined): Marked {
  const order: Citation[] = []
  if (!cites?.length) return { text, order }
  const marked = text.replace(HANDLE, (whole, handle: string) => {
    const c = cites.find((x) => x.handle === handle)
    if (!c) return whole
    let i = order.findIndex((o) => o.handle === handle)
    if (i < 0) i = order.push(c) - 1
    return `[[${i + 1}]](${CITE_HREF}${handle})`
  })
  return { text: marked, order }
}

/** The handle a markdown link names, if it is a citation's. */
export function citeOf(href: string | undefined): string | null {
  return href?.startsWith(CITE_HREF) ? href.slice(CITE_HREF.length) : null
}

/** Where a citation's source lives: its session, in the cockpit. */
export function sessionHref(c: Citation): string {
  return `/session/${encodeURIComponent(c.session_id)}`
}

/** The hover card's lines: whose, where and when (the frozen header), the excerpt, and the command that opens it. */
export function cardLines(c: Citation): { header: string; excerpt: string; command: string } {
  return { header: c.header, excerpt: c.excerpt, command: `theseus history ${c.handle}` }
}
