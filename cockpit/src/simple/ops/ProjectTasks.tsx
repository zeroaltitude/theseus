// A project's "Tasks" (issue 37): the tasks that were involved in it, running first, each opening the task as the page's
// other rows do. It reads the `project.show` the Horizon section reads (one query key), so a push that refreshes one
// refreshes both; nothing here acts.
import type { ProjectShowResult } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { cn } from '@/lib/format'
import { Section } from '@/ops/InspectorSections'
import { taskHint, taskRows } from './projectTasks'

export function ProjectTasks({ project, now, onPick }: { project: string; now: number; onPick?: (id: string) => void }) {
  const { data } = useRpc<ProjectShowResult>('project.show', { id: project }, 0)
  if (!data) return null
  const rows = taskRows(data.tasks ?? [], now)
  return (
    <Section title="Tasks" hint={taskHint(data.project.tasks ?? 0, data.project.tasks_running ?? 0)}>
      <div data-testid="project-tasks">
        {rows.length === 0 ? <p className="text-[12px] text-ink-faint">No task has been started for this project, and none holds a step of it.</p> : (
          <ul className="flex flex-col gap-0.5 text-[12px]">
            {rows.map((t) => (
              <li key={t.id} className="flex items-baseline gap-2">
                <span aria-hidden className={t.running ? 'text-live' : 'text-ink-faint'}>●</span>
                {onPick ? <button type="button" onClick={() => onPick(t.id)} className={cn('min-w-0 flex-1 truncate text-left hover:underline', t.running ? 'text-ink' : 'text-ink-dim')}>{t.title}</button> : <span className="min-w-0 flex-1 truncate text-ink">{t.title}</span>}
                <span className="num shrink-0 text-[11px] text-ink-faint">{t.sub}</span>
              </li>
            ))}
          </ul>
        )}
        {data.tasks_more ? <p className="mt-1 text-[11.5px] text-ink-faint">… and {data.tasks_more} more, older</p> : null}
      </div>
    </Section>
  )
}
