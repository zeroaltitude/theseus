// The "?" near a panel's top-right corner (theseus-onxwh): a dismissable popover that says what the panel's readout is
// for. The words are `lib/explainers.ts`, keyed by the panel's `help` id; the brass frame (`Panel`) draws it, so every
// panel gets it from one place. Escape or a click outside closes it; the button is a real button with a label.
import { CircleHelp, X } from 'lucide-react'
import { Popover } from 'radix-ui'
import { explainerOf, PARTS } from '@/lib/explainers'

export function HelpButton({ id, title }: { id: string; title?: string }) {
  const e = explainerOf(id)
  if (!e) return null
  return (
    <Popover.Root>
      <Popover.Trigger asChild>
        <button type="button" data-help={id} aria-label={`What this panel shows${title ? `: ${title}` : ''}`}
          title="What is this panel? Its goal, how to read it, why it matters"
          className="grid h-5 w-5 shrink-0 place-items-center rounded-full border border-line text-ink-faint transition-colors hover:border-gold hover:text-gold focus-visible:outline-2 focus-visible:outline-live data-[state=open]:border-gold data-[state=open]:text-gold">
          <CircleHelp size={12} aria-hidden />
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content side="bottom" align="end" sideOffset={6} collisionPadding={12} aria-label="About this panel"
          className="brass-card z-50 w-[min(380px,calc(100vw-24px))] !p-3 text-[12px] leading-snug text-ink-dim shadow-2xl">
          <div className="mb-1.5 flex items-center gap-2">
            <span className="ship-engraved text-[10px]">About this panel</span>
            <Popover.Close aria-label="Close" className="ml-auto grid h-5 w-5 place-items-center rounded text-ink-faint hover:text-ink"><X size={12} aria-hidden /></Popover.Close>
          </div>
          <dl className="flex flex-col gap-2">
            {PARTS.map(([key, label]) => (
              <div key={key}>
                <dt className="font-display text-[9.5px] font-bold uppercase tracking-[0.12em] text-gold">{label}</dt>
                <dd className="mt-0.5 text-ink">{e[key]}</dd>
              </div>
            ))}
          </dl>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  )
}
