// Run an MCP server's prompt as this session's next turn (M7 step 36c): the picker beside the composer. It lists the
// prompts the servers offer (`mcp.prompt.list`), a field per argument of the one picked (a required one marked), and
// sends `turn.submit { prompt }`. The daemon asks the server, writes the messages as the turn's input, and refuses a
// shared place's session; its refusal shows here as it is. The reply streams into the transcript as any turn's does.
import { useState } from 'react'
import { ScrollText } from 'lucide-react'
import type { McpPromptListResult, ProviderErrorData } from '@protocol'
import { call, useRpc } from '@/lib/rpc'
import { fieldLabel, missingRequired, promptRef, sortedPrompts } from '@/lib/prompts'
import { cn } from '@/lib/format'

export function PromptPicker({ sessionId, profile, onError }: { sessionId: string; profile: string; onError: (m: string | null) => void }) {
  const [open, setOpen] = useState(false)
  const { data } = useRpc<McpPromptListResult>('mcp.prompt.list', undefined, 10_000, { enabled: open })
  const [picked, setPicked] = useState('')
  const [values, setValues] = useState<Record<string, string>>({})
  const [running, setRunning] = useState(false)
  const prompts = sortedPrompts(data?.prompts ?? [])
  const prompt = prompts.find((p) => p.name === picked)
  const missing = prompt ? missingRequired(prompt, values) : []
  const run = () => {
    if (!prompt || missing.length) return
    setRunning(true)
    onError(null)
    // The call resolves when the turn ends, as a send's does; the picker closes at once, and the transcript follows.
    call('turn.submit', { session_id: sessionId, prompt: promptRef(prompt, values), profile: profile || undefined })
      .catch((e: any) => {
        const d = e?.data as ProviderErrorData | undefined
        onError(`${d?.class ? `turn failed · class ${d.class}: ` : ''}${e?.message ?? String(e)}`)
      })
      .finally(() => setRunning(false))
    setOpen(false)
    setValues({})
  }
  return (
    <div className="relative">
      <button onClick={() => setOpen((o) => !o)} title="Run an MCP server's prompt as the next turn" aria-label="MCP prompt"
        className={cn('grid h-9 w-9 place-items-center rounded-lg ring-1 transition-colors',
          open ? 'bg-live/15 text-live ring-live/40' : 'text-ink-dim ring-line hover:text-ink')}>
        <ScrollText size={16} />
      </button>
      {open && (
        <div className="absolute bottom-11 right-0 z-20 w-80 rounded-lg bg-hull p-3 text-[12px] shadow-xl ring-1 ring-line">
          {prompts.length === 0 ? (
            <div className="text-ink-faint">No MCP server offers a prompt. Attach one under [mcp.servers] in the config.</div>
          ) : (
            <div className="flex flex-col gap-2">
              <select value={picked} onChange={(e) => { setPicked(e.target.value); setValues({}) }} aria-label="prompt"
                className="h-8 rounded-md bg-white/[0.04] px-2 text-ink outline-none ring-1 ring-line">
                <option value="">pick a prompt…</option>
                {prompts.map((p) => <option key={p.name} value={p.name}>{p.name}{p.stored ? ' (stored)' : ''}</option>)}
              </select>
              {prompt?.description && <div className="text-ink-dim">{prompt.description}</div>}
              {prompt?.arguments.map((a) => (
                <label key={a.name} className="flex flex-col gap-0.5">
                  <span className={cn('text-ink-dim', a.required && 'text-ink')}>{fieldLabel(a)}{a.description ? ` · ${a.description}` : ''}</span>
                  <input value={values[a.name] ?? ''} onChange={(e) => setValues((v) => ({ ...v, [a.name]: e.target.value }))}
                    onKeyDown={(e) => { if (e.key === 'Enter') run() }}
                    className="h-8 rounded-md bg-white/[0.04] px-2 text-ink outline-none ring-1 ring-line focus:ring-live/40" />
                </label>
              ))}
              {prompt && missing.length > 0 && <div className="text-fault">needs {missing.join(', ')}</div>}
              <button onClick={run} disabled={!prompt || missing.length > 0 || running}
                className={cn('h-8 rounded-md ring-1 transition-colors',
                  prompt && !missing.length ? 'bg-live/15 text-live ring-live/40 hover:bg-live/25' : 'text-ink-faint ring-line')}>
                Run as the next turn
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  )
}
