// A view that throws shows this instead of a blank page: what broke, and a way back. The rest of the cockpit
// (the rail, the header, the river) keeps running.
import { isRouteErrorResponse, useLocation, useNavigate, useRouteError } from 'react-router'
import { Compass, TriangleAlert } from 'lucide-react'
import { Btn, Panel } from './ui'

export function Crash() {
  const err = useRouteError()
  const nav = useNavigate()
  const msg = isRouteErrorResponse(err) ? `${err.status} ${err.statusText}` : err instanceof Error ? err.message : String(err)
  const stack = err instanceof Error ? err.stack : undefined
  return (
    <Panel title="This view stopped" icon={<TriangleAlert size={13} className="text-fault" />} bodyClassName="p-4">
      <div className="text-[13px] text-fault">{msg}</div>
      {stack && <pre className="mt-2 max-h-64 overflow-auto rounded-md bg-black/30 p-3 font-mono text-[11px] text-ink-faint ring-1 ring-line">{stack}</pre>}
      <div className="mt-3 flex gap-2">
        <Btn onClick={() => window.location.reload()}>Reload</Btn>
        <Btn tone="idle" onClick={() => nav('/')}>Back to the Bridge</Btn>
      </div>
    </Panel>
  )
}

export function NotFound() {
  const nav = useNavigate()
  const loc = useLocation()
  return (
    <Panel title="Nothing here" icon={<Compass size={13} />} bodyClassName="p-4">
      <div className="text-[13px] text-ink-dim">The cockpit has no view at <span className="num text-ink">{loc.pathname}</span>.</div>
      <div className="mt-3"><Btn onClick={() => nav('/')}>Back to the Bridge</Btn></div>
    </Panel>
  )
}
