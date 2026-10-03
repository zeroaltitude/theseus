// One short line for any ledger row: what happened, in the daemon's own terms. Used by the activity river, the
// ledger explorer, and the session deck's rows. Unknown kinds fall back to their first few fields. Pure, with no
// import but the protocol's types and the pure figures, so `node --test` runs its test (`test/summary.test.ts`).
import type { LedgerEntry } from '@protocol'
import { ms, short, tokens, usd } from './figures.ts'

type D = Record<string, any>

/** A field as text: a string as it is, anything else as JSON, nothing as empty. */
const text = (v: unknown): string => (v == null ? '' : typeof v === 'string' ? v : JSON.stringify(v))
const n = (v: unknown): string => (v == null ? '' : String(v))
const count = (v: unknown): number => (Array.isArray(v) ? v.length : 0)

export function summarize(r: LedgerEntry): string {
  const d = (r.data ?? {}) as D
  const s = (k: string) => text(d[k])
  switch (r.kind) {
    case 'provider.call':
      return `${d.provider}/${d.model} loop ${d.loop} · ${tokens((d.usage?.input_tokens ?? 0) + (d.usage?.cache_read_input_tokens ?? 0) + (d.usage?.cache_creation_input_tokens ?? 0))} in, ${tokens(d.usage?.output_tokens)} out · ${usd(d.cost_usd)} · ${ms(d.timing?.total_ms)} · ${d.stop_reason ?? ''}`
    case 'provider.error':
      return `${s('class')}${d.transient ? ' transient' : ''}${d.usage_unknown ? ' usage unknown' : ''} · ${s('message')}`
    case 'loop.ended':
      return `loop ${d.loop} ended: ${d.outcome?.provider_stop_reason ?? ''}, ${d.outcome?.tool_calls ?? 0} tool calls → ${d.decision?.decision ?? ''}`
    case 'loop.started': return `loop ${d.loop ?? ''} started`
    case 'turn.started':
      return `turn started${d.model ? ` · ${s('profile')} → ${s('provider')}/${s('model')}` : ''}${d.input_chars ? ` · ${d.input_chars} chars in` : ''}`
    case 'turn.ended':
      return `turn ended: ${d.loops} loops, ${d.tool_calls ?? 0} tools, ${usd(d.cost_usd)}, ${ms(d.elapsed_ms)} (first token ${ms(d.first_token_ms)})${d.stop_reason ? ` · ${s('stop_reason')}` : ''}`
    case 'turn.failed': return `turn failed: ${d.class ?? d.error ?? ''}${d.reason ? ` · ${s('reason')}` : ''}`
    case 'turn.trace': return 'timing tree (open for spans)'
    case 'context.compiled':
      return `context ${d.decision}${d.trigger ? ` (${s('trigger')})` : ''}: ${tokens(d.est_tokens)} tokens, ${d.messages} messages (${d.strategy ?? ''}) · ${n(d.prefix_nodes ?? 0)}+${n(d.tail_nodes ?? 0)} nodes`
    case 'context.recompiled':
      return `${s('trigger')} · ${s('strategy')} · ${s('includes')} node(s)${d.strip_thinking ? ' · thinking stripped' : ''}`
    case 'action.planned':
      return `${d.tool} planned (${d.retry_class?.class ?? s('retry_class').replace(/[{}"]/g, '')}) · reserved ${d.reserved_usd != null ? usd(Number(d.reserved_usd)) : `${s('reserved')} units`}`
    case 'action.authorized': return `${short(d.correlation_id)} authorized`
    case 'action.dispatched': return `${d.tool ?? short(d.correlation_id)} dispatched${d.external_op_id ? ` · ext ${s('external_op_id')}` : ''}`
    case 'action.succeeded': return `${d.producer ?? short(d.correlation_id)} succeeded in ${ms(d.duration_ms)}`
    case 'action.failed': return `${d.producer ?? short(d.correlation_id)} failed${d.error ? `: ${String(d.error).slice(0, 80)}` : ''}`
    // Rows from before theseus-8az say `action.denied`.
    case 'action.declined': case 'action.denied': return `${s('tool')} · declined by ${s('by')} · ${s('reason')}`
    case 'action.confirmed': return `confirmed${d.by ? ` by ${d.by}` : ''}`
    case 'action.confirm_answered':
      return `${d.approved === false ? 'declined' : 'approved'}${d.by ? ` by ${d.by}` : ''}${d.via ? ` via ${s('via')}` : ''}${d.note ? `: ${String(d.note).slice(0, 80)}` : ''}`
    case 'tool.job_started': return `job pid ${d.pid}: ${(d.argv ?? []).join(' ').slice(0, 90)}`
    case 'tool.denied': return `${d.tool ?? ''} denied${d.reason ? `: ${d.reason}` : ''}`
    case 'tool.confirm_requested': return `${d.tool ?? ''} asks for approval${d.reason ? `: ${s('reason')}` : ''}`
    case 'tool.notified':
      return `notified · ${s('tool')} · ${s('summary')} · ${s('setting')}${d.granted ? ` · 🔑 ${s('granted')}` : ''}`
    case 'secret.granted': return `${d.program ? `${s('program')} got ${s('variable')}` : `${s('tool')} got`} (${s('secret')}) · ${s('correlation_id')}`
    case 'secret.withheld': return `${s('program')} got no ${s('variable')} (${s('secret')}): ${s('why')}`
    case 'job.wrapper_lost':
      return `${s('tool')} · job ${s('correlation_id')} lost its wrapper (pid ${s('pid')}, signal ${s('signal')}) before it reported · outcome unknown`
    case 'approval.refused':
      return `${d.act === 'policy.untighten' ? 'undo of ' : d.act === 'policy.tighten' ? 'should have asked for ' : ''}${s('tool')} · ${s('who')} via ${s('via')} did not count: ${s('why')}`
    case 'approval.channel_checked': return `${s('channel')} · ${d.trusted ? 'trusted' : 'not trusted'}: ${s('detail')}`
    case 'policy.tightened':
      return `${s('tool')} asks first: tightened by ${s('by')} via ${s('via')}${d.correlation_id ? ` · from ${s('correlation_id')}` : ''}${d.changed === false ? ` · the config already asks (${s('config_setting')})` : ` · the config says ${s('config_posture')}`}`
    case 'policy.untightened':
      return `${s('tool')} back to ${s('posture')} (${s('setting')}) · undone by ${s('by')} via ${s('via')} · tightened by ${s('tightened_by')}`
    case 'execution.waiting': return `execution waits (turn ${d.turn}, ${ms(d.turn_ms)})${d.wake ? ` · wake ${s('wake')}` : ''}`
    case 'execution.running': return `execution running${d.turn ? ` · turn ${s('turn')}` : ''}${d.queued_results ? ` · ${s('queued_results')} queued result(s)` : ''}`
    case 'execution.queued': return `execution queued${d.why ? ` · why ${s('why')}` : ''}`
    case 'budget.asked':
      return `at its limit: spent ${usd(Number(d.spent_usd))} of ${usd(Number(d.limit_usd))}, the call needs ${usd(Number(d.needed_usd))}`
    case 'budget.reset':
      return `spend reset to $0 by ${s('by')} · it was ${usd(Number(d.spent_before_usd))} of ${usd(Number(d.limit_usd))} · reset ${s('resets')}`
    case 'budget.migrated':
      return `unit budget read in dollars · ${s('state')} · spent ${usd(Number(d.spent_usd))} of ${usd(Number(d.limit_usd))}`
    // theseus-3pj: an open session took the config's changed spend limit.
    case 'budget.limit_changed':
      return `limit ${usd(Number(d.from_usd))} → ${usd(Number(d.to_usd))} (the config's) · spent ${usd(Number(d.spent_usd))}, ${usd(Number(d.available_usd))} left${d.proceeds ? ' · the waiting call proceeds' : ''}${d.withdrew ? ' · its question withdrawn' : ''}`
    case 'discord.confirm':
      return `Discord press: ${d.approve === false ? 'decline' : 'approve'}${d.by ? ` by ${d.by}` : ''}${d.ok === false ? ` (failed${d.error ? `: ${String(d.error).slice(0, 60)}` : ''})` : ''}`
    case 'discord.tighten': return `should have asked: ${s('tool')} by ${s('by')}${d.ok ? '' : ` · failed: ${s('error')}`}`
    case 'discord.command': return `/${s('command')} by ${s('by')}`
    case 'discord.ignored': return `${s('author')} (${s('author_id')}) · ${s('reason')}`
    case 'discord.bound': return `${s('label')} → this session`
    case 'discord.ready': return `${s('bot')} · ${s('guilds')} guild(s) · bindings ${s('revision')}`
    case 'discord.disconnected': return s('why')
    case 'discord.error': return `${s('op')}: ${s('error')}`
    case 'discord.message.out': return `→ ${d.place ?? ''} · ${d.chars ?? 0} chars${d.buttons ? ' · with Approve/Decline' : ''}${d.part ? ` · ${s('part')}` : ''}`
    case 'discord.message.in': return `← ${d.place ?? ''}${d.author ? ` · from ${s('author')}` : ''}${d.chars ? ` · ${d.chars} chars` : ''}`
    case 'startup.step':
      return `startup step ${d.step}: ${d.name}${d.requeued ? ` · requeued ${s('requeued')}` : ''}${d.drained != null ? ` · drained ${s('drained')}` : ''}`
    case 'store.restored':
      return `from ${s('from')} · ${s('records')} records · ${s('sessions')} sessions${Number(d.truncated_bytes ?? 0) > 0 ? ` · cut ${s('truncated_bytes')} torn bytes` : ''}`
    case 'reconcile':
      return `woke ${count(d.woke_due)} · unknown ${count(d.marked_unknown)} · settled ${count(d.settled_from_evidence)} · ${s('elapsed_us')} µs`
    // Rows from before theseus-hco, which removed the hook system; old stores keep them.
    case 'hook.site': return `hook ${d.event} · ${d.handlers} handlers → ${d.outcome}`
    case 'secrets.resolved': return `secrets resolved: ${(d.names ?? []).length} in ${ms(d.ms)}`
    case 'server.started': case 'server.serving': return r.kind.replace('server.', 'server ')
    // The web UI's door (theseus-70f, 3qf, zab): one row per kind a minute, with the count it stands for.
    case 'web.refused': {
      const by: Record<string, string> = { host: 'by address', origin: 'by page', peer: 'by user' }
      const k = d.count ?? 1
      return `web UI turned away ${k} connection${k === 1 ? '' : 's'} ${by[d.why] ?? d.why ?? ''}${d.last?.why ? `: ${String(d.last.why).slice(0, 90)}` : ''}`
    }
    case 'web.dev_origin': {
      const k = d.count ?? 1
      return `web UI served the dev page ${k}× (${d.last?.origin ?? '?'} via ${d.last?.host ?? '?'})`
    }
    default: {
      // The families whose rows carry their own outcome: an action's, a budget's, a completion's, an execution's.
      // (Each row of a family that has no case above reads by these, or by its first fields when it has none of them.)
      let fam = ''
      if (r.kind.startsWith('action.')) {
        fam = `${s('outcome') || s('cancel')}${d.duration_ms != null ? ` · ${s('duration_ms')} ms` : ''}${d.cost_usd != null ? ` · ${usd(Number(d.cost_usd))}` : ''}${d.usage_units != null ? ` · ${s('usage_units')} units` : ''}${d.execution_state ? ` · execution ${s('execution_state')}` : ''}`
      } else if (r.kind.startsWith('budget.')) {
        fam = `${d.units ? `${s('units')} units · ` : ''}${s('purpose')}${d.actual != null ? `actual ${s('actual')}` : ''}${d.available_after != null ? ` · ${s('available_after')} available` : ''}`
      } else if (r.kind.startsWith('completion.')) fam = `${s('producer')} · ${s('outcome')}${d.seen ? ` · seen ${s('seen')}` : ''}`
      else if (r.kind.startsWith('execution.')) fam = s('reason') || s('why') || s('by')
      if (fam.replace(/[ ·]/g, '')) return fam
      const keys = Object.entries(d).filter(([, v]) => typeof v !== 'object').slice(0, 3)
      return keys.map(([k, v]) => `${k}=${String(v).slice(0, 40)}`).join(' · ')
    }
  }
}
