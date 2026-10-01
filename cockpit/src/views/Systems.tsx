// Systems: the machine under the harness. Config and secrets (names, never values), the last start, the kernel,
// the daemon's children, the broker's grants, Discord, approval channels, context files, profiles, and the catalog.
import type { ReactNode } from 'react'
import { Link } from 'react-router'
import {
  Baby, Bot, Cpu, FileText, KeyRound, Network, Radio, Rocket, Server, Settings2, ShieldCheck, Tags,
} from 'lucide-react'
import type { CatalogList, Health, ProfileList } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { useTick } from '@/lib/hooks'
import { ago, ms, stamp, tokens, uptime, us, usd } from '@/lib/format'
import { stateTone } from '@/lib/taxonomy'
import { Startup } from '@/components/instruments'
import { RpcConsole } from '@/components/RpcConsole'
import { Empty, Field, Panel, Pill, StatePill } from '@/components/ui'

export default function Systems() {
  const { data: h, dataUpdatedAt } = useRpc<Health>('health', undefined, 2000)
  const { data: pl } = useRpc<ProfileList>('profile.list', undefined, 15_000)
  const { data: cat } = useRpc<CatalogList>('catalog.list', undefined, 60_000)
  const now = useTick(1000)
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
      </Card>

      <Card title="Kernel" icon={<Cpu size={13} />}>
        <Field label="accepting">{k.accepting ? <Pill tone="ok">accepting</Pill> : <Pill tone="wait">held</Pill>}</Field>
        <Field label="admission ceiling · turns held" mono>{k.admission_ceiling} · {k.turns_held}</Field>
        <Field label="executions">{Object.entries(k.executions_by_state).map(([s, n]) => <Pill key={s} tone={stateTone(s)} className="ml-1">{s} {n}</Pill>)}</Field>
        <Field label="actions">{Object.entries(k.actions_by_state).map(([s, n]) => <Pill key={s} tone={stateTone(s)} className="ml-1">{s} {n}</Pill>)}</Field>
        <Field label="quarantined completions" mono>{k.quarantined_completions}</Field>
        <Field label="lingering wrappers" mono>{k.lingering_wrappers ?? 0}</Field>
        <Field label="spend limit per session" mono>{usd(k.spend_limit_usd)}</Field>
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

      <Card title="Config" icon={<Settings2 size={13} />}>
        <Field label="source">{h.config?.source ?? '—'}</Field>
        <Field label="reference" mono>{h.config?.reference ?? '—'}</Field>
        <Field label="state">{h.config ? <StatePill state={h.config.state} /> : '—'}</Field>
        <Field label="started from" mono>{h.config?.started_from ?? '—'}</Field>
        {h.config?.confirmed_ms !== undefined && h.config?.confirmed_ms !== null && <Field label="confirmed" mono>{ms(h.config.confirmed_ms)} after start</Field>}
        {h.config?.detail && <Field label="detail">{h.config.detail}</Field>}
        {h.config?.restarted && <Field label="restarted onto" mono>{h.config.restarted.tables.join(', ')}</Field>}
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
          {b.connected_at_ms > 0 && <Field label="connected" mono>{ago(b.connected_at_ms, now)}{b.latency_ms ? ` · ${b.latency_ms} ms` : ''}</Field>}
          <Field label="in · out · edits" mono>{b.messages_in} · {b.messages_out} · {b.edits}</Field>
          <Field label="interactions · ignored · errors" mono>{b.interactions} · {b.ignored} · <span className={b.errors ? 'text-fault' : ''}>{b.errors}</span></Field>
          {b.last_error && <Field label="last error">{b.last_error}</Field>}
          {b.outbox && <Field label="outbox" mono>{b.outbox.pending} pending · {b.outbox.sent} sent · {b.outbox.failed} failed{b.outbox.oldest_pending_ms ? ` · oldest ${ago(b.outbox.oldest_pending_ms, now)}` : ''}</Field>}
          {b.places.length > 0 && (
            <div className="mt-2">
              <div className="panel-title mb-1">places</div>
              {b.places.map((p) => <Field key={p.label} label={`${p.kind} ${p.label}`} mono>{p.users.length} users{p.mention_only ? ' · mention only' : ''} · {ago(p.last_activity_ms, now)}</Field>)}
            </div>
          )}
        </Card>
      ))}

      <Card title="Approval" icon={<ShieldCheck size={13} />}>
        {h.approval?.configured ? <>
          <Field label="trusted users" mono>{h.approval.trusted_users.join(', ') || '—'}</Field>
          {h.approval.channels.map((c) => <Field key={c.channel} label={c.channel}><StatePill state={c.state} /> <span className="text-[11px] text-ink-faint">{c.detail}</span></Field>)}
        </> : <div className="text-[12px] text-ink-dim">No <span className="num">[approval]</span> rule: the CLI, the web UI, and each place's listed Discord users answer.</div>}
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
          <div key={p.name} className="flex items-center gap-2 border-b border-line/50 py-1 text-[12px]">
            <span className="num w-20 text-ink">{p.name}</span>
            {p.live && <Pill tone="live">live</Pill>}
            <span className="num text-ink-dim">{p.provider} · {p.model}</span>
            <span className="num ml-auto text-ink-faint">{tokens(p.max_output_tokens)} out{p.has_system ? ' · system' : ''}</span>
          </div>
        ))}
        {pl && <div className="num mt-1 text-[11px] text-ink-faint">live from {pl.live_source}</div>}
      </Card>

      <Card title={`Catalog · ${cat?.version ?? '…'}`} icon={<Tags size={13} />}>
        <table className="w-full whitespace-nowrap text-[11.5px]">
          <thead className="text-[10px] uppercase tracking-wider text-ink-faint">
            <tr><th className="py-1 text-left">model</th><th className="text-right">window</th><th className="text-right">in</th><th className="text-right">out</th><th className="text-right">cache r/w</th></tr>
          </thead>
          <tbody>
            {(cat?.models ?? []).map((m) => {
              const e = m.entry as Record<string, any>
              return (
                <tr key={m.model} className="border-t border-line/50">
                  <td className="num py-1 text-model">{m.model}{m.profiles.length ? <span className="text-ink-faint"> · {m.profiles.join(', ')}</span> : null}</td>
                  <td className="num text-right text-ink-dim">{tokens(e.context_window)}</td>
                  <td className="num text-right text-ink-dim">${e.input_per_mtok}</td>
                  <td className="num text-right text-ink-dim">${e.output_per_mtok}</td>
                  <td className="num text-right text-ink-faint">${e.cache_read_per_mtok} / ${e.cache_write_per_mtok}</td>
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
