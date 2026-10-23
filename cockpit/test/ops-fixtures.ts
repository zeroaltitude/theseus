// The Operations page's fixtures (theseus-jid7t.3): an invented board, shared by its tests. Every id and title is made
// up; the clock is a fixed local afternoon, so "today" is the same wherever the tests run.
import type { ConfirmRequest, ExecutionInfo, SessionInfo, TaskInfo, TaskRecord, ThreadGroup, WorkView } from '../src/protocol.gen/index.ts'

/** 14:00 local on an ordinary day: midnight is fourteen hours back, the day ends ten hours on. */
export const NOW = new Date(2026, 9, 21, 14, 0, 0).getTime()
export const MIN = 60_000
export const HOUR = 3_600_000

type Level = WorkView['attention']['level']

export function view(id: string, kind: WorkView['kind'], state: WorkView['state'], over: Partial<WorkView> = {}, level?: Level, since = NOW - 10 * MIN): WorkView {
  const lv: Level = level ?? (state === 'needs_you' ? 'needs_you' : state === 'running' || state === 'queued' || state === 'waiting' ? 'working' : state === 'ready' ? 'ready' : 'idle')
  return {
    id, kind, position: 1, at_ms: NOW - 5 * MIN, root: 'exe_home', title: id, state,
    attention: { level: lv, label: state, since_ms: since }, started_at_ms: NOW - HOUR, state_since_ms: since, ...over,
  } as WorkView
}

export function info(n: number, state: string, over: Partial<TaskInfo> = {}): TaskInfo {
  return {
    task_id: `ses_t${n}`, short: `t${n}`.padStart(6, '0'), execution_id: `exe_t${n}`, parent_session_id: 'ses_home', parent_execution_id: 'exe_home',
    title: `Task ${n}`, state, spent_usd: 0.25, limit_usd: 2, cost_usd: 0.25, turns: 3, pending_confirms: 0, created_at_ms: NOW - HOUR, updated_at_ms: NOW - n * MIN, ...over,
  } as TaskInfo
}

export function session(id: string, over: Partial<SessionInfo> = {}): SessionInfo {
  return {
    session_id: id, kind: 'conversation', label: null, created_at_unix_ms: NOW - 2 * HOUR, turns: 4, usage: {}, last_active_ms: NOW - 5 * MIN,
    cost_usd: 0.5, tool_calls: 2, pending_confirms: 0, profile: 'quick', title: `Chat ${id.slice(-2)}`, ...over,
  } as SessionInfo
}

export function confirm(id: string, over: Partial<ConfirmRequest> = {}): ConfirmRequest {
  return {
    correlation_id: id, session_id: 'ses_home', execution_id: 'exe_home', tool: 'fs.write', input: { path: 'notes/tides.txt' }, reason: 'write under the projects root',
    by: 'policy', requested_at_ms: NOW - 20 * MIN, expires_at_ms: NOW + 10 * MIN, floor: false, ...over,
  } as ConfirmRequest
}

export function group(slug: string, over: Partial<ThreadGroup> = {}): ThreadGroup {
  return { slug, title: slug.replace('-', ' '), state: 'active', session_id: `ses_${slug}`, last_active_ms: NOW - 2 * MIN, context_tokens: 41_000, today_usd: 1.2, ...over }
}

export function record(id: string, over: Partial<TaskRecord> = {}): TaskRecord {
  return {
    id, version: 1, title: id, objective: `the objective of ${id}`, acceptance: [], state: 'open', deps: [], owner: 'agent',
    origin: { session: 'ses_home' }, evidence: [], created_at_ms: NOW - HOUR, updated_at_ms: NOW - HOUR, ...over,
  } as TaskRecord
}

export function execution(id: string, state: string, over: Partial<ExecutionInfo> = {}): ExecutionInfo {
  return { execution_id: id, session_id: `ses_${id.slice(4)}`, kind: 'conversation', state, turns: 2, interrupted: 0, outstanding: 0, queued_results: 0, created_at_ms: NOW - HOUR, updated_at_ms: NOW - 30 * MIN, ...over } as ExecutionInfo
}

/** The invented board: two threads, a few tasks, one question, one approval, one failure. */
export function board() {
  const views: WorkView[] = [
    view('exe_home', 'conversation', 'ready', { session_id: 'ses_home', thread: 'harbour-survey' }),
    view('exe_tides', 'conversation', 'running', { session_id: 'ses_tides', thread: 'tide-tables', doing: { kind: 'model', label: 'thinking' } as WorkView['doing'] }),
    view('exe_t1', 'task', 'needs_you', { session_id: 'ses_t1', title: 'Map the quay walls', thread: 'harbour-survey', question: { id: 'act_q1', prompt: 'Which pier first?', choices: ['north', 'south'] } as WorkView['question'] }, 'needs_you', NOW - 40 * MIN),
    view('act_q1', 'question', 'needs_you', { session_id: 'ses_t1', parent: 'exe_t1', thread: 'harbour-survey', question: { id: 'act_q1', kind: 'ask', prompt: 'Which pier first?', choices: ['north', 'south'], options: [], floor: false, may_answer: true, asked_at_ms: NOW - 40 * MIN, asked_by: { id: 'exe_t1' } } as unknown as WorkView['question'] }, 'needs_you', NOW - 40 * MIN),
    view('exe_t2', 'task', 'running', { session_id: 'ses_t2', title: 'Tabulate the tides', thread: 'tide-tables', moved_to: 'sonnet' }),
    view('exe_t3', 'task', 'running', { session_id: 'ses_t3', title: 'Survey the buoys', paused: true }),
    view('exe_t4', 'task', 'done', { session_id: 'ses_t4', title: 'Count the moorings', ended_at_ms: NOW - HOUR, report: { outcome: 'done', first_line: 'Forty moorings, six free.' } as WorkView['report'] }, 'idle'),
    view('exe_t5', 'task', 'failed', { session_id: 'ses_t5', title: 'Fetch the chart', ended_at_ms: NOW - 2 * HOUR, report: { outcome: 'failed', reason: 'the chart server refused' } as WorkView['report'] }, 'needs_you', NOW - 2 * HOUR),
    view('act_j1', 'job', 'running', { session_id: 'ses_tides', parent: 'exe_tides', thread: 'tide-tables', title: 'cargo test -p tides' }),
    view('wak_r1', 'wake', 'waiting', { session_id: 'ses_home', title: 'check the tide gauge', purpose: 'remind', next_at_ms: NOW + 2 * HOUR }),
    view('wak_late', 'wake', 'waiting', { session_id: 'ses_home', title: 'next week', next_at_ms: NOW + 7 * 24 * HOUR }),
  ]
  const tasks: TaskInfo[] = [
    info(1, 'waiting', { waiting_on: 'confirm', pending_confirms: 1, target: 'discord:dm:4242' }),
    info(2, 'running'),
    info(3, 'running'),
    info(4, 'complete', { updated_at_ms: NOW - HOUR }),
    info(5, 'failed', { updated_at_ms: NOW - 2 * HOUR }),
    info(6, 'queued', { title: 'Paint the lighthouse', starts_at_ms: NOW + 3 * HOUR }),
    info(7, 'complete', { title: 'Old survey', updated_at_ms: NOW - 30 * HOUR }),
  ]
  const sessions: SessionInfo[] = [
    session('ses_home'), session('ses_tides', { profile: 'deep' }), session('ses_t1', { kind: 'task', profile: 'quick' }),
    session('ses_t2', { kind: 'task', profile: 'quick' }),
  ]
  const groups: ThreadGroup[] = [group('harbour-survey'), group('tide-tables', { seal_pending: 'it read a private chart', last_active_ms: NOW - 50 * MIN })]
  const confirms: ConfirmRequest[] = [confirm('act_c1', { session_id: 'ses_t1', task: { task_id: 'ses_t1', short: '0000t1', title: 'Map the quay walls' } })]
  return { views, tasks, sessions, groups, confirms }
}
