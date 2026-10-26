// The per-test pop-out (theseus-1de57): "every test" on a benchmark card opens this table, a row opens a drawer. The
// words come from `lib/benchtasks.ts`'s ladder, always with the raw counts beside them; no per-task row carries a
// percentage. The table and drawer are driven from the address (`?tasks=on`, `?task=<id>`), so a link reopens them.
import { ListChecks } from 'lucide-react'
import { Drawer, Section } from './CallInspector'
import { Panel } from './ui'
import { usdWords, wallWords, type TaskRow } from '@/lib/benchtasks'

export function BenchTasks({ rows, label, selected, onSelect }: {
  rows: TaskRow[]; label: string; selected: string | null; onSelect: (task: string | null) => void
}) {
  const row = rows.find((r) => r.task === selected)
  return (
    <>
      <Panel help={false} title={`Every test · ${label}`} icon={<ListChecks size={14} />} bodyClassName="p-0">
        <p className="px-4 pt-2 text-[11px] text-ink-faint">
          one row a task, counted within this one report (the same tasks and attempts for every arm): attempts solved of attempts made. A few attempts are a count, never a rate.
        </p>
        <div className="max-h-[480px] overflow-auto">
          <table className="viz-table w-full text-[12px]">
            <caption className="sr-only">every task of {label}: Theseus against the best other arm, with its cost and wall time</caption>
            <thead>
              <tr>
                <th scope="col" className="text-left">task</th><th scope="col" className="text-left">Theseus</th>
                <th scope="col" className="text-left">best other arm</th><th scope="col" className="text-left">$ a trial (Theseus)</th>
                <th scope="col" className="text-left">wall time (Theseus)</th><th scope="col" className="text-left">where it stands</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.task} className="cursor-pointer align-top" tabIndex={0} aria-selected={r.task === selected}
                  onClick={() => onSelect(r.task)} onKeyDown={(e) => { if (e.key === 'Enter') onSelect(r.task) }}>
                  <td className="font-mono text-[11px] text-ink">{r.task}</td>
                  <td className="num">{r.theseus ? `${r.theseus.k} of ${r.theseus.n}` : 'did not run'}</td>
                  <td className="num">{r.best ? `${r.best.k} of ${r.best.n}` : 'no other arm'} <span className="text-ink-faint">{r.best?.name}</span></td>
                  <td className="num text-ink-dim">{usdWords(r.theseus?.usd)}</td>
                  <td className="num text-ink-dim">{wallWords(r.theseus?.wall)}</td>
                  <td className="!whitespace-normal text-ink-dim">{r.assessment.word}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Panel>
      {row && <TaskDrawer row={row} onClose={() => onSelect(null)} />}
    </>
  )
}

function TaskDrawer({ row, onClose }: { row: TaskRow; onClose: () => void }) {
  const a = row.assessment
  return (
    <Drawer onClose={onClose} head={<span className="font-mono text-[13px] text-ink">{row.task}</span>}>
      <div className="min-h-0 flex-1 overflow-auto px-4 py-3 text-[12px]">
        <Section title="Where Theseus stands">
          <p className="text-ink"><b className="font-semibold">{a.word}</b></p>
          <p className="mt-0.5 text-ink-dim">{a.counts}{row.best ? ` (compared with ${a.against})` : ''}</p>
        </Section>
        <Section title="Every arm on this task">
          <table className="viz-table w-full">
            <thead><tr><th scope="col" className="text-left">arm</th><th scope="col" className="text-left">solved</th><th scope="col" className="text-left">$ a trial</th><th scope="col" className="text-left">wall time</th></tr></thead>
            <tbody>
              {row.sides.map((s) => (
                <tr key={s.arm}><td>{s.name}</td><td className="num">{s.k} of {s.n}</td><td className="num text-ink-dim">{usdWords(s.usd)}</td><td className="num text-ink-dim">{wallWords(s.wall)}</td></tr>
              ))}
            </tbody>
          </table>
        </Section>
      </div>
    </Drawer>
  )
}
