// Configuration, the map: every setting of Theseus that was found, in plain words, and where it is changed. What can be
// changed while Theseus runs has its control one click away (a tab of this page, which is the Ship's own component with
// its own confirmation, or a page that already holds it); what is set in the configuration file says so, with what
// Theseus is running with now; and what can change live but only at the command line says the command. The inventory is
// `settings.ts`, held by tests to the daemon's method names and to what the cockpit actually calls.
import { Link } from 'react-router'
import { ArrowRight, FileCog, SlidersHorizontal, Wrench } from 'lucide-react'
import type { Health } from '@protocol'
import { Panel, Pill } from '@/components/ui'
import { useRpc } from '@/lib/rpc'
import { CLI_ONLY, FIXED, LIVE, type ConfigTab, type LiveSetting } from './settings'

/** The tab names as the page's tabs say them (`Config.tsx` has the same words). */
const TAB_WORDS: Record<ConfigTab, string> = {
  settings: 'All settings', personality: 'Personality', ontology: 'Ontology and people', standing: 'Standing rules', policy: 'Policy',
  approvals: 'Tools and approvals', packs: 'Judging', controls: 'More controls', system: 'Models and system',
}

const addressOf = (s: LiveSetting) => ('tab' in s.at ? `${s.at.tab === 'settings' ? '/config' : `/config?v=${s.at.tab}`}${s.at.section ? `#${s.at.section}` : ''}` : s.at.path)
const wordsOf = (s: LiveSetting) => ('tab' in s.at ? `${TAB_WORDS[s.at.tab]} tab` : s.at.label)

export default function Settings() {
  const { data: h } = useRpc<Health>('health', undefined, 15_000)
  return (
    <div className="flex flex-col gap-3">
      <Panel help="simple.config.live" title={<>Change it now · {LIVE.length}</>} icon={<SlidersHorizontal size={13} />} bodyClassName="divide-y divide-line">
        {LIVE.map((s) => (
          <div key={s.id} className="flex flex-wrap items-baseline gap-x-4 gap-y-1 px-4 py-2.5">
            <div className="w-[260px] shrink-0 text-[13.5px] font-semibold text-ink">{s.name}</div>
            <p className="min-w-[280px] flex-1 text-[13px] leading-snug text-ink-dim">{s.words}</p>
            <Link to={addressOf(s)} className="simple-chip shrink-0" title={s.methods.join(' · ')}>{wordsOf(s)}<ArrowRight size={12} /></Link>
          </div>
        ))}
      </Panel>

      <Panel help="simple.config.fixed" title={<>Set in the configuration file · {FIXED.length}</>} icon={<FileCog size={13} />} bodyClassName="divide-y divide-line"
        actions={h?.config ? <Pill tone={h.config.state === 'held' ? 'wait' : 'ok'} title="where Theseus read its configuration from, and whether that copy is confirmed">{h.config.state}</Pill> : undefined}>
        <p className="px-4 py-2.5 text-[13px] leading-snug text-ink-dim">
          Theseus reads its configuration file once, when it starts. To change any of these, edit the file and restart Theseus; nothing here can be changed while it runs.
          {h?.config ? <> Right now it is running on the configuration it read from <span className="text-ink">{h.config.source}</span>.</> : null}
        </p>
        {FIXED.map((s) => {
          const now = h ? s.now?.(h) : undefined
          return (
            <div key={s.id} className="flex flex-wrap items-baseline gap-x-4 gap-y-1 px-4 py-2.5">
              <div className="w-[260px] shrink-0 text-[13.5px] font-semibold text-ink">{s.name}</div>
              <div className="min-w-[280px] flex-1">
                <p className="text-[13px] leading-snug text-ink-dim">{s.words}</p>
                {now && <p className="num mt-0.5 text-[12px] text-ink">{now}</p>}
              </div>
              <div className="w-[260px] shrink-0 text-[12px] text-ink-faint" title="the sections of the configuration file">{s.tables.map((t) => <code key={t} className="num mr-1.5">[{t}]</code>)}</div>
            </div>
          )
        })}
      </Panel>

      <Panel help="simple.config.cli" title={<>Only at the command line · {CLI_ONLY.length}</>} icon={<Wrench size={13} />} bodyClassName="divide-y divide-line">
        {CLI_ONLY.map((s) => (
          <div key={s.id} className="flex flex-wrap items-baseline gap-x-4 gap-y-1 px-4 py-2.5" title={s.methods.join(' · ')}>
            <div className="w-[260px] shrink-0 text-[13.5px] font-semibold text-ink">{s.name}</div>
            <p className="min-w-[280px] flex-1 text-[13px] leading-snug text-ink-dim">{s.words}</p>
            <code className="num w-[260px] shrink-0 text-[12px] text-ink-faint">{s.command}</code>
          </div>
        ))}
      </Panel>
    </div>
  )
}
