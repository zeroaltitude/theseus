// A reply as markdown, with the sources it cites as numbered marks (theseus-ejdk). A mark's hover card holds the
// source (whose message, where, when, the excerpt, and the command that opens it) and a link to the session that
// holds it; `lib/cites.ts` makes the marks and says the card's words. A handle with no source stays text.
import { Link } from 'react-router'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import type { Citation } from '@protocol'
import { cardLines, citeOf, markCites, sessionHref } from '@/lib/cites'

/** One mark: `[1]`, linked to the source's session, with its card beside it on hover or focus. */
function CiteMark({ n, c }: { n: string; c: Citation }) {
  const card = cardLines(c)
  return (
    <span className="group/cite relative inline-block align-baseline">
      <Link to={sessionHref(c)} className="num text-[11px] text-model hover:underline" aria-label={`source ${n}: ${card.header}`}>[{n}]</Link>
      <span role="tooltip" className="pointer-events-none absolute bottom-full left-0 z-20 mb-1 hidden w-72 rounded-md bg-deck px-2.5 py-2 text-[11.5px] text-ink-dim shadow-lg ring-1 ring-line group-hover/cite:block group-focus-within/cite:block">
        <span className="block text-ink">{card.header}</span>
        {card.excerpt && <span className="mt-1 block whitespace-pre-wrap border-l-2 border-model/40 pl-2 italic">{card.excerpt}</span>}
        <span className="num mt-1 block text-[10.5px] text-ink-faint">{card.command}</span>
      </span>
    </span>
  )
}

/** The reply's markdown, its cited handles drawn as marks. */
export function ReplyMarkdown({ text, cites }: { text: string; cites?: Citation[] }) {
  const { text: marked, order } = markCites(text, cites)
  return (
    <div className="md text-[13px] text-ink">
      <Markdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ node: _node, href, children, ...rest }) => {
            const handle = citeOf(href)
            const c = handle ? order.find((o) => o.handle === handle) : undefined
            return c ? <CiteMark n={String(order.indexOf(c) + 1)} c={c} /> : <a href={href} {...rest}>{children}</a>
          },
        }}
      >{marked}</Markdown>
    </div>
  )
}
