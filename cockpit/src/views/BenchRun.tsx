// A run's report, whole (theseus-raf4): its markdown as the repository keeps it (`docs/benchmarks/<report>.md`), every
// section, every table, and every figure as the report's own SVG (the dark twin at night, the light one by day), each
// with a chart | table toggle whose table is the figure's numbers from the report's data file. Links between reports
// open their readers; a link to the data opens the run's per-trial table at the foot. Read only, nothing live.
import { lazy, Suspense, useEffect, useMemo, useState, type ReactNode } from 'react'
import { Link, useLocation, useParams } from 'react-router'
import Markdown, { type Components } from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { ArrowLeft, Database, FileText } from 'lucide-react'
import { figureOf, figurePath, figureTable, linkTo, parseCsv, readerMarkdown, reportTitle, sections } from '@/lib/bench'
import { figureUrl, has, useBenchFile } from '@/lib/benchfiles'
import { useTableView } from '@/lib/chartview'
import { useMode } from '@/lib/mode'
import { cn } from '@/lib/format'
import { ChartTable, TableToggle } from '@/components/ChartPanel'
import { Panel } from '@/components/ui'

const JsonView = lazy(() => import('@/components/JsonView').then((m) => ({ default: m.JsonView })))

type Spec = Parameters<typeof figureTable>[0]

/** A heading's anchor, as GitHub makes it: lower case, spaces to hyphens, punctuation dropped. */
const slug = (s: string) => s.toLowerCase().trim().replace(/[^\p{L}\p{N}\s-]/gu, '').replace(/\s/g, '-')
const text = (n: ReactNode): string => (typeof n === 'string' || typeof n === 'number' ? String(n) : Array.isArray(n) ? n.map(text).join('') : n && typeof n === 'object' && 'props' in n ? text((n.props as { children?: ReactNode }).children) : '')

export default function BenchRun() {
  const { run = '' } = useParams()
  const md = useBenchFile(run, 'md')
  const json = useBenchFile(run, 'json')
  const known = has(run, 'md')
  const figures = useMemo(() => {
    try { return new Map(((JSON.parse(json.data ?? '{}') as { figures?: Spec[] }).figures ?? []).map((f) => [String(f.name), f])) } catch { return new Map<string, Spec>() }
  }, [json.data])
  const body = useMemo(() => (md.data ? readerMarkdown(md.data.replace(/^#\s+.+\n/, '')) : ''), [md.data])
  const title = md.data ? reportTitle(md.data) : undefined
  const heads = useMemo(() => (md.data ? sections(md.data) : []), [md.data])
  // A link with a section (`…#the-question`, `#data-csv`) lands on it once the report is drawn.
  const { hash } = useLocation()
  useEffect(() => {
    if (!hash || !md.data) return
    const t = setTimeout(() => document.getElementById(decodeURIComponent(hash.slice(1)))?.scrollIntoView({ block: 'start' }), 60)
    return () => clearTimeout(t)
  }, [hash, md.data, run])

  if (!known) {
    return (
      <Panel title="No such run">
        <div className="p-4 text-[13px] text-ink-dim">This build has no report named <span className="font-mono">{run}</span>. <Link to="/benchmarks" className="text-live">Every run</Link>.</div>
      </Panel>
    )
  }
  return (
    <div className="flex flex-col gap-3">
      <div className="panel flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-3">
        <Link to="/benchmarks" className="flex items-center gap-1 text-[11.5px] text-ink-faint hover:text-live"><ArrowLeft size={13} /> Benchmarks</Link>
        <h1 className="min-w-0 flex-1 font-display text-[19px] font-bold leading-snug tracking-[0.02em] text-ink">{title ?? run}</h1>
        <div className="flex items-center gap-3 text-[11.5px] text-ink-faint">
          <span className="font-mono">{run}.md</span>
          {has(run, 'csv') && <a href="#data-csv" className="flex items-center gap-1 hover:text-live"><Database size={12} /> per-trial data</a>}
          {has(run, 'json') && <a href="#data-json" className="flex items-center gap-1 hover:text-live"><FileText size={12} /> data file</a>}
        </div>
      </div>
      <Panel bodyClassName="px-5 py-4">
        {md.isLoading ? <div className="text-ink-faint">Reading the report…</div> : (
          <div className="flex gap-8">
            <article className="bench-prose min-w-0 flex-1">
              <Markdown remarkPlugins={[remarkGfm]} components={components(run, figures)}>{body}</Markdown>
            </article>
            {/* The report's sections, beside it where the screen has room, kept in view as it scrolls. */}
            <nav aria-label="the report's sections" className="hidden w-56 shrink-0 xl:block">
              <div className="sticky top-2">
                <div className="font-display text-[10px] font-bold uppercase tracking-[0.12em] text-gold/90">In this report</div>
                <ol className="mt-1.5 space-y-0.5 text-[12px]">
                  {heads.map((h) => <li key={h}><a href={`#${slug(h)}`} className="block truncate text-ink-faint hover:text-live" title={h}>{h}</a></li>)}
                  {has(run, 'csv') && <li><a href="#data-csv" className="block text-ink-faint hover:text-live">The trials, one row each</a></li>}
                  {has(run, 'json') && <li><a href="#data-json" className="block text-ink-faint hover:text-live">The data file</a></li>}
                </ol>
              </div>
            </nav>
          </div>
        )}
      </Panel>
      {has(run, 'csv') && <TrialTable run={run} />}
      {has(run, 'json') && (
        <Panel title={<span id="data-json">The data file</span>} icon={<FileText size={14} />} bodyClassName="p-3">
          <p className="mb-2 text-[12px] text-ink-dim">The run’s summary: its numbers, task ids and the specs every figure is drawn from (<span className="font-mono">{run}.json</span>).</p>
          {json.data && <Suspense fallback={<div className="text-ink-faint">…</div>}><JsonView value={json.data} maxHeight="420px" /></Suspense>}
        </Panel>
      )}
    </div>
  )
}

function components(run: string, figures: Map<string, Spec>): Components {
  const heading = (Tag: 'h2' | 'h3' | 'h4') => ({ children }: { children?: ReactNode }) => <Tag id={slug(text(children))}>{children}</Tag>
  return {
    h1: heading('h2'), h2: heading('h2'), h3: heading('h3'), h4: heading('h4'),
    // A paragraph that is only a figure is the figure's block, not a paragraph (a block in a <p> is not HTML).
    p: ({ node, children }) => {
      const kids = (node?.children ?? []).filter((c) => !(c.type === 'text' && !c.value.trim()))
      return kids.length === 1 && kids[0].type === 'element' && kids[0].tagName === 'img' ? <>{children}</> : <p>{children}</p>
    },
    img: ({ src, alt }) => <Figure run={run} src={String(src ?? '')} alt={alt ?? ''} figures={figures} />,
    a: ({ href, children }) => {
      const to = linkTo(String(href ?? ''))
      switch (to.kind) {
        case 'report': return <Link to={to.name ? `/benchmarks/${to.name}${to.hash ? `#${to.hash}` : ''}` : '/benchmarks'}>{children}</Link>
        case 'data': return to.name === run ? <a href={`#data-${to.ext}`}>{children}</a> : <Link to={`/benchmarks/${to.name}#data-${to.ext}`}>{children}</Link>
        case 'anchor': return <a href={`#${to.hash}`}>{children}</a>
        case 'web': return <a href={to.href} target="_blank" rel="noreferrer noopener">{children}</a>
        case 'repo': return <span className="bench-repo" title={`in the repository: ${to.path}`}>{children}</span>
      }
    },
    table: ({ children }) => <div className="my-2 overflow-x-auto"><table className="viz-table text-[12px]">{children}</table></div>,
  }
}

/** A figure: the report's SVG for the mode, and its table behind the toggle (`?table=fig-<name>`). */
function Figure({ run, src, alt, figures }: { run: string; src: string; alt: string; figures: Map<string, Spec> }) {
  const night = useMode((s) => s.mode) !== 'light'
  const fig = figureOf(src)
  const id = `fig-${fig?.figure ?? src}`
  const [tableOn] = useTableView(id)
  const spec = fig ? figures.get(fig.figure) : undefined
  const table = spec ? figureTable(spec) : undefined
  const url = fig ? figureUrl(figurePath(fig.report, fig.figure, night)) ?? figureUrl(figurePath(fig.report, fig.figure, false)) : undefined
  return (
    <figure className="my-3">
      <div className="mb-1 flex items-center gap-2">
        <figcaption className="min-w-0 flex-1 text-[11.5px] text-ink-faint">{alt}</figcaption>
        {table && <TableToggle id={id} />}
      </div>
      {tableOn && table ? (
        <div className="max-h-[520px] overflow-auto rounded-md ring-1 ring-line">
          <ChartTable caption={alt} rows={table.rows.map((r, i) => ({ r, i }))} rowKey={(x) => String(x.i)}
            columns={table.columns.map((c, j) => ({ key: String(j), label: c.label, num: c.num, cell: (x: { r: string[] }) => x.r[j], title: (x: { r: string[] }) => x.r[j] }))} />
        </div>
      ) : url ? (
        <img src={url} alt={alt} loading="lazy" className={cn('block h-auto w-full max-w-[760px] rounded-md', night ? 'ring-1 ring-line' : '')} />
      ) : <div className="text-[12px] text-ink-faint">This build has no file for {src} ({run}).</div>}
    </figure>
  )
}

/** The run's per-trial table, sortable by any column. */
function TrialTable({ run }: { run: string }) {
  const csv = useBenchFile(run, 'csv')
  const rows = useMemo(() => (csv.data ? parseCsv(csv.data) : []), [csv.data])
  const cols = useMemo(() => Object.keys(rows[0] ?? {}), [rows])
  const numeric = useMemo(() => new Set(cols.filter((c) => rows.some((r) => r[c] !== '') && rows.every((r) => r[c] === '' || Number.isFinite(Number(r[c]))))), [cols, rows])
  const [sort, setSort] = useState<{ col: string; dir: 1 | -1 } | null>(null)
  const sorted = useMemo(() => {
    if (!sort) return rows
    const { col, dir } = sort
    return [...rows].sort((a, b) => {
      const x = a[col], y = b[col]
      if (x === '' || y === '') return x === y ? 0 : x === '' ? 1 : -1
      return dir * (numeric.has(col) ? Number(x) - Number(y) : x.localeCompare(y))
    })
  }, [rows, sort, numeric])
  return (
    <Panel title={<span id="data-csv">The trials, one row each</span>} icon={<Database size={14} />} bodyClassName="p-0"
      actions={<span className="text-[11px] text-ink-faint">{rows.length} rows · {run}.csv · click a column to sort</span>}>
      <div className="max-h-[560px] overflow-auto">
        <table className="viz-table w-full text-[12px]">
          <caption className="sr-only">every row of {run}.csv</caption>
          <thead>
            <tr>
              {cols.map((c) => (
                <th key={c} scope="col" className={numeric.has(c) ? 'text-right' : 'text-left'} aria-sort={sort?.col === c ? (sort.dir === 1 ? 'ascending' : 'descending') : undefined}>
                  <button type="button" className="hover:text-live" onClick={() => setSort((s) => (s?.col === c ? (s.dir === 1 ? { col: c, dir: -1 } : null) : { col: c, dir: 1 }))}>
                    {c}{sort?.col === c ? (sort.dir === 1 ? ' ↑' : ' ↓') : ''}
                  </button>
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {sorted.map((r, i) => (
              <tr key={i}>
                {cols.map((c) => <td key={c} title={r[c]} className={numeric.has(c) ? 'num text-right text-ink' : 'text-ink-dim'}>{r[c] === '' ? '–' : r[c]}</td>)}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Panel>
  )
}
