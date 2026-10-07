// The context explorer's turn tab and its search (theseus-7n3e). A turn's context is `context.explain`: the request's
// parts as the next turn would build them (the header's, each context file, each category's guidance, the tools, the
// recall in front of the model, and the conversation, the rest of the turn's estimate), each with its tokens, and said
// against the digests the turn's compilation recorded; then why each node was recalled. Asking the index is
// `memory.search`, from a private place, writing nothing. Both read only. The pure parts are lib/explorer.ts.
import { useState, type ReactNode } from 'react'
import { Link } from 'react-router'
import { AlertTriangle, CheckCircle2, ChevronDown, ChevronRight, Layers, MessagesSquare, Search, Sparkles } from 'lucide-react'
import type { ContextExplainResult, ContextPart, ContextSource, RecallManifest, SessionListResult } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { cn, short, stamp } from '@/lib/format'
import { CATEGORICAL } from '@/lib/viz'
import { useMode } from '@/lib/mode'
import { daylightColor } from '@/lib/daylight'
import { Empty, Field, Panel, Pill } from './ui'
import { ChartPanel, Swatch, TipArea, TipBody, TipTarget, type LegendItem } from './ChartPanel'
import { BLOCKS, anatomy, contextHref, count, episodeHref, sensitivityWords, thenWords, type BlockKey } from '@/lib/explorer'

type SetParams = (patch: Record<string, string | null | undefined>) => void

/** Each block's colour: the categorical slots in order, a block a slot. */
const BLOCK_COLOR: Record<BlockKey, string> = Object.fromEntries(BLOCKS.map((b, i) => [b.key, CATEGORICAL.dark[i]])) as Record<BlockKey, string>

function useMark(): (c: string) => string {
  const day = useMode((s) => s.mode) === 'light'
  return (c) => (day ? daylightColor(c) : c)
}

/** A turn's context: pick a session (the owner's newest), then a turn (its newest by default). */
export function TurnContext({ session, turn, set }: { session: string | null; turn: string | null; set: SetParams }) {
  if (!session) return <SessionPicker pick={(s) => set({ session: s, turn: null })} />
  return <Explained session={session} turn={turn} set={set} />
}

function SessionPicker({ pick }: { pick: (s: string) => void }) {
  const { data } = useRpc<SessionListResult>('session.list', { n: 40 }, 10_000)
  const list = data?.sessions ?? []
  return (
    <Panel title="pick a session" icon={<MessagesSquare size={13} />}>
      {!list.length ? <Empty>no sessions yet</Empty> : (
        <ul className="divide-y divide-line/50">
          {list.map((s) => (
            <li key={s.session_id}>
              <button type="button" onClick={() => pick(s.session_id)} className="flex w-full items-center gap-3 px-3 py-2 text-left text-[12px] hover:bg-white/[0.03]">
                <span className="num w-28 shrink-0 text-ink-faint">{short(s.session_id)}</span>
                <span className="min-w-0 flex-1 truncate text-ink">{s.title ?? s.label ?? '—'}</span>
                <span className="num text-ink-faint">{s.turns} turns · {stamp(s.last_active_ms)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </Panel>
  )
}

function Explained({ session, turn, set }: { session: string; turn: string | null; set: SetParams }) {
  const { data: x, error, isFetching } = useRpc<ContextExplainResult>('context.explain', { session_id: session, ...(turn ? { turn_id: turn } : {}) }, 0)
  if (error) return <Panel title="a turn’s context"><Empty>the daemon refused the read: {(error as Error).message}</Empty></Panel>
  if (!x) return <Panel title="a turn’s context"><Empty>reading what the turn saw…</Empty></Panel>
  return (
    <div className={cn('flex flex-col gap-3', isFetching && 'opacity-80')}>
      <Panel title={<>{x.title ?? short(x.session_id)}</>} icon={<Layers size={13} />}
        actions={<>
          <Link to={`/session/${x.session_id}`} className="text-[11px] text-ink-faint hover:text-ink">session deck</Link>
          <button type="button" onClick={() => set({ session: null, turn: null })} className="text-[11px] text-ink-faint hover:text-ink">another session</button>
        </>}>
        <div className="grid grid-cols-2 gap-x-4 gap-y-2 p-3 text-[12px] md:grid-cols-4">
          <Field label="where it speaks">{x.place} <span className="text-ink-faint">({x.class})</span></Field>
          <Field label="model" mono>{x.model || '—'}{x.profile ? ` · ${x.profile}` : ''}</Field>
          <Field label="window" mono>{x.window ? count(x.window) : '—'}</Field>
          <Field label="read" mono>{x.ms.toFixed(1)} ms</Field>
        </div>
        {x.withheld && <div className="px-3 pb-2 text-[11.5px] text-wait">{x.withheld}</div>}
        {x.imported && (
          <div className="flex flex-col gap-1 px-3 pb-3 text-[12px] text-ink-dim">
            <div>An imported session: the owner’s own history. It takes no turn, so it has no context of its own; it reaches a model only as recall’s testimony, from a private place.</div>
            <Link to={episodeHref(x.session_id)} className="text-live hover:underline">open it among the episodes</Link>
          </div>
        )}
        {x.turns.length > 0 && (
          <div className="flex gap-1 overflow-x-auto border-t border-line px-2 py-1.5" aria-label="turns">
            {x.turns.map((t, i) => (
              <button key={t.turn_id} type="button" onClick={() => set({ turn: i === 0 ? null : t.turn_id })}
                className={cn('num shrink-0 rounded-md px-2 py-1 text-left text-[10.5px] ring-1 ring-inset',
                  t.turn_id === x.turn_id ? 'bg-live/10 text-live ring-live/30' : 'text-ink-faint ring-line hover:text-ink')}>
                <div>{i === 0 ? 'newest turn' : `turn −${i}`} · {stamp(t.at_ms)}</div>
                <div>{t.loops} loop{t.loops === 1 ? '' : 's'} · {count(t.est_tokens)} tokens</div>
              </button>
            ))}
          </div>
        )}
      </Panel>
      {!x.imported && <>
        <Unchanged x={x} />
        {x.unbuilt && <Panel title="parts"><Empty>the parts could not be built: {x.unbuilt}</Empty></Panel>}
        {x.parts.length > 0 && <Anatomy x={x} />}
        {x.parts.length > 0 && <Parts parts={x.parts} />}
        <Recalls recalls={x.recalls} sources={x.sources} title="why each was recalled · this turn’s recall" />
      </>}
    </div>
  )
}

/** Whether the parts below are the bytes the turn saw: the system blocks' digest now against its compilation's. */
function Unchanged({ x }: { x: ContextExplainResult }) {
  if (x.unchanged == null) return <div className="px-1 text-[11.5px] text-ink-faint">no turn of this session compiled yet: the parts are what its first turn would carry now</div>
  return x.unchanged
    ? <div className="flex items-center gap-1.5 px-1 text-[11.5px] text-ink-dim"><CheckCircle2 size={13} className="text-ok" /> the system blocks below are the bytes this turn saw: their digest <span className="num text-ink">{x.digest_now}</span> is its compilation’s</div>
    : <div className="flex items-center gap-1.5 px-1 text-[11.5px] text-wait"><AlertTriangle size={13} /> the system blocks changed since this turn (its compilation’s digest <span className="num">{x.digest_then}</span>, now <span className="num">{x.digest_now}</span>): the parts are today’s, and each says whether it is as the turn saw it</div>
}

/** The request's tokens by block: one bar of their shares, each labelled under it (the key and the labels at once); its
 *  table is every part. */
function Anatomy({ x }: { x: ContextExplainResult }) {
  const mark = useMark()
  const blocks = anatomy(x.parts)
  const total = blocks.reduce((a, b) => a + b.tokens, 0)
  const est = x.compiled?.est_tokens
  const legend: LegendItem[] = blocks.map((b) => ({ key: b.key, label: b.word, color: BLOCK_COLOR[b.key], mark: 'rect' }))
  return (
    <ChartPanel id="anatomy" title={<>what the turn sees · {count(total)} tokens{x.window ? ` of a ${count(x.window)} window` : ''}</>} icon={<Layers size={13} />} legend={legend}
      empty={total ? undefined : 'no tokens'}
      table={{
        caption: 'every part of the request, in order, with its tokens', rows: x.parts.map((p, i) => ({ ...p, i })), rowKey: (p) => String(p.i),
        columns: [
          { key: 'b', label: 'block', cell: (p) => p.block },
          { key: 'n', label: 'part', cell: (p) => p.name, className: 'max-w-[40ch] truncate', title: (p) => p.name },
          { key: 't', label: 'tokens', num: true, cell: (p) => count(p.tokens) },
          { key: 'y', label: 'bytes', num: true, cell: (p) => (p.bytes ? count(p.bytes) : '') },
          { key: 's', label: 'share', num: true, cell: (p) => (total ? `${((p.tokens / total) * 100).toFixed(1)}%` : '') },
          { key: 'w', label: 'against the turn', cell: (p) => thenWords(p.then) },
        ],
      }}>
      <TipArea className="flex flex-col gap-2 p-1">
        <div className="flex h-4 w-full gap-[2px]" role="group" aria-label="the request's tokens by block">
          {blocks.map((b, i) => (
            <TipTarget key={b.key} label={`${b.word}: ${b.tokens} tokens`} className="viz-mark h-full min-w-[3px]"
              style={{ flex: `${(b.tokens / total) * 1000} 1 0`, background: mark(BLOCK_COLOR[b.key]), borderRadius: i === 0 ? '3px 0 0 3px' : i === blocks.length - 1 ? '0 3px 3px 0' : 0 }}
              tip={<TipBody head={b.word} rows={[{ value: count(b.tokens), label: `tokens in ${b.parts} part${b.parts === 1 ? '' : 's'}`, color: BLOCK_COLOR[b.key], mark: 'rect' }]} foot={`${((b.tokens / total) * 100).toFixed(1)}% · ${b.what}`} />} />
          ))}
        </div>
        <div className="num flex flex-wrap gap-x-4 gap-y-1 text-[11px] text-ink-dim">
          {blocks.map((b) => <span key={b.key} className="flex items-center gap-1.5"><Swatch color={BLOCK_COLOR[b.key]} />{b.word} <span className="text-ink">{count(b.tokens)}</span><span className="text-ink-faint">{((b.tokens / total) * 100).toFixed(0)}%</span></span>)}
        </div>
        {est != null && <div className="text-[11px] text-ink-faint">Every part is counted from its bytes at the model’s figures, the conversation as the turn’s request less the parts above. The turn’s last loop itself estimated {count(est)} tokens{x.compiled?.estimate?.method === 'counted' ? ', part of it the provider’s own count' : ', from its bytes'}.</div>}
      </TipArea>
    </ChartPanel>
  )
}

/** Every part, by block, each opening to its text. */
function Parts({ parts }: { parts: ContextPart[] }) {
  const [open, setOpen] = useState<ReadonlySet<number>>(() => new Set())
  const toggle = (i: number) => setOpen((s) => { const n = new Set(s); if (n.has(i)) n.delete(i); else n.add(i); return n })
  return (
    <Panel title={<>the parts · {parts.length}</>} icon={<Layers size={13} />}>
      <ol className="divide-y divide-line/50">
        {parts.map((p, i) => {
          const isOpen = open.has(i)
          const has = !!p.text
          return (
            <li key={i}>
              <button type="button" onClick={() => has && toggle(i)} aria-expanded={has ? isOpen : undefined}
                className={cn('flex w-full items-center gap-2 px-3 py-1.5 text-left text-[12px]', has && 'hover:bg-white/[0.03]')}>
                <span className="w-3 text-ink-faint">{has ? (isOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />) : null}</span>
                <Swatch color={BLOCK_COLOR[p.block as BlockKey] ?? CATEGORICAL.dark[7]} />
                <span className="w-24 shrink-0 text-ink-faint">{BLOCKS.find((b) => b.key === p.block)?.word ?? p.block}</span>
                <span className="min-w-0 flex-1 truncate text-ink" title={p.name}>{p.name}{p.version != null ? <span className="num text-ink-faint"> · v{p.version}</span> : null}</span>
                {p.then && <span className={cn('shrink-0 text-[11px]', p.then === 'same' ? 'text-ink-faint' : 'text-wait')}>{thenWords(p.then)}</span>}
                <span className="num w-20 shrink-0 text-right text-ink">{count(p.tokens)}</span>
              </button>
              {(p.note || p.digest) && !isOpen && <div className="num truncate px-3 pb-1.5 pl-[3.25rem] text-[10.5px] text-ink-faint" title={p.note ?? ''}>{p.digest ? `digest ${p.digest}` : ''}{p.digest && p.note ? ' · ' : ''}{p.note ?? ''}</div>}
              {isOpen && <pre className="mx-3 mb-2 max-h-80 overflow-auto whitespace-pre-wrap break-words rounded-md bg-black/20 p-2.5 text-[11.5px] leading-relaxed text-ink-dim ring-1 ring-inset ring-line">{p.text}</pre>}
            </li>
          )
        })}
      </ol>
    </Panel>
  )
}

/** Where a recalled node came from: an imported episode's labels, or a session's place. */
function SourceWords({ id, src }: { id: string; src?: ContextSource }) {
  const imp = src?.imported
  if (imp) {
    const w = sensitivityWords(imp.sensitivity)
    return (
      <span className="flex flex-wrap items-center gap-1.5">
        <Link to={episodeHref(id)} className="text-live hover:underline">an imported {imp.place_kind}{imp.place_name ? ` · ${imp.place_name}` : ''}</Link>
        <Pill tone={w.tone}>{w.word}</Pill>
        <span className="num text-ink-faint">{imp.source} · {new Date(imp.end_ms).toISOString().slice(0, 10)}</span>
      </span>
    )
  }
  // An imported session's id starts so (theseus-0lrr.6): its episode, not a turn's context.
  if (id.startsWith('ses_ep')) return <Link to={episodeHref(id)} className="text-live hover:underline">an imported episode · {short(id)}</Link>
  return <Link to={contextHref(id)} className="text-live hover:underline">{src?.place || short(id)}{src?.title ? ` · ${src.title}` : ''}</Link>
}

/** Recall's manifests: each admitted node with why (its rank, its fused score, each source's rank and score), where it
 *  came from, its tokens and excerpt; then the drops by reason. */
export function Recalls({ recalls, sources, title }: { recalls: RecallManifest[]; sources: Record<string, ContextSource>; title: ReactNode }) {
  if (!recalls.length) return <Panel title={title} icon={<Sparkles size={13} />}><Empty>no recall recorded for this turn</Empty></Panel>
  return (
    <>
      {recalls.map((m) => (
        <Panel key={m.recall_id} title={title} icon={<Sparkles size={13} />}
          actions={<span className="num text-[11px] text-ink-faint">{m.mode} · {m.arm ?? m.science} · {m.outcome}</span>}>
          <div className="num flex flex-wrap gap-x-4 gap-y-1 border-b border-line px-3 py-2 text-[11px] text-ink-faint">
            <span>{count(m.candidates)} candidates{Object.keys(m.sources ?? {}).length ? ` (${Object.entries(m.sources ?? {}).map(([k, v]) => `${k} ${v}`).join(', ')})` : ''}</span>
            <span className="text-ink">{m.admitted.length} admitted</span>
            <span>{count(m.used_tokens)} of {count(m.budget_tokens)} tokens</span>
            <span>{m.timings.total_ms.toFixed(0)} ms</span>
            <span>where it speaks: {m.place}</span>
            {m.why && <span className="text-wait">{m.why}</span>}
            {Object.entries(m.skipped ?? {}).map(([k, v]) => <span key={k} className="text-wait">{k} skipped: {v}</span>)}
          </div>
          <ol className="divide-y divide-line/50">
            {m.admitted.map((it) => (
              <li key={`${it.node_id}:${it.chunk}`} className="px-3 py-2 text-[12px]">
                <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                  <span className="num text-ink">#{it.rank}</span>
                  <span className="num text-ink-faint" title="the fused score the rank ordered by">fused {it.fused.toFixed(3)}</span>
                  {Object.entries(it.sources ?? {}).map(([k, s]) => <span key={k} className="num text-ink-dim" title={`${k}: its rank among ${k}'s hits, and its score`}>{k} #{s.rank} ({s.score.toFixed(2)})</span>)}
                  <span className="num ml-auto text-ink-faint">{count(it.tokens)} tokens · {it.kind}</span>
                </div>
                <div className="mt-0.5 text-[11.5px]"><SourceWords id={it.session_id} src={sources[it.session_id]} /></div>
                {it.text && <p className="mt-1 line-clamp-4 whitespace-pre-wrap text-[12px] text-ink-dim">{it.text}</p>}
              </li>
            ))}
            {!m.admitted.length && <li><Empty>nothing admitted</Empty></li>}
          </ol>
          {Object.keys(m.drops ?? {}).length > 0 && (
            <div className="num border-t border-line px-3 py-2 text-[11px] text-ink-faint">
              dropped: {Object.entries(m.drops ?? {}).map(([k, v]) => `${v} for ${k}`).join(' · ')}
            </div>
          )}
        </Panel>
      ))}
    </>
  )
}

/** The question's box: its draft starts from the address's question; the parent keys it on the question. */
function AskForm({ initial, submit }: { initial: string; submit: (t: string) => void }) {
  const [text, setText] = useState(initial)
  return (
    <form className="flex gap-2 p-3" onSubmit={(e) => { e.preventDefault(); submit(text) }}>
      <input value={text} onChange={(e) => setText(e.target.value)} placeholder="what would recall bring for these words?" aria-label="words to ask the index"
        className="min-w-0 flex-1 rounded-md bg-white/5 px-2.5 py-1.5 text-[12.5px] text-ink outline-none ring-1 ring-line placeholder:text-ink-faint focus:ring-live/40" />
      <button type="submit" className="brass-button">ask</button>
    </form>
  )
}

/** Ask the index what recall would admit for some words, as a private place's turn would: `memory.search`, which
 *  writes nothing. */
export function AskIndex({ ask, set }: { ask: string; set: SetParams }) {
  const { data, error, isFetching } = useRpc<RecallManifest>('memory.search', { query: ask }, 0, { enabled: ask.trim() !== '' })
  const ids = [...new Set((data?.admitted ?? []).map((i) => i.session_id))]
  return (
    <div className="flex flex-col gap-3">
      <Panel title="ask the index" icon={<Search size={13} />}>
        <AskForm key={ask} initial={ask} submit={(t) => set({ ask: t.trim() || null })} />
        <div className="px-3 pb-3 text-[11px] text-ink-faint">Recall’s pipeline over the words, as a turn in a private place would run it (the baseline arm): what it would admit and why, and what it would drop. It writes nothing and calls no model.</div>
      </Panel>
      {!ask ? null : error ? <Panel title="answer"><Empty>the daemon refused the read: {(error as Error).message}</Empty></Panel>
        : !data ? <Panel title="answer"><Empty>{isFetching ? 'asking…' : '—'}</Empty></Panel>
        : <Recalls recalls={[data]} sources={Object.fromEntries(ids.map((id) => [id, { place: '' }]))} title={<>what recall would admit · “{ask}”</>} />}
    </div>
  )
}
