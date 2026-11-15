// Projects, the control: which repositories Theseus trusts (`project.trusted` read; trust and stop trusting), and starting a
// project. Trusting a folder lets its AGENTS.md and CLAUDE.md shape Theseus's behaviour, so it asks first and says so.
import { useState } from 'react'
import { FolderGit2 } from 'lucide-react'
import type { ProjectTrustedResult } from '@protocol'
import { Empty, Panel } from '@/components/ui'
import { when } from '@/lib/explorer'
import { useRpc } from '@/lib/rpc'
import { ActButton, FIELD } from '../ActButton'
import { absolute, projectParams, projectWords, trustWords, untrustWords } from './logic'

export function Projects() {
  const { data } = useRpc<ProjectTrustedResult>('project.trusted', {}, 10_000)
  const [path, setPath] = useState('')
  const [title, setTitle] = useState('')
  const [objective, setObjective] = useState('')
  const [acceptance, setAcceptance] = useState('')
  const params = projectParams(title, objective, acceptance)
  return (
    <Panel help="simple.config.projects" title="Projects and the folders Theseus trusts" icon={<FolderGit2 size={13} />} bodyClassName="flex flex-col gap-4 p-4">
      <section className="flex flex-col gap-2">
        <h3 className="simple-eyebrow">Folders Theseus trusts</h3>
        {!data ? <Empty>reading…</Empty> : !data.trusted.length ? <p className="text-[13px] text-ink-faint">Theseus trusts no project folder yet.</p> : (
          <ul className="divide-y divide-line">
            {data.trusted.map((g) => (
              <li key={g.id} className="flex flex-wrap items-center gap-x-3 gap-y-1 py-2">
                <span className="num min-w-0 flex-1 truncate text-[13px] text-ink" title={g.root}>{g.root}</span>
                <span className="text-[12px] text-ink-faint">trusted by {g.by}, {when(g.at_ms)}</span>
                <ActButton verb="project_untrust" params={{ path: g.root }} label="Stop trusting" effect={untrustWords(g.root)} tone="fault" words={(r) => `no longer trusted: ${r.root ?? g.root}`} />
              </li>
            ))}
          </ul>
        )}
        <div className="flex flex-wrap items-center gap-2">
          <input value={path} onChange={(e) => setPath(e.target.value)} placeholder="the full path of a folder in the repository" aria-label="folder to trust" className={`${FIELD} min-w-[320px] flex-1`} spellCheck={false} />
          <ActButton verb="project_trust" params={{ path: path.trim() }} label="Trust this folder" effect={trustWords(path)} disabled={!absolute(path)} submit="Yes, trust it" words={(r) => `now trusted: ${r.root ?? path.trim()}`} />
        </div>
      </section>
      <section className="flex flex-col gap-2 border-t border-line pt-3">
        <h3 className="simple-eyebrow">Start a project</h3>
        <input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="what the project is called (one line)" aria-label="project title" className={FIELD} />
        <input value={objective} onChange={(e) => setObjective(e.target.value)} placeholder="what it is for (optional)" aria-label="project objective" className={FIELD} />
        <textarea value={acceptance} onChange={(e) => setAcceptance(e.target.value)} rows={3} placeholder="what done looks like, one line each" aria-label="what done looks like" className={FIELD} />
        <div className="flex items-center gap-2">
          <ActButton verb="project_create" params={params} label="Start the project" effect={projectWords(title, params.acceptance.length)} disabled={!title.trim() || !params.acceptance.length} submit="Yes, start it"
            onDone={() => { setTitle(''); setObjective(''); setAcceptance('') }} words={(r) => `started “${r.project?.title ?? title}”`} />
        </div>
      </section>
    </Panel>
  )
}
