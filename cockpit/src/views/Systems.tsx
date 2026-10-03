// Systems: the machine under the harness. Config and secrets (names, never values), the last start, the kernel,
// the daemon's children, the broker's grants, Discord, approval channels, context files, profiles, and the catalog.
import { useMemo, useState, type ReactNode } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { Link, useNavigate } from 'react-router'
import {
  Baby, Bot, Cpu, FileText, KeyRound, Network, Radio, Rocket, Server, Settings2, ShieldCheck, Tags,
} from 'lucide-react'
import type { CatalogList, Health, ProfileList } from '@protocol'
import { call, useRpc } from '@/lib/rpc'
import { useTick } from '@/lib/hooks'
import { ago, ms, short, stamp, tokens, uptime, us, usd } from '@/lib/format'
import { stateTone } from '@/lib/taxonomy'
import { useHistoryRows } from '@/lib/history'
import { Startup } from '@/components/instruments'
import { AwsCard, PhasesCard, PushFields, RecentRows, StoreCard } from '@/components/SystemsCards'
import { DiskSpoolCard } from '@/components/DiskSpool'
import { RpcConsole } from '@/components/RpcConsole'
import { Empty, Field, Panel, Pill, StatePill } from '@/components/ui'

export default function Systems() {
  const { data: h, dataUpdatedAt } = useRpc<Health>('health', undefined, 2000)
  const { data: pl } = useRpc<ProfileList>('profile.list', undefined, 15_000)
  const { data: cat } = useRpc<CatalogList>('catalog.list', undefined, 60_000)
  const now = useTick(1000)
  const nav = useNavigate()
  // The Discord binding's traffic and the approval channels' rows: the shared copy of the ledger, never a read of its own.
  const { rows } = useHistoryRows()
  const discordRows = useMemo(() => rows.filter((r) => r.kind.startsWith('discord.')).slice(-12).reverse(), [rows])
  const approvalRows = useMemo(() => rows.filter((r) => r.kind.startsWith('approval.')).slice(-8).reverse(), [rows])
  if (!h) return <Panel title="Systems" bodyClassName="h-64"><Empty>reading health…</Empty></Panel>
  const k = h.kernel
  const st = (k.startup ?? {}) as Record<string, any>
  return (
    <div className="columns-1 gap-3 lg:columns-2 2xl:columns-3 [&>*]:mb-3 [&>*]:break-inside-avoid">
      <Card title="Daemon" icon={<Server size={13} />}>
        <Field label="name · version" mono>{h.name} {h.version} · protocol {h.protocol}</Field>
        <Field label="up" mono>{uptime(h.uptime_secs + (now - dataUpdatedAt) / 1000)}</Field>
        <Field label="sessions · turns" mono>{h.sessions} · {h.turns}</Field>
        <Field label="model" mono>{h.provider} · {h.model} · profile {h.profile}</Field>
        <Field label="providers" mono>{h.providers.join(', ')}</Field>
        <Field label="ledger rows" mono>{h.ledger_rows.toLocaleString()}</Field>
        <Field label="provider errors" mono>{h.provider_errors}</Field>
        <Field label="spend" mono>{usd(h.cost_usd_total)}</Field>
        <Field label="catalog" mono>{h.catalog_version ?? '—'}</Field>
        <Field label="narrative" mono>{h.narrative ? 'on' : 'off'}</Field>
        <Field label="turns held · ceiling" mono>{h.kernel.turns_held} / {h.kernel.admission_ceiling}</Field>
        <Field label="tokens in · out" mono>{tokens(h.usage_total.input_tokens + h.usage_total.cache_read_input_tokens + h.usage_total.cache_creation_input_tokens)} · {tokens(h.usage_total.output_tokens)}</Field>
        <Field label="cache read · written" mono>{tokens(h.usage_total.cache_read_input_tokens)} · {tokens(h.usage_total.cache_creation_input_tokens)}</Field>
      </Card>

      <DiskSpoolCard disk={h.disk} sweep={h.spool?.last_sweep} now={now} />

      <StoreCard health={h} />

      <AwsCard aws={h.aws} now={now} />

      <Card title="Kernel" icon={<Cpu size={13} />}>
        <Field label="accepting">{k.accepting ? <Pill tone="ok">accepting</Pill> : <Pill tone="wait">held</Pill>}</Field>
        <Field label="admission ceiling · turns held" mono>{k.admission_ceiling} · {k.turns_held}</Field>
        <Field label="executions">{Object.entries(k.executions_by_state).map(([s, n]) => <Pill key={s} tone={stateTone(s)} className="ml-1">{s} {n}</Pill>)}</Field>
        <Field label="actions">{Object.entries(k.actions_by_state).map(([s, n]) => <Pill key={s} tone={stateTone(s)} className="ml-1">{s} {n}</Pill>)}</Field>
        <Field label="quarantined completions" mono><span className={k.quarantined_completions ? 'text-wait' : ''} title="results that matched no action; never inferred into anything">{k.quarantined_completions}</span></Field>
        <Field label="lingering wrappers" mono>{k.lingering_wrappers ?? 0}</Field>
        <Field label="spend limit per session" mono>{usd(k.spend_limit_usd)}</Field>
        <PushFields push={h.push} />
        {Array.isArray(st.steps) && (
          <div className="mt-2">
            <div className="panel-title mb-1">kernel startup · {us(st.elapsed_us)}</div>
            {st.steps.map((s: any) => <Field key={s.step} label={`${s.step}. ${s.name}`} mono>{us(s.elapsed_us)}</Field>)}
            {st.reconcile && <Field label="reconcile" mono>{us(st.reconcile.elapsed_us)} · {st.reconcile.open_executions} open executions · {st.reconcile.open_actions} open actions</Field>}
          </div>
        )}
      </Card>

      <Card title="Last start" icon={<Rocket size={13} />} bodyClassName="h-[230px] p-2">
        <Startup phases={h.startup ?? []} />
      </Card>

      <PhasesCard phases={h.startup ?? []} />

      <Card title="Config" icon={<Settings2 size={13} />}>
        <Field label="source">{h.config?.source ?? '—'}</Field>
        <Field label="reference" mono>{h.config?.reference ?? '—'}</Field>
        <Field label="state">{h.config ? <StatePill state={h.config.state} /> : '—'}</Field>
        <Field label="started from" mono>{h.config?.started_from ?? '—'}</Field>
        {h.config?.confirmed_ms !== undefined && h.config?.confirmed_ms !== null && <Field label="confirmed" mono>{ms(h.config.confirmed_ms)} after start</Field>}
        {h.config?.detail && <Field label="detail">{h.config.detail}</Field>}
        {h.config?.state === 'held' && h.config.retry_in_ms != null && <Field label="read again in" mono>{Math.ceil(h.config.retry_in_ms / 1000)} s</Field>}
        {h.config?.restarted && <Field label="restarted onto" mono>{new Date(h.config.restarted.at_unix_ms).toLocaleTimeString()} · {h.config.restarted.tables.join(', ')}</Field>}
        {h.config && h.config.state !== 'confirmed' && h.config.source === 'vault' && <div className="mt-1 text-[11.5px] text-wait">Nothing acts until the vault confirms the copy: reads answer, and every method that acts waits.</div>}
        {h.config?.restarted && <div className="mt-1 text-[11.5px] text-wait">Restarted onto the vault&rsquo;s note, which had changed since the copy.</div>}
      </Card>

      <Card title="Secrets · names only" icon={<KeyRound size={13} />}>
        <Field label="state">{h.secrets ? <StatePill state={h.secrets.state} /> : '—'}</Field>
        <Field label="method · rounds" mono>{h.secrets?.method ?? '—'} · {h.secrets?.rounds ?? 0}</Field>
        {h.secrets?.settled_ms !== undefined && h.secrets?.settled_ms !== null && <Field label="settled" mono>{ms(h.secrets.settled_ms)} after start</Field>}
        <div className="mt-1.5 flex flex-wrap gap-1">
          {h.secrets?.ready.map((n) => <Pill key={n} tone="ok">{n}</Pill>)}
          {h.secrets?.resolving.map((n) => <Pill key={n} tone="wait">{n}</Pill>)}
          {h.secrets?.failed.map((f) => <Pill key={f.name} tone="fault">{f.name}</Pill>)}
        </div>
        {(h.secrets?.failed ?? []).map((f) => <div key={f.name} className="mt-1.5 text-[11.5px] text-fault">{f.name} did not resolve: {f.error}. Whatever needs it waits, and never runs without it.</div>)}
        {h.secrets?.retry_in_ms != null && (h.secrets?.failed.length ?? 0) > 0 && <div className="mt-1 text-[11px] text-ink-faint">fetched again in {Math.ceil(h.secrets.retry_in_ms / 1000)} s</div>}
      </Card>

      <Card title="Children" icon={<Baby size={13} />}>
        {h.children ? <>
          <Field label="subreaper">{h.children.subreaper ? <Pill tone="ok">yes</Pill> : <Pill tone="wait">no</Pill>}</Field>
          <Field label="wrappers running · lingering" mono>{h.children.wrappers_running} · {h.children.wrappers_lingering}</Field>
          <Field label="orphans · owned" mono>{h.children.orphans} · {h.children.owned}</Field>
          <Field label="zombies" mono><span className={h.children.zombies ? 'text-fault' : ''}>{h.children.zombies}</span></Field>
          <Field label="reaped wrappers · orphans" mono>{h.children.reaped_wrappers} · {h.children.reaped_orphans}</Field>
        </> : <Empty>not reported</Empty>}
      </Card>

      <Card title="Broker grants" icon={<ShieldCheck size={13} />}>
        {h.broker?.length ? (
          <table className="w-full text-[12px]">
            <tbody>
              {h.broker.map((g, i) => (
                <tr key={i} className="border-b border-line/50">
                  <td className="num py-1 text-tool">{g.to}</td>
                  <td className="py-1 text-ink-faint">{g.kind}{g.variable ? ` · ${g.variable}` : ''}</td>
                  <td className="num py-1 text-ink-dim">{g.secret}</td>
                  <td className="py-1"><Pill tone={g.posture === 'confirm' ? 'wait' : 'live'}>{g.posture}</Pill></td>
                  <td className="num py-1 text-right text-ink-faint">{g.uses} uses</td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : <Empty>no grants</Empty>}
      </Card>

      {(h.bindings ?? []).map((b) => (
        <Card key={b.kind} title={`${b.kind} binding`} icon={<Radio size={13} />}>
          <Field label="state"><StatePill state={b.state} /></Field>
          {b.detail && <Field label="detail">{b.detail}</Field>}
          {b.bot_user && <Field label="bot · guild" mono>{b.bot_user} · {b.guild_id ?? '—'}</Field>}
          {b.bindings_file && <Field label="bindings" mono>{b.bindings_file}{b.revision ? ` · revision ${b.revision}` : ''}</Field>}
          {b.members_intent != null && (
            <Field label="Server Members intent"><Pill tone={b.members_intent ? 'ok' : 'wait'}>{b.members_intent ? 'on' : 'off'}</Pill>
              <span className="ml-1 text-[11px] text-ink-faint">{b.members_intent ? 'who can view a guild channel is checked for approvals' : 'a guild channel cannot be verified for approvals, so none is trusted'}</span></Field>
          )}
          {b.connected_at_ms > 0 && <Field label="connected" mono>{ago(b.connected_at_ms, now)}{b.latency_ms ? ` · ${b.latency_ms} ms` : ''}</Field>}
          <Field label="in · out · edits" mono>{b.messages_in} · {b.messages_out} · {b.edits}</Field>
          <Field label="interactions · ignored · errors" mono>{b.interactions} · {b.ignored} · <span className={b.errors ? 'text-fault' : ''}>{b.errors}</span></Field>
          {b.last_error && <Field label="last error">{b.last_error}</Field>}
          {b.outbox && <Field label="outbox" mono>{b.outbox.pending} pending · {b.outbox.sent} sent · {b.outbox.failed} refused{b.outbox.oldest_pending_ms ? ` · oldest ${ago(b.outbox.oldest_pending_ms, now)}` : ''}</Field>}
          {b.outbox?.last_error && <Field label="outbox last error"><span className="text-wait">{b.outbox.last_error_ms ? `${ago(b.outbox.last_error_ms, now)}: ` : ''}{b.outbox.last_error}</span></Field>}
          {b.places.length > 0 && (
            <div className="mt-2">
              <div className="panel-title mb-1">places</div>
              {b.places.map((p) => (
                <div key={p.label} className="border-b border-line/40 py-1 text-[12px] last:border-0">
                  <div className="flex items-baseline gap-2">
                    <span className="num text-ink">{p.label}</span><span className="text-ink-faint">{p.kind}</span>
                    {p.mention_only && <span className="text-[11px] text-ink-faint" title="only messages that @mention Theseus or reply to it start a turn">@mention only</span>}
                    <span className="num ml-auto text-[11px] text-ink-faint">{p.last_activity_ms ? ago(p.last_activity_ms, now) : '—'}</span>
                  </div>
                  <div className="num flex flex-wrap gap-x-3 text-[10.5px] text-ink-faint">
                    <span>{p.channel_id ? `channel ${p.channel_id}` : 'opens on first DM'}</span>
                    {p.session_id && <button onClick={() => nav(`/session/${p.session_id}`)} className="text-live hover:underline" title="open this place's session">session {short(p.session_id)}</button>}
                    <span title="who may drive it">{p.users.join(', ')}</span>
                  </div>
                </div>
              ))}
            </div>
          )}
          <div className="panel-title mb-0.5 mt-2">recent traffic</div>
          <RecentRows rows={discordRows} empty="nothing since the daemon started" toneOf={(r) => r.kind === 'discord.error' ? 'fault' : r.kind === 'discord.ignored' ? 'wait' : 'idle'} />
        </Card>
      ))}

      <Card title="Approval" icon={<ShieldCheck size={13} />}>
        {h.approval?.configured ? <>
          <Field label="trusted users" mono>{h.approval.trusted_users.join(', ') || '—'}</Field>
          {h.approval.channels.map((c) => <Field key={c.channel} label={c.channel}><StatePill state={c.state} /> <span className="text-[11px] text-ink-faint">{c.detail}</span></Field>)}
        </> : <div className="text-[12px] text-ink-dim">No <span className="num">[approval]</span> rule: the CLI, the web UI, and each place's listed Discord users answer.</div>}
        <div className="panel-title mb-0.5 mt-2">recent approval rows</div>
        <RecentRows rows={approvalRows} empty="no approval rows yet" toneOf={(r) => r.kind === 'approval.refused' ? 'wait' : 'idle'} />
        {(h.tightenings ?? []).length > 0 && (
          <div className="mt-2">
            <div className="panel-title mb-1">tightened at run time</div>
            {h.tightenings!.map((t) => <Field key={t.tool} label={t.tool} mono>{t.posture} · by {t.by} · {stamp(t.at_ms)}</Field>)}
          </div>
        )}
      </Card>

      <Card title="Context files" icon={<FileText size={13} />}>
        <Field label="persona" mono>{h.context?.persona ?? '—'}</Field>
        <Field label="personas" mono>{h.context?.personas.join(', ') || '—'}</Field>
        {(h.context?.system_files ?? []).map((f) => <Field key={f} label="system" mono>{f}</Field>)}
        {(h.context?.persona_files ?? []).map((f) => <Field key={f} label="persona" mono>{f}</Field>)}
        {!h.context?.system_files.length && !h.context?.persona_files.length && <div className="text-[12px] text-ink-faint">no context files configured</div>}
      </Card>

      <Card title="Profiles" icon={<Bot size={13} />}>
        {(pl?.profiles ?? []).map((p) => (
          <div key={p.name} className="group flex items-center gap-2 border-b border-line/50 py-1 text-[12px]">
            <span className="num w-20 text-ink">{p.name}</span>
            {p.live && <Pill tone="live">live</Pill>}
            <span className="num text-ink-dim">{p.provider} · {p.model}</span>
            <span className="num ml-auto text-ink-faint">{tokens(p.max_output_tokens)} out{p.has_system ? ' · system' : ''}</span>
            {!p.live && <MakeLive name={p.name} model={`${p.provider} · ${p.model}`} />}
          </div>
        ))}
        {pl && <div className="num mt-1 text-[11px] text-ink-faint">live from {pl.live_source}</div>}
      </Card>

      <Card title={`Catalog · ${cat?.version ?? '…'}`} icon={<Tags size={13} />}>
        <table className="w-full whitespace-nowrap text-[11.5px]">
          <thead className="text-[10px] uppercase tracking-wider text-ink-faint">
            <tr><th className="py-1 text-left">model</th><th className="pl-2 text-left">provider</th><th className="pl-2 text-right">window</th><th className="pl-2 text-right">max out</th><th className="pl-2 text-right">in</th><th className="pl-2 text-right">out</th><th className="pl-2 text-right" title="cache read / write (5 min) / write (1 hour)">cache r/w/1h</th><th className="pl-2 text-left">thinking</th></tr>
          </thead>
          <tbody>
            {(cat?.models ?? []).map((m) => {
              const e = m.entry as Record<string, any>
              return (
                <tr key={m.model} className="border-t border-line/50" title={`source: ${String(e.source ?? '')}${e.refusal_fallbacks ? '\nserver-side refusal fallbacks' : ''}\ncache minimum ${String(e.cache_min_tokens ?? '')} tokens${e.vision ? '\nvision' : ''}`}>
                  <td className="num py-1 text-model">{m.model}{m.profiles.length ? <div className="text-[10px] leading-tight text-ink-faint">{m.profiles.join(', ')}</div> : null}</td>
                  <td className="pl-2 text-ink-faint">{String(e.provider ?? '')}</td>
                  <td className="num pl-2 text-right text-ink-dim">{tokens(e.context_window)}</td>
                  <td className="num pl-2 text-right text-ink-dim">{tokens(e.max_output_tokens)}</td>
                  <td className="num pl-2 text-right text-ink-dim">${e.input_per_mtok}</td>
                  <td className="num pl-2 text-right text-ink-dim">${e.output_per_mtok}</td>
                  <td className="num pl-2 text-right text-ink-faint">${e.cache_read_per_mtok} / ${e.cache_write_per_mtok}{e.cache_write_1h_per_mtok != null && <> / ${e.cache_write_1h_per_mtok}</>}</td>
                  <td className="pl-2 text-ink-faint">{String(e.thinking ?? '')}{e.effort ? ' · effort' : ''}</td>
                </tr>
              )
            })}
          </tbody>
        </table>
        <div className="mt-1 text-[10.5px] text-ink-faint">US dollars per million tokens</div>
      </Card>

      <RpcConsole />

      <WebAccess web={h.web} />
    </div>
  )
}

/** profile.use: the live profile, which new turns run on unless one names its own. Confirmed first, since every
 *  session's next turn changes model. */
function MakeLive({ name, model }: { name: string; model: string }) {
  const qc = useQueryClient()
  const [busy, setBusy] = useState(false)
  const use = async () => {
    if (!window.confirm(`Make "${name}" (${model}) the live profile? New turns run on it unless they name their own.`)) return
    setBusy(true)
    try { await call('profile.use', { name }); await qc.invalidateQueries() } catch (e: any) { window.alert(e?.message ?? String(e)) } finally { setBusy(false) }
  }
  return (
    <button onClick={use} disabled={busy} className="rounded px-1.5 py-0.5 text-[10.5px] text-live opacity-0 ring-1 ring-live/30 hover:bg-live/10 group-hover:opacity-100 disabled:opacity-50">
      {busy ? '…' : 'make live'}
    </button>
  )
}

/** The web UI's own door: who it turned away since the daemon started, and whether a dev page is let in. */
function WebAccess({ web }: { web: Health['web'] }) {
  const refused = (web?.refused_host ?? 0) + (web?.refused_origin ?? 0) + (web?.refused_peer ?? 0)
  const count = (n: number | undefined) => <span className={n ? 'text-wait' : 'text-ink-dim'}>{n ?? 0}</span>
  return (
    <Card title="Web UI · access" icon={<Network size={13} />}>
      {web ? <>
        <Field label="another address (Host)" mono>{count(web.refused_host)}</Field>
        <Field label="another page (Origin)" mono>{count(web.refused_origin)}</Field>
        <Field label="another user (socket owner)" mono>{count(web.refused_peer)}</Field>
        <Field label="owner check">
          {web.refused_peer === undefined
            ? <Pill tone="wait" title="this daemon predates the owner check (theseus-3qf)">not in this build · any local user is served</Pill>
            : web.peer_unchecked
              ? <Pill tone="fault" title={web.peer_unchecked}>off · any local user is served</Pill>
              : <Pill tone="ok">on · this user only</Pill>}
        </Field>
        <Field label="dev origin">
          {web.dev_origin
            ? <><Pill tone="wait">open</Pill> <span className="num text-[11px] text-ink-dim">{web.dev_origin} · {web.dev_origin_served ?? 0} served</span></>
            : <Pill tone="ok">closed</Pill>}
        </Field>
        <div className="mt-1.5 flex gap-3 text-[11px]">
          <Link to="/ledger?kind=web.refused" className="text-live hover:underline">refusals in the ledger →</Link>
          {web.dev_origin && <Link to="/ledger?kind=web.dev_origin" className="text-live hover:underline">dev page's uses →</Link>}
        </div>
        <div className="mt-1.5 text-[11px] text-ink-faint">
          {refused ? `${refused} turned away since the daemon started; each kind is ledgered at most once a minute.` : 'Nothing turned away since the daemon started.'}{' '}
          {web.dev_origin && 'Unset [web] dev_origin when you are done developing: the dev server checks no one.'}
        </div>
      </> : <Empty>this daemon doesn't report its web UI's refusals</Empty>}
      <div className="mt-2 border-t border-line/50 pt-1.5 text-[11px] text-ink-faint">This page talks to the daemon over one WebSocket (<span className="num">/ws</span>), JSON-RPC 2.0, the same protocol as the CLI and the classic Observatory.</div>
    </Card>
  )
}

function Card({ title, icon, children, bodyClassName }: { title: string; icon: ReactNode; children: ReactNode; bodyClassName?: string }) {
  return <Panel title={title} icon={icon} bodyClassName={bodyClassName ?? 'px-3.5 py-2.5'}>{children}</Panel>
}
