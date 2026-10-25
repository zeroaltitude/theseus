// The personas (theseus-utfnl): a `persona:<key>` category each, its soul one small file. A view of `persona.list` with
// the owner's writes: add (a persona with no soul awaits it, and the owner is asked once), a soul editor (`persona.soul`),
// make default (the one Jev's `none` engages) and retire. Every write is a protocol method, confirmed first, and judged
// by the core: only the owner, from a private place, writes a soul. A soul over the cap is refused with its count.
import { useCallback, useEffect, useState } from 'react'
import { Plus, UserRound } from 'lucide-react'
import type { PersonaInfo, PersonaListResult, PersonaSoulParams, PersonaSoulReadParams, PersonaSoulReadResult, PersonaSoulBackup } from '@protocol'
import { addParams, bigShrink, byteCount, personasOf, refusalWords, SHRINK_WARNING, sizeWords, soulAsk, soulHead, soulState, soulWords } from '@/lib/ontology'
import { cn } from '@/lib/format'
import { call, useRpc } from '@/lib/rpc'
import { Empty, Panel, Pill } from '@/components/ui'
import { Act, Refused, useOntologyWrite } from '@/components/OntologyParts'

const field = 'rounded-md bg-white/5 px-2.5 py-1.5 text-[12px] text-ink outline-none ring-1 ring-line placeholder:text-ink-faint focus:ring-live/40 disabled:opacity-50'
const TONE = { live: 'idle', awaiting: 'wait', over_cap: 'fault' } as const

export function PersonasPanel({ disabled }: { disabled: boolean }) {
  const { data } = useRpc<PersonaListResult>('persona.list', {}, 5000)
  const [open, setOpen] = useState<string | null>(null)
  const list = data ? personasOf(data) : []
  const cap = data?.cap ?? 2000
  return (
    <>
      <Panel help="ontology.personas" title={<>personas · {list.length}</>} icon={<UserRound size={13} />}>
        {list.length === 0 ? <Empty>no persona yet: add one below, and write its soul</Empty> : (
          <ul className="divide-y divide-line/50">
            {list.map((p) => <PersonaRow key={p.key} p={p} cap={cap} disabled={disabled} editing={open === p.key} toggle={() => setOpen(open === p.key ? null : p.key ?? null)} />)}
          </ul>
        )}
      </Panel>
      <AddPersona disabled={disabled} />
    </>
  )
}

function PersonaRow({ p, cap, disabled, editing, toggle }: { p: PersonaInfo; cap: number; disabled: boolean; editing: boolean; toggle: () => void }) {
  const { write, busy, refused } = useOntologyWrite()
  const key = p.key ?? ''
  const state = soulState(p)
  const retire = () => void write(`Retire the persona ${key}? Its file is kept, and \`theseus ontology undo\` or adding it again brings it back.`, 'persona.retire', { key })
  const makeDefault = () => void write(`Make ${key} the default persona? Jev’s “none” engages it.`, 'persona.default', { key })
  return (
    <li className="flex flex-col gap-1 px-3 py-2 text-[12px]">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-ink">{p.emoji ? `${p.emoji} ` : ''}{p.name}</span>
        <span className="num text-[10px] text-ink-faint">persona:{key}</span>
        {p.is_default && <Pill tone="think">default</Pill>}
        <Pill tone={TONE[state]}>{soulWords(p, cap)}</Pill>
        <span className="flex-1" />
        <button type="button" onClick={toggle} className="text-[11px] text-ink-faint hover:text-live">{editing ? 'close' : p.awaiting ? 'write its soul' : 'edit its soul'}</button>
        {!p.is_default && !p.awaiting && <button type="button" disabled={disabled || busy} onClick={makeDefault} className="text-[11px] text-ink-faint hover:text-live disabled:opacity-50">make default</button>}
        <button type="button" disabled={disabled || busy} onClick={retire} className="text-[11px] text-ink-faint hover:text-fault disabled:opacity-50">retire</button>
      </div>
      {p.card && <div className="text-ink-faint">{p.card}</div>}
      {!p.awaiting && <SoulHead persona={key} digest={p.files?.[0]?.digest} />}
      {editing && <SoulEditor key={key} persona={key} cap={cap} disabled={disabled} />}
      <Refused words={refused} />
    </li>
  )
}

/** The card's view of the current soul: its first lines, read from the file (`persona.soul_read`) whenever its digest moves. */
function SoulHead({ persona, digest }: { persona: string; digest?: string }) {
  const [head, setHead] = useState('')
  useEffect(() => {
    let live = true
    const p: PersonaSoulReadParams = { key: persona }
    call<PersonaSoulReadResult>('persona.soul_read', p).then((r) => { if (live) setHead(soulHead(r.text ?? '')) }, () => { if (live) setHead('') })
    return () => { live = false }
  }, [persona, digest])
  return head ? <div className="whitespace-pre-wrap font-mono text-[11px] text-ink-faint">{head}</div> : null
}

/**
 * A soul, edited whole (`persona.soul`), as the CLI does: the editor opens with the file’s text (`persona.soul_read`) and
 * never an empty box that could be saved over it. It sends the digest it loaded, so a file written since is refused
 * and nothing changes; the daemon keeps the replaced file as a backup, and “restore previous” loads one into the editor
 * (the restore is the same write).
 */
function SoulEditor({ persona, cap, disabled }: { persona: string; cap: number; disabled: boolean }) {
  const { write, busy, refused } = useOntologyWrite()
  const [live, setLive] = useState<PersonaSoulReadResult | null>(null)
  const [text, setText] = useState<string | null>(null)
  const [base, setBase] = useState<string | null>(null)
  const [from, setFrom] = useState<string | null>(null)
  const [failed, setFailed] = useState<string | null>(null)
  const [listing, setListing] = useState(false)
  const show = useCallback((r: PersonaSoulReadResult) => {
    setLive(r)
    setText(r.text ?? '')
    setBase(r.text ?? '')
    setFrom(null)
    setFailed(null)
  }, [])
  const load = () => call<PersonaSoulReadResult>('persona.soul_read', { key: persona } satisfies PersonaSoulReadParams).then(show, (e) => setFailed(refusalWords(e)))
  useEffect(() => {
    let open = true
    call<PersonaSoulReadResult>('persona.soul_read', { key: persona } satisfies PersonaSoulReadParams).then((r) => { if (open) show(r) }, (e) => { if (open) setFailed(refusalWords(e)) })
    return () => { open = false }
  }, [persona, show])
  const dirty = text !== null && base !== null && text !== base
  const reload = () => { if (!dirty || window.confirm('Discard what you have typed and load the file again?')) void load() }
  const pick = async (b: PersonaSoulBackup) => {
    if (dirty && !window.confirm('Replace what you have typed with the backup? Your edit is not saved anywhere else.')) return
    try {
      const r = await call<PersonaSoulReadResult>('persona.soul_read', { key: persona, bak_ms: b.ms } satisfies PersonaSoulReadParams)
      setText(r.text ?? '')
      setBase(r.text ?? '')
      setFrom(new Date(b.ms).toLocaleString())
      setListing(false)
    } catch (e) {
      setFailed(refusalWords(e))
    }
  }
  const oldBytes = live?.bytes ?? 0
  const newBytes = byteCount(text ?? '')
  const save = async () => {
    if (text === null || !live) return
    const p: PersonaSoulParams = { key: persona, text, expect_digest: live.digest }
    const out = await write(soulAsk(persona, cap, { oldBytes, newBytes, backup: from ?? undefined }), 'persona.soul', p)
    if (out) await load()
  }
  const backups = live?.backups ?? []
  return (
    <div className="flex flex-col gap-2 pt-1">
      <textarea value={text ?? ''} onChange={(e) => setText(e.target.value)} rows={12} disabled={disabled} readOnly={text === null} aria-label={`soul of ${persona}`}
        placeholder={text === null ? 'loading the soul…' : 'Its voice, its temperament, what it cares about, what it is for, and an emoji. Keep it short.'} className={cn(field, 'font-mono')} />
      {from && <div className="text-[11px] text-wait">from the backup of {from}: writing it replaces the file now, which is kept as a backup in turn</div>}
      {failed && <div className="text-[11px] text-fault">could not read the soul: {failed}. Writing is off until it loads.</div>}
      {text !== null && bigShrink(oldBytes, newBytes) && <div className="text-[11px] text-wait">{SHRINK_WARNING}</div>}
      <Refused words={refused} />
      <div className="flex flex-wrap items-center gap-2">
        <Act onClick={() => void save()} busy={busy} off={disabled || text === null || failed !== null || text.trim() === ''} title="persona.soul: confirmed first">Write soul</Act>
        <span className="num text-[11px] text-ink-faint">{sizeWords(newBytes)} of {cap.toLocaleString('en-US')} tokens{live && live.bytes > 0 ? ` · file now ${sizeWords(oldBytes)}` : ''}</span>
        <span className="flex-1" />
        <button type="button" onClick={reload} className="text-[11px] text-ink-faint hover:text-live">reload</button>
        <button type="button" disabled={backups.length === 0} onClick={() => setListing(!listing)} className="text-[11px] text-ink-faint hover:text-live disabled:opacity-50">
          restore previous{backups.length ? ` · ${backups.length}` : ''}
        </button>
      </div>
      {listing && (
        <ul className="divide-y divide-line/50 rounded-md ring-1 ring-line">
          {backups.map((b) => (
            <li key={b.ms} className="flex items-center gap-2 px-2 py-1 text-[11px]">
              <span className="num text-ink">{new Date(b.ms).toLocaleString()}</span>
              <span className="num text-ink-faint">{b.bytes.toLocaleString('en-US')} B · about {b.tokens.toLocaleString('en-US')} tokens</span>
              <span className="flex-1" />
              <button type="button" onClick={() => void pick(b)} className="text-ink-faint hover:text-live">load into the editor</button>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

/** `persona.add`: a key and optional name, emoji, card and soul. With no soul it awaits one, and the owner is asked once. */
function AddPersona({ disabled }: { disabled: boolean }) {
  const { write, busy, refused } = useOntologyWrite()
  const [f, setF] = useState({ key: '', name: '', emoji: '', card: '', soul: '' })
  const set = (k: keyof typeof f) => (e: { target: { value: string } }) => setF({ ...f, [k]: e.target.value })
  const add = async () => {
    const p = addParams(f)
    const out = await write(`Add the persona ${p.key}${p.soul ? ' with the soul below' : '? It will await its soul, and you will be asked for it once'}.`, 'persona.add', p)
    if (out) setF({ key: '', name: '', emoji: '', card: '', soul: '' })
  }
  return (
    <Panel help="ontology.personaadd" title="add a persona" icon={<Plus size={13} />}>
      <form className="flex flex-col gap-2 p-3" onSubmit={(e) => { e.preventDefault(); if (f.key.trim()) void add() }}>
        <input value={f.key} onChange={set('key')} placeholder="key" aria-label="persona key" disabled={disabled} className={field} />
        <input value={f.name} onChange={set('name')} placeholder="name (optional)" aria-label="persona name" disabled={disabled} className={field} />
        <input value={f.emoji} onChange={set('emoji')} placeholder="emoji (optional)" aria-label="persona emoji" disabled={disabled} className={field} />
        <input value={f.card} onChange={set('card')} placeholder="card: about 60 tokens Jev reads (optional)" aria-label="persona card" disabled={disabled} className={field} />
        <textarea value={f.soul} onChange={set('soul')} rows={4} placeholder="soul (optional: without one it awaits it)" aria-label="persona soul" disabled={disabled} className={cn(field, 'font-mono')} />
        <Refused words={refused} />
        <div className="flex items-center gap-2">
          <Act onClick={() => { if (f.key.trim()) void add() }} busy={busy} off={disabled} title="persona.add: confirmed first"><Plus size={13} /> Add persona</Act>
          {disabled && <span className="text-[11px] text-wait">the present only</span>}
        </div>
      </form>
    </Panel>
  )
}
