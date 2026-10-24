// The Operations page's part of ⌘K (theseus-jid7t.6), inside the shell's palette: the verbs over a scope (the focused
// card, all visible, or the selection; ⇥ cycles), then every entity the page holds. A verb is "pause", "switch model to
// deep", "answer…": the verb then the target, or the target then the verb, since cmdk matches words in any order. An
// entry no target takes is greyed with the reason, in the floor's or the daemon's words; the shell's other groups stay.
import { useMemo } from 'react'
import { Command } from 'cmdk'
import { useQueryClient } from '@tanstack/react-query'
import { Lock } from 'lucide-react'
import type { SessionProfileResult } from '@protocol'
import { cn } from '@/lib/format'
import { useOpsAct } from './act'
import { SCOPE_WORDS, scopeTargets, searchValue, verbEntries, type Entry } from './palette'
import { useOpsPalette } from './paletteStore'
import { profileToast } from './menus'
import { useOpsUi } from './ui'

const ITEM = 'flex cursor-pointer items-center gap-2.5 rounded-lg px-2.5 py-2 text-[13px] text-ink data-[selected=true]:bg-live/10'
const HEAD = 'text-[11px] text-ink-faint [&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5'

export function OpsPalette({ close }: { close: () => void }) {
  const feed = useOpsPalette((s) => s.feed)
  const scope = useOpsPalette((s) => s.scope)
  const { post } = useOpsAct()
  const qc = useQueryClient()
  const say = useOpsUi((s) => s.say)
  const targets = useMemo(() => (feed ? scopeTargets(scope, { focused: feed.targets.one[0], visible: feed.targets.visible, selection: feed.targets.selection }) : []), [feed, scope])
  const entries = useMemo(() => (feed ? verbEntries(targets, { profiles: feed.profiles, prices: feed.prices }) : []), [feed, targets])
  if (!feed) return null
  const run = async (e: Entry) => {
    close()
    if (e.opens) { feed.newTask(); return }
    const first = e.plans[0]
    if (first?.ask && !window.confirm(first.ask)) return
    let done = 0
    const refused: string[] = []
    let undo: (() => void) | undefined
    for (const p of e.plans) {
      try {
        const r = await post<SessionProfileResult>(p.verb as never, p.method, p.params)
        if (r?.refused) refused.push(r.refused)
        else {
          done++
          if (p.method === 'session.profile' && e.plans.length === 1) {
            const t = profileToast(r, (p.params as { profile: string | null }).profile)
            if (t.undo !== undefined) undo = () => void post(p.verb as never, p.method, { ...(p.params as object), profile: t.undo }).then(() => qc.invalidateQueries())
          }
        }
      } catch (err: any) { refused.push(err?.message ?? String(err)) }
    }
    void qc.invalidateQueries()
    say(`${e.label}: ${done} done${refused.length ? ` · ${refused.length} refused: ${refused[0]}` : ''}`, undo)
  }
  return (
    <>
      <Command.Group heading={`Operations · ${SCOPE_WORDS[scope]} · ${targets.length} (⇥ changes the scope)`} className={HEAD}>
        {entries.map((e) => (
          <Command.Item key={e.id} value={searchValue(e.label, SCOPE_WORDS[scope])} keywords={[e.id]} disabled={!!e.grey} onSelect={() => void run(e)} className={cn(ITEM, e.grey && 'opacity-50')}>
            <span>{e.label}</span>
            <span className="num truncate text-[11px] text-ink-faint">{e.detail}</span>
            {e.grey && <span className="ml-auto flex items-center gap-1 text-[11px] text-fault"><Lock size={10} /> {e.grey}</span>}
          </Command.Item>
        ))}
        <Command.Item value="self changes what theseus changed about itself" onSelect={() => { close(); feed.selfChanges() }} className={ITEM}>Self changes <span className="ml-auto text-[11px] text-ink-faint">what Theseus changed about itself</span></Command.Item>
      </Command.Group>
      <Command.Group heading="Operations · everything on the page" className={HEAD}>
        {feed.entities.map((en) => (
          <Command.Item key={`${en.kind}:${en.id}`} value={`${en.kind} ${en.label} ${en.id}`} onSelect={() => { close(); feed.go(en) }} className={ITEM}>
            <span className="truncate">{en.label}</span><span className="num ml-auto shrink-0 text-[11px] text-ink-faint">{en.hint}</span>
          </Command.Item>
        ))}
      </Command.Group>
    </>
  )
}
