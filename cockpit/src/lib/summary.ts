// One short line for any ledger row: what happened, in the daemon's own terms. Used by the activity river and the
// ledger explorer. Unknown kinds fall back to their first few fields.
import type { LedgerEntry } from '@protocol'
import { ms, short, tokens, usd } from './format'

type D = Record<string, any>

export function summarize(r: LedgerEntry): string {
  const d = (r.data ?? {}) as D
  switch (r.kind) {
    case 'provider.call':
      return `${d.provider}/${d.model} loop ${d.loop} · ${tokens((d.usage?.input_tokens ?? 0) + (d.usage?.cache_read_input_tokens ?? 0) + (d.usage?.cache_creation_input_tokens ?? 0))} in, ${tokens(d.usage?.output_tokens)} out · ${usd(d.cost_usd)} · ${ms(d.timing?.total_ms)} · ${d.stop_reason ?? ''}`
    case 'loop.ended':
      return `loop ${d.loop} ended: ${d.outcome?.provider_stop_reason ?? ''}, ${d.outcome?.tool_calls ?? 0} tool calls → ${d.decision?.decision ?? ''}`
    case 'loop.started': return `loop ${d.loop ?? ''} started`
    case 'turn.started': return `turn started${d.input_chars ? ` · ${d.input_chars} chars in` : ''}`
    case 'turn.ended':
      return `turn ended: ${d.loops} loops, ${d.tool_calls ?? 0} tools, ${usd(d.cost_usd)}, ${ms(d.elapsed_ms)} (first token ${ms(d.first_token_ms)})`
    case 'turn.failed': return `turn failed: ${d.class ?? d.error ?? ''}`
    case 'context.compiled':
      return `context ${d.decision}: ${tokens(d.est_tokens)} tokens, ${d.messages} messages (${d.strategy ?? ''})`
    case 'action.planned': return `${d.tool} planned (${d.retry_class?.class ?? ''})`
    case 'action.authorized': return `${short(d.correlation_id)} authorized`
    case 'action.dispatched': return `${short(d.correlation_id)} dispatched`
    case 'action.succeeded': return `${d.producer ?? short(d.correlation_id)} succeeded in ${ms(d.duration_ms)}`
    case 'action.failed': return `${d.producer ?? short(d.correlation_id)} failed${d.error ? `: ${String(d.error).slice(0, 80)}` : ''}`
    case 'tool.job_started': return `job pid ${d.pid}: ${(d.argv ?? []).join(' ').slice(0, 90)}`
    case 'tool.denied': return `${d.tool ?? ''} denied${d.reason ? `: ${d.reason}` : ''}`
    case 'tool.confirm_requested': return `${d.tool ?? ''} asks for approval`
    case 'execution.waiting': return `execution waits (turn ${d.turn}, ${ms(d.turn_ms)})`
    case 'execution.running': return 'execution running'
    case 'execution.queued': return 'execution queued'
    case 'discord.message.out': return `→ ${d.place ?? ''} · ${d.chars ?? 0} chars`
    case 'discord.message.in': return `← ${d.place ?? ''}${d.chars ? ` · ${d.chars} chars` : ''}`
    case 'startup.step': return `startup step ${d.step}: ${d.name}`
    case 'hook.site': return `hook ${d.event} · ${d.handlers} handlers → ${d.outcome}`
    case 'secrets.resolved': return `secrets resolved: ${(d.names ?? []).length} in ${ms(d.ms)}`
    case 'server.started': case 'server.serving': return r.kind.replace('server.', 'server ')
    // The web UI's door (theseus-70f, 3qf, zab): one row per kind a minute, with the count it stands for.
    case 'web.refused': {
      const by: Record<string, string> = { host: 'by address', origin: 'by page', peer: 'by user' }
      const n = d.count ?? 1
      return `web UI turned away ${n} connection${n === 1 ? '' : 's'} ${by[d.why] ?? d.why ?? ''}${d.last?.why ? `: ${String(d.last.why).slice(0, 90)}` : ''}`
    }
    case 'web.dev_origin': {
      const n = d.count ?? 1
      return `web UI served the dev page ${n}× (${d.last?.origin ?? '?'} via ${d.last?.host ?? '?'})`
    }
    default: {
      const keys = Object.entries(d).filter(([, v]) => typeof v !== 'object').slice(0, 3)
      return keys.map(([k, v]) => `${k}=${String(v).slice(0, 40)}`).join(' · ')
    }
  }
}
