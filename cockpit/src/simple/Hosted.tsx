// A simple page that shows instrument pages of the cockpit inside the plain frame: its tabs in the address (`?v=`),
// one view at a time, each loaded on first visit. The view is the Ship's own component, so its panes, their "?",
// its reads and its controls are exactly the Ship's; only the look and the way in differ.
import { Suspense, type ComponentType, type LazyExoticComponent } from 'react'
import { Page, Tabs, useTab, type TabDef } from './kit'

export interface HostedTab<T extends string> extends TabDef<T> { view: LazyExoticComponent<ComponentType> | ComponentType }

export function Hosted<T extends string>({ title, sub, tabs }: { title: string; sub: string; tabs: readonly HostedTab<T>[] }) {
  const [tab, setTab] = useTab(tabs)
  const View = tabs.find((t) => t.id === tab)!.view
  return (
    <Page fill title={title} sub={sub} actions={tabs.length > 1 ? <Tabs tabs={tabs} value={tab} onChange={setTab} label={`${title}: what to show`} /> : undefined}>
      <div className="min-h-0 flex-1 overflow-auto">
        <Suspense fallback={<div className="p-6 text-[13px] text-ink-faint">Loading…</div>}>
          <View key={tab} />
        </Suspense>
      </div>
    </Page>
  )
}
