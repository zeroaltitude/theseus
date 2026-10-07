// Since you last looked (theseus-hnof.3; the owner's answer C2, 2026-10-06): the watch's sixth plate. Back at the Ship
// after a while away, it says what happened meanwhile: the sessions and tasks that started or finished, what went
// wrong, the questions that came and went, and what it cost. Its show lights all of it on the chart (or one part, from
// its tally), its link opens the ledger from that moment, and its replay runs the time machine over the stretch.
//
// The stretch it tells is the time you were away: from the end of your last look at the Ship to the start of this one,
// kept in this browser (`lookAt`, `looked`); the daemon keeps nothing of it. What happens while you look, the other five
// plates and the Ship show live. The pure half: `Watch.tsx` keeps the times and draws the plate. A row after the moment
// is never read, so under the time machine (and in a replay, which moves the moment) the plate tells the stretch up to
// the moment.
import type { ActionInfo, ConfirmRequest, LedgerEntry } from '@protocol'
import type { ShipModel } from './model.ts'
import {
  andList, count, dollars, failureLights, failureLine, Failures, isModelFailure, LINES, lightsOfTurns, lookOf, oneLine, setOf, shortPaths,
  SKEW_MS, span, type Plate, type Tone, type ToolOf, type WatchLine, type WatchSet,
} from './watch.ts'

/** A look ends when the Ship has been out of sight this long: back after it, the plate tells what happened since. */
export const LOOK_GAP_MS = 5 * 60_000
/** While the Ship is in sight, the time is written this often, so a browser closed without warning keeps it. */
export const SEEN_EVERY_MS = 30_000
/** Where this browser keeps it. */
export const LOOKED_KEY = 'cockpit.watch.looked'

/** What this browser keeps: the stretch you were away, from `since` (your last look's end) to `start` (this look's
 *  start), and when the Ship was last in sight. */
export interface Looked { since: number; start: number; seen: number }

/** The Ship is in sight at `now`. If it was last in sight more than `LOOK_GAP_MS` ago, that look is over: a new one
 *  starts, and the plate tells the stretch between them. Within the gap the look goes on, and so does the stretch it
 *  tells. A browser's first look tells nothing yet, and so does a time kept in the future (a clock set back). */
export function lookAt(kept: Looked | null, now: number): Looked {
  if (!kept || kept.seen > now || kept.start > now) return { since: now, start: now, seen: now }
  if (now - kept.seen > LOOK_GAP_MS) return { since: kept.seen, start: now, seen: now }
  return { since: kept.since, start: kept.start, seen: now }
}

/** What a browser kept, read: anything else reads as nothing kept. */
export function looked(raw: string | null): Looked | null {
  try {
    const v = JSON.parse(raw ?? 'null') as Partial<Looked> | null
    const ok = (x: unknown): x is number => typeof x === 'number' && Number.isFinite(x) && x > 0
    if (!v || !ok(v.since) || !ok(v.seen)) return null
    const start = ok(v.start) ? v.start : v.since
    return start < v.since || v.seen < start ? null : { since: v.since, start, seen: v.seen }
  } catch {
    return null
  }
}

/** The parts of the stretch, each a cell of the plate's tally that can light just its own. */
export type SincePart = 'sessions' | 'tasks' | 'wrong' | 'questions' | 'spent'

export interface Tally {
  part: SincePart
  /** The figure ("3", "$0.42"), its word ("sessions"), and a quieter line under them ("2 new"). */
  value: string
  word: string
  sub: string
  /** All of it in words, for the tooltip. */
  title: string
  tone: Tone
  /** What it lights alone. */
  focus: WatchSet
}

export interface Since extends Plate {
  /** The stretch you were away: from the end of your last look to the start of this one. */
  since: number
  until: number
  /** How long you were away; 0 when you have looked all along. */
  away: number
  /** Nothing happened in the stretch (or it has not begun). */
  quiet: boolean
  sessions: { started: number; worked: number; turns: number }
  tasks: { started: number; ended: number; failed: number }
  failures: number
  /** Questions asked in the stretch, answered or expired in it, both (you never saw them), and still waiting. */
  questions: { came: number; went: number; missed: number; waiting: number }
  usd: number
  calls: number
  tally: Tally[]
}

export interface SinceInput {
  model: ShipModel | null
  /** The calls, for a failure's tool when no row says it. */
  actions?: readonly ActionInfo[]
  /** The questions waiting at the moment. */
  confirms?: readonly ConfirmRequest[]
  /** The ledger history's rows, oldest first, and whether its walk has reached the ledger's end. */
  rows: readonly LedgerEntry[]
  rowsReady: boolean
  /** The stretch you were away: the end of your last look, and the start of this one. */
  since: number
  until: number
  /** The moment: now, or the time machine's. The stretch is read up to it. */
  now: number
}

type D = Record<string, unknown>
const str = (v: unknown): string | undefined => (typeof v === 'string' && v ? v : undefined)

interface Asked { at: number; tool: string; session: string | null; what: string; budget: boolean }
interface Gone { at: number; how: 'approved' | 'declined' | 'expired'; by?: string; via?: string }
interface TaskEnd { at: number; exec: string; state: string; reason?: string; parent: string | null }

/** A question's end, the most telling first: an expiry (whose frame declines the call too) says how long it waited;
 *  an answer says who; a decline alone, less. */
const GONE_RANK: Record<Gone['how'], number> = { expired: 0, approved: 1, declined: 2 }

export function sinceOf(input: SinceInput): Since {
  const { model, since, until, rows, now } = input
  // The stretch is read to its end, or to the moment when that is earlier (the time machine, a replay); its lines say
  // how long ago from the moment.
  const end = Math.min(until, now)
  const look = lookOf(model)
  const failures = new Failures()
  const opened = new Map<string, { at: number; kind: string; exec?: string }>()
  const worked = new Map<string, number>()
  const ended: TaskEnd[] = []
  const asked = new Map<string, Asked>()
  const gone = new Map<string, Gone>()
  const tools = new Map<string, string>()
  const spentBy = new Map<string, number>()
  let usd = 0
  let calls = 0
  for (let i = rows.length - 1; i >= 0; i--) {
    const r = rows[i]
    const at = r.at_unix_ms
    if (at < since - SKEW_MS) break
    if (at > end || at <= since) continue
    const d = (r.data ?? {}) as D
    const sid = r.session_id
    const cid = str(d.correlation_id)
    failures.add(r)
    switch (r.kind) {
      case 'execution.opened':
        if (sid) opened.set(sid, { at, kind: str(d.kind) ?? 'conversation', exec: str(d.execution_id) })
        break
      case 'turn.ended':
      case 'turn.failed':
        if (sid) worked.set(sid, (worked.get(sid) ?? 0) + 1)
        break
      case 'task.ended': {
        const exec = str(d.task)
        if (exec) ended.push({ at, exec, state: str(d.state) ?? 'ended', reason: str(d.reason), parent: sid })
        break
      }
      case 'tool.confirm_requested':
        if (cid) {
          const tool = str(d.tool) ?? 'a call'
          const reason = oneLine(str(d.reason) ?? '')
          asked.set(cid, { at, tool, session: str(d.session_id) ?? sid, what: shortPaths(reason.split(`: ${tool} `)[0] || str(d.resource) || tool), budget: false })
        }
        break
      case 'budget.asked':
        if (cid) asked.set(cid, { at, tool: 'budget', session: sid, what: `needs ${dollars(Number(d.needed_usd ?? 0))} · limit ${dollars(Number(d.limit_usd ?? 0))}`, budget: true })
        break
      case 'action.confirm_answered':
      case 'action.declined':
      case 'action.expired': {
        if (!cid) break
        const how: Gone['how'] = r.kind === 'action.expired' ? 'expired' : r.kind === 'action.declined' || d.approved === false ? 'declined' : 'approved'
        const g: Gone = { at, how, by: str(d.by), via: str(d.via) }
        const was = gone.get(cid)
        if (!was || GONE_RANK[how] < GONE_RANK[was.how] || (!was.by && g.by && how === was.how)) gone.set(cid, was ? { ...g, at: Math.max(at, was.at) } : g)
        break
      }
      case 'action.planned': {
        const tool = str(d.tool)
        if (cid && tool) tools.set(cid, tool)
        break
      }
      case 'provider.call': {
        const cost = Number(d.cost_usd ?? 0) || 0
        usd += cost
        calls++
        if (sid) spentBy.set(sid, (spentBy.get(sid) ?? 0) + cost)
        break
      }
    }
  }

  const toolOf: ToolOf = (cid) => tools.get(cid) ?? input.actions?.find((a) => a.correlation_id === cid)?.tool ?? look.call.get(cid)?.tool
  const failed = failures.list(isModelFailure(toolOf))
  const turnLights = lightsOfTurns(model, new Set(failed.map((f) => (f.kind === 'turn' ? f.turn : undefined)).filter((t): t is string => !!t)))
  // A task's session: its execution opened in the stretch, or its vessel.
  const sessionOfExec = (exec: string) => [...opened].find(([, o]) => o.exec === exec)?.[0] ?? model?.vessels.find((v) => v.executionId === exec)?.id
  const started = [...opened].filter(([, o]) => o.kind !== 'task')
  const tasksStarted = [...opened].filter(([, o]) => o.kind === 'task')
  const tasksFailed = ended.filter((t) => t.state !== 'complete' && t.state !== 'succeeded')
  const waitingNow = new Set((input.confirms ?? []).map((c) => c.correlation_id))
  const missed = [...asked].filter(([cid]) => gone.has(cid))
  const waiting = [...asked].filter(([cid]) => waitingNow.has(cid)).length
  const turns = [...worked.values()].reduce((a, b) => a + b, 0)
  const away = Math.max(0, until - since)

  // The lines: a failure, a question you never saw, a task that ended and what started, the newest of each first, in that
  // order; then the next of each. The other plates show the rest live.
  const failLines: WatchLine[] = failed.map((f) => failureLine(f, look, now, turnLights, toolOf))
  const missedLines: WatchLine[] = []
  const endedLines: WatchLine[] = []
  const openedLines: WatchLine[] = []
  for (const [cid, q] of missed.sort((a, b) => b[1].at - a[1].at)) {
    const g = gone.get(cid)!
    const who = look.title(q.session)
    const fate = g.how === 'expired' ? `expired after ${span(g.at - q.at)}` : `${g.how}${g.by ? ` by ${g.by}` : ''}${g.via ? ` (${g.via})` : ''}`
    missedLines.push({
      id: `missed:${cid}`, tag: q.tool, text: q.what, detail: `${fate} · ${who}`, figure: span(now - q.at), tone: g.how === 'expired' ? 'fault' : 'wait',
      flag: g.how === 'expired' ? 'expired' : g.how === 'declined' ? 'declined' : undefined,
      title: `While you were away, ${q.budget ? 'a budget question' : `${q.tool} asked`} in "${who}" (${q.what}) and was ${fate}, ${span(g.at - q.at)} later.`,
      to: { session: q.session ?? undefined, node: look.call.get(cid)?.id },
    })
  }
  for (const t of [...ended].sort((a, b) => b.at - a.at)) {
    const sid = sessionOfExec(t.exec)
    const v = look.vessel(sid)
    const ok = t.state === 'complete' || t.state === 'succeeded'
    const what = v?.title ?? `task ${t.exec.slice(-6)}`
    endedLines.push({
      id: `taskend:${t.exec}`, tag: 'task', text: what, detail: ok ? 'reported back' : `${t.state}${t.reason ? `: ${oneLine(t.reason)}` : ''}`,
      figure: span(now - t.at), tone: ok ? 'ok' : 'fault',
      title: `The task "${what}" ${ok ? 'finished and reported back' : `ended ${t.state}${t.reason ? `: ${oneLine(t.reason)}` : ''}`}, ${span(now - t.at)} ago.`,
      to: { session: sid ?? t.parent ?? undefined },
    })
  }
  for (const [sid, o] of [...opened].sort((a, b) => b[1].at - a[1].at)) {
    const task = o.kind === 'task'
    const v = look.vessel(sid)
    const n = worked.get(sid) ?? 0
    openedLines.push({
      id: `opened:${sid}`, tag: task ? 'task' : 'session', text: v?.title ?? look.title(sid), detail: `started · ${count(n, 'turn', 'turns')} since`,
      figure: span(now - o.at), tone: 'live',
      title: `${task ? 'A task' : 'A session'} started ${span(now - o.at)} ago: "${v?.title ?? look.title(sid)}", with ${count(n, 'turn', 'turns')} since.`,
      to: { session: sid },
    })
  }

  const groups = [failLines, missedLines, endedLines, openedLines]
  const lines: WatchLine[] = []
  for (let i = 0; groups.some((g) => i < g.length); i++) for (const g of groups) if (i < g.length) lines.push(g[i])

  const parts = [
    started.length ? count(started.length, 'session started', 'sessions started') : '',
    tasksStarted.length ? count(tasksStarted.length, 'task started', 'tasks started') : '',
    ended.length ? count(ended.length, 'task finished', 'tasks finished') : '',
    failed.length ? count(failed.length, 'failure', 'failures') : '',
    missed.length ? count(missed.length, 'question came and went', 'questions came and went') : '',
    usd > 0 ? `${dollars(usd)} spent` : '',
  ].filter(Boolean)
  const quiet = !parts.length && !worked.size && !asked.size && !gone.size
  const ready = input.rowsReady

  // Each part's lights: the sessions it names, and its calls.
  const taskIds = [...tasksStarted.map(([sid]) => sid), ...ended.map((t) => sessionOfExec(t.exec))]
  const focus = {
    sessions: setOf([...started.map(([sid]) => sid), ...worked.keys()], []),
    tasks: setOf(taskIds, []),
    wrong: setOf(failed.map((f) => f.session), failed.flatMap((f) => failureLights(f, look, turnLights))),
    questions: setOf([...asked.values()].map((q) => q.session), [...asked.keys()].map((cid) => look.call.get(cid)?.id)),
    spent: setOf(spentBy.keys(), []),
  }
  const tally: Tally[] = [
    {
      part: 'sessions', value: String(worked.size), word: worked.size === 1 ? 'session' : 'sessions', sub: started.length ? `${started.length} new` : count(turns, 'turn', 'turns'),
      title: `${count(worked.size, 'session', 'sessions')} worked (${count(turns, 'turn', 'turns')}); ${count(started.length, 'session', 'sessions')} started.`,
      tone: worked.size || started.length ? 'live' : 'idle', focus: focus.sessions,
    },
    {
      part: 'tasks', value: String(ended.length), word: 'finished', sub: `${tasksStarted.length} started`,
      title: `${count(ended.length, 'task', 'tasks')} ended (${tasksFailed.length} not complete); ${count(tasksStarted.length, 'task', 'tasks')} started.`,
      tone: tasksFailed.length ? 'fault' : ended.length ? 'ok' : 'idle', focus: focus.tasks,
    },
    {
      part: 'wrong', value: String(failed.length), word: 'failed', sub: failed.length ? 'went wrong' : 'all clear',
      title: `${count(failed.length, 'failure', 'failures')} while you were away.`, tone: failed.length ? 'fault' : 'ok', focus: focus.wrong,
    },
    {
      part: 'questions', value: String(missed.length), word: 'missed', sub: waiting ? `${waiting} still ${waiting === 1 ? 'waits' : 'wait'}` : `of ${asked.size} asked`,
      title: `${count(asked.size, 'question', 'questions')} asked while you were away; ${missed.length} of them answered or expired before you came back; ${waiting} still waiting.`,
      tone: waiting ? 'wait' : missed.length ? 'wait' : 'idle', focus: focus.questions,
    },
    {
      part: 'spent', value: dollars(usd), word: 'spent', sub: count(calls, 'call', 'calls'),
      title: `${dollars(usd)} spent on ${count(calls, 'model call', 'model calls')} while you were away.`, tone: usd > 0 ? 'money' : 'idle', focus: focus.spent,
    },
  ]
  const all: WatchSet = setOf(
    [...focus.sessions.vessels, ...focus.tasks.vessels, ...focus.wrong.vessels, ...focus.questions.vessels, ...focus.spent.vessels],
    [...focus.wrong.lights, ...focus.questions.lights],
  )
  const begun = away > 0 && end > since
  return {
    key: 'since', question: 'Since you last looked', since, until, away, quiet: quiet || !begun,
    sessions: { started: started.length, worked: worked.size, turns },
    tasks: { started: tasksStarted.length, ended: ended.length, failed: tasksFailed.length },
    failures: failed.length,
    questions: { came: asked.size, went: gone.size, missed: missed.length, waiting },
    usd, calls, tally,
    value: !ready ? '…' : away > 0 ? span(away) : '—',
    unit: 'away',
    caption: !ready ? 'reading the ledger…'
      : away <= 0 ? 'you have looked all along: it tells what happens while you are away'
        : !begun ? 'this moment is before your last look ended'
          : quiet ? 'nothing happened while you were away'
          : `while you were away: ${andList(parts.length ? parts : [`${count(turns, 'turn', 'turns')} ran`])}`,
    tone: failed.length ? 'fault' : missed.length || waiting ? 'wait' : quiet ? 'idle' : 'ok',
    pulse: `${since}|${parts.join(',')}|${worked.size}`,
    lines: ready ? lines.slice(0, LINES) : [], more: ready ? Math.max(0, lines.length - LINES) : 0,
    focus: all,
    link: { to: `/ledger?from=${since}&to=${until}`, label: 'the ledger from then' },
  }
}

/** The moments a replay of the stretch shows: `steps` of them from `from` to `to`, each the next frame. A quiet
 *  stretch longer than `gap` takes the time of `gap`, so the busy minutes get the replay's time, as the ship's log
 *  folds its quiet stretches. */
export function replayMoments(times: readonly number[], from: number, to: number, steps: number, gap = 5 * 60_000): number[] {
  if (to <= from || steps < 2) return [to]
  const ts = times.filter((t) => t > from && t < to).sort((a, b) => a - b)
  const pieces: { a: number; b: number; v: number }[] = []
  let prev = from
  for (const t of [...ts, to]) {
    if (t <= prev) continue
    pieces.push({ a: prev, b: t, v: Math.min(t - prev, gap) })
    prev = t
  }
  const total = pieces.reduce((s, p) => s + p.v, 0)
  const out: number[] = []
  let i = 0
  let acc = 0
  for (let k = 0; k < steps; k++) {
    const v = (total * k) / (steps - 1)
    while (i < pieces.length - 1 && acc + pieces[i].v < v) acc += pieces[i++].v
    const p = pieces[i]
    const f = p.v > 0 ? Math.min(1, Math.max(0, (v - acc) / p.v)) : 1
    out.push(Math.round(p.a + f * (p.b - p.a)))
  }
  out[out.length - 1] = to
  return out
}
