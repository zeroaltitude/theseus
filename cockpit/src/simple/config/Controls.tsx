// Configuration, More controls: the settings that can change while Theseus runs and had no control elsewhere, twelve panels
// in the order a person is likely to want them. Each act is one protocol call through `ActButton`, whose posture (one press,
// two presses, or a sheet that says what will happen) comes from `verbs.ts`; each panel reads what it needs from the
// daemon's own reads. A panel that cannot act (Google, whose methods the daemon answers only to its own command line)
// says where the act is done. A link from the "All settings" tab opens this tab at its panel (`#id`).
import { useEffect, type ReactNode } from 'react'
import { useLocation } from 'react-router'
import type { Health } from '@protocol'
import { X } from 'lucide-react'
import { useRpc } from '@/lib/rpc'
import { cn } from '@/lib/format'
import { useNotes } from './notes'
import { Google } from './controls/Google'
import { Imports } from './controls/Imports'
import { Initiative, SelfPause } from './controls/Itself'
import { Judging } from './controls/Judging'
import { Kinds } from './controls/Kinds'
import { Personas } from './controls/Personas'
import { Projects } from './controls/Projects'
import { Standing } from './controls/Standing'
import { Index, Pins, Servers } from './controls/Upkeep'

/** What the controls said, newest first: the daemon's own words after each act, kept after its row has gone, and held at the
 *  top of the window while the page scrolls under it, so an act far down the page is answered where it can be seen. */
function Notes() {
  const { notes, drop } = useNotes()
  if (!notes.length) return null
  return (
    <div id="notes" className="sticky top-0 z-10 flex flex-col gap-1.5 pb-1.5" style={{ background: 'var(--color-void)' }}>
      {notes.map((n) => (
        <div key={n.id} role="status" className={cn('flex items-center gap-2 rounded-md px-3 py-1.5 text-[13px] ring-1 ring-inset', n.ok ? 'text-ink ring-ok/40' : 'text-fault ring-fault/40')} style={{ background: 'var(--color-hull)' }}>
          <span className="min-w-0 flex-1">{n.text}</span>
          <button type="button" onClick={() => drop(n.id)} aria-label="dismiss" title="dismiss" className="text-ink-faint hover:text-ink"><X size={13} /></button>
        </div>
      ))}
    </div>
  )
}

export default function Controls() {
  const { data: h } = useRpc<Health>('health', undefined, 5000)
  const { hash } = useLocation()
  // The tab is lazy, so the browser has no panel to scroll to when it follows the link: do it once the panels are drawn.
  useEffect(() => { if (hash) document.getElementById(hash.slice(1))?.scrollIntoView({ block: 'start' }) }, [hash])
  const at = (id: string, node: ReactNode) => <div id={id} className="scroll-mt-4">{node}</div>
  return (
    <div className="flex flex-col gap-3">
      <Notes />
      {at('projects', <Projects />)}
      {at('standing', <Standing />)}
      {at('personas', <Personas />)}
      {at('kinds', <Kinds />)}
      {at('judging', <Judging />)}
      {at('initiative', <Initiative />)}
      {at('selfpause', <SelfPause h={h} />)}
      {at('servers', <Servers h={h} />)}
      {at('index', <Index h={h} />)}
      {at('pins', <Pins />)}
      {at('imports', <Imports />)}
      {at('google', <Google />)}
    </div>
  )
}
