// Health's binary (theseus-od13): a loud card at the top of Systems when this daemon's jobs can write the binary it
// runs, and one line in the Daemon card otherwise; the header's dot and the card share `lib/binary.ts`.
import { ShieldAlert } from 'lucide-react'
import type { BinaryStatus } from '@protocol'
import { binaryLine, binaryTone } from '@/lib/binary'
import { cn } from '@/lib/format'
import { toneClass } from '@/lib/taxonomy'
import { Field, Panel, Pill } from './ui'

/** The fault card; nothing in any other state. */
export function BinaryFault({ binary }: { binary?: BinaryStatus }) {
  if (binaryTone(binary) !== 'fault') return null
  return (
    <Panel title="Binary · jobs can write it" icon={<ShieldAlert size={13} />} bodyClassName="px-3.5 py-2.5">
      <div className={cn('rounded-md px-2.5 py-2 text-[12px] ring-1', toneClass.fault)}>{binaryLine(binary)}</div>
      <Field label="path" mono>{binary?.path}</Field>
      <Field label="what is writable">{binary?.detail}</Field>
    </Panel>
  )
}

/** The Daemon card's line: ok is quiet, unknown waits, a fault repeats the card's state. */
export function BinaryField({ binary }: { binary?: BinaryStatus }) {
  const tone = binaryTone(binary)
  return (
    <Field label="binary" mono>
      <span title={binaryLine(binary)}>
        <Pill tone={tone}>{binary?.state ? binary.state.replaceAll('_', ' ') : 'not reported'}</Pill>
      </span>
    </Field>
  )
}
