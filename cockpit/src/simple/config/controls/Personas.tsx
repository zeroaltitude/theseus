// Personas, the control: change one persona's short fields (`persona.set`: name, emoji, voice, how much it speaks first),
// and bring an agent over as a persona (`persona.import`, previewed first with no change, then done after asking). Adding,
// retiring and writing a persona’s character are the Personality tab's.
import { useState } from 'react'
import { UserRoundCog } from 'lucide-react'
import type { PersonaImportResult, PersonaListResult } from '@protocol'
import { Panel } from '@/components/ui'
import { useRpc } from '@/lib/rpc'
import { ActButton, FIELD } from '../ActButton'
import { personaChanges, personaImportWords } from './logic'

const LEVELS = ['quiet', 'normal', 'chatty'] as const

export function Personas() {
  const { data } = useRpc<PersonaListResult>('persona.list', {}, 15_000)
  const list = (data?.personas ?? []).filter((p) => p.key)
  const [key, setKey] = useState('')
  const p = list.find((x) => x.key === (key || list[0]?.key))
  const [edit, setEdit] = useState<Partial<Record<'name' | 'emoji' | 'tts_voice' | 'initiative', string>>>({})
  const changes = p?.key ? personaChanges(p, p.key, edit) : null
  const [from, setFrom] = useState('')
  const [name, setName] = useState('')
  const [preview, setPreview] = useState<PersonaImportResult | null>(null)
  const field = (f: 'name' | 'emoji' | 'tts_voice') => edit[f] ?? p?.[f] ?? ''
  return (
    <Panel help="simple.config.personas" title="Personas: change one, or bring one over" icon={<UserRoundCog size={13} />} bodyClassName="flex flex-col gap-4 p-4">
      <section className="flex flex-col gap-2">
        <h3 className="simple-eyebrow">Change a persona</h3>
        {!p ? <p className="text-[13px] text-ink-faint">There is no persona to change yet; add one on the Personality tab.</p> : (
          <div className="flex flex-wrap items-center gap-2">
            <select value={p.key} onChange={(e) => { setKey(e.target.value); setEdit({}) }} aria-label="which persona" className={FIELD}>
              {list.map((x) => <option key={x.key} value={x.key}>{x.key}</option>)}
            </select>
            <input value={field('name')} onChange={(e) => setEdit({ ...edit, name: e.target.value })} placeholder="name" aria-label="name" className={`${FIELD} w-[180px]`} />
            <input value={field('emoji')} onChange={(e) => setEdit({ ...edit, emoji: e.target.value })} placeholder="emoji" aria-label="emoji" className={`${FIELD} w-[80px]`} />
            <input value={field('tts_voice')} onChange={(e) => setEdit({ ...edit, tts_voice: e.target.value })} placeholder="voice" aria-label="voice" className={`${FIELD} w-[140px]`} />
            <select value={edit.initiative ?? p.initiative} onChange={(e) => setEdit({ ...edit, initiative: e.target.value })} aria-label="how much it speaks first" className={FIELD}>
              {LEVELS.map((l) => <option key={l} value={l}>speaks first: {l}</option>)}
            </select>
            <ActButton verb="persona_set" params={changes} label="Save" effect="Change this persona’s short fields as shown. Its character file is not touched." disabled={!changes} words={() => 'saved'} onDone={() => setEdit({})} />
          </div>
        )}
      </section>
      <section className="flex flex-col gap-2 border-t border-line pt-3">
        <h3 className="simple-eyebrow">Bring an agent over as a persona</h3>
        <div className="flex flex-wrap items-center gap-2">
          <input value={from} onChange={(e) => setFrom(e.target.value)} placeholder="where the agent’s files are" aria-label="where the agent’s files are" className={`${FIELD} min-w-[300px] flex-1`} spellCheck={false} />
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder="persona name" aria-label="persona name" className={`${FIELD} w-[160px]`} />
          <ActButton verb="persona_preview" params={{ from: from.trim(), name: name.trim(), dry_run: true }} label="Preview" effect="Show which files would come in. Nothing is changed." tone="plain" disabled={!from.trim() || !name.trim()} onDone={setPreview}
            words={(r) => `${r.files.length} files would come in${r.over_limit.length ? `, ${r.over_limit.length} over the size limit` : ''}${r.skipped.length ? `, ${r.skipped.length} skipped` : ''}`} />
          <ActButton verb="persona_import" params={{ from: from.trim(), name: name.trim(), dry_run: false }} label="Import" effect={personaImportWords(from, name)} disabled={!from.trim() || !name.trim() || !preview} submit="Yes, import it"
            words={(r) => `imported ${r.files.length} files as ${r.name}`} />
        </div>
        {preview && <p className="num text-[12px] text-ink-faint">{preview.files.map((f) => f.rel).slice(0, 8).join(' · ')}</p>}
      </section>
    </Panel>
  )
}
