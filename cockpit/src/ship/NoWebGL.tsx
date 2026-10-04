// The Ship's place when it can't draw (theseus-9k53): a plain page that says why and how to fix it, with the way to
// every other view, none of which needs WebGL. Before it, a browser with WebGL off got only the route's crash page on
// the cockpit's landing view.
import { Component, type ReactNode } from 'react'
import { Link } from 'react-router'
import { RotateCw, TriangleAlert } from 'lucide-react'
import { Panel } from '@/components/ui'
import { isWebGLFailure } from './webgl'

// The rail's views (Shell's NAV), less the Ship.
const VIEWS = [
  ['/bridge', 'Bridge'], ['/fleet', 'Fleet'], ['/actions', 'Actions'], ['/boundaries', 'Bounds'], ['/ledger', 'Ledger'],
  ['/money', 'Money'], ['/economics', 'Economics'], ['/speed', 'Speed'], ['/judgment', 'Judgment'],
  ['/systems', 'Systems'], ['/ontology', 'Ontology'],
] as const

export function ShipFallback({ reason }: { reason: string }) {
  const webgl = isWebGLFailure(reason)
  return (
    <div data-ship-fallback={webgl ? 'webgl' : 'error'} className="h-full overflow-auto p-6">
      <Panel title={webgl ? 'The Ship needs WebGL' : 'The Ship stopped'} icon={<TriangleAlert size={13} className="text-wait" />}
        className="mx-auto max-w-[760px]" bodyClassName="space-y-3 p-5 text-[13px] leading-relaxed text-ink-dim">
        {webgl ? (
          <>
            <p className="text-ink">The Ship draws its fleet in 3D with WebGL, and this browser has WebGL turned off, so the Ship can't draw here. Every other view works without it.</p>
            <div>
              <div className="text-ink">To turn WebGL back on:</div>
              <ol className="mt-1 list-decimal space-y-1 pl-5">
                <li>Turn on hardware acceleration in the browser's settings. In Chrome or Edge: Settings, System, "Use graphics acceleration when available". In Firefox: Settings, General, Performance.</li>
                <li>Restart the browser: the setting takes effect only then.</li>
                <li>Check its GPU page, <span className="num text-ink">chrome://gpu</span> (<span className="num text-ink">edge://gpu</span> in Edge, <span className="num text-ink">about:support</span> in Firefox): WebGL should read "Hardware accelerated". If it still reads "Disabled", the browser has blocked this machine's GPU.</li>
              </ol>
            </div>
          </>
        ) : (
          <p className="text-ink">The Ship hit an error and stopped drawing. Reload to try again; every other view works without it.</p>
        )}
        <div className="flex flex-wrap gap-1.5 pt-1">
          <button type="button" className="brass-button" onClick={() => window.location.reload()}><RotateCw size={12} /> Reload</button>
          {VIEWS.map(([to, label]) => <Link key={to} to={to} className="brass-button">{label}</Link>)}
        </div>
        <p className="num text-[11px] text-ink-faint">Cause: {reason}</p>
      </Panel>
    </div>
  )
}

/** Any later error in the Ship's own tree shows the same page in its place, not the route's crash page. */
export class ShipBoundary extends Component<{ children: ReactNode }, { failed: string | null }> {
  state = { failed: null as string | null }

  static getDerivedStateFromError(e: unknown) {
    return { failed: e instanceof Error ? e.message : String(e) }
  }

  render() {
    return this.state.failed === null ? this.props.children : <ShipFallback reason={this.state.failed} />
  }
}
