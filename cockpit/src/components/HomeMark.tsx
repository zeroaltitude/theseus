// The Theseus mark at the top left, as the way home (theseus-j4cjc): the rail's coin and the heartbeat bar's
// wordmark both wear it, so a press on either lands on the root URL from every view and route.
//
// It is a real anchor, not a button: the middle button and ⌘-click open a tab, the address shows on hover, and the
// browser's own back and forward still mean what they mean. `lib/home.ts` holds the rules; this binds them to the
// two stores a fresh load of `/` starts clean.
import type { ReactNode } from 'react'
import { Link } from 'react-router'
import { HOME, pressedHome } from '@/lib/home'
import { useAsOf } from '@/lib/timemachine'
import { useSelection } from '@/ops/useSelection'

export function HomeMark({ title, className, children }: { title: string; className?: string; children: ReactNode }) {
  return (
    <Link
      to={HOME}
      title={title}
      className={className}
      onClick={(e) => pressedHome(e, {
        live: () => useAsOf.getState().set(null),
        clearSelection: () => useSelection.getState().clear(),
      })}
    >
      {children}
    </Link>
  )
}
