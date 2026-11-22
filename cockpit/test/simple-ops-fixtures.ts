// The simple Ops page's fixtures: one store both Ops pages are read over (`simple-ops-parity.test.ts`) and the keys are
// resolved against (`simple-ops-keys.test.ts`). Every id and title is made up.
import type { ConfirmRequest, WorkView } from '../src/protocol.gen/index.ts'
import type { WorkSetInput } from '../src/simple/ops/workset.ts'
import { board, confirm, HOUR, MIN, NOW, record, session, view } from './ops-fixtures.ts'

/** The invented board of the Ship's tests, with plans and steps under it, a job, a loose approval (in a conversation
 *  no card holds) and a held
 *  question, a conversation at rest, and ledger rows for today's spend. */
export function store(): WorkSetInput {
  const b = board()
  const views: WorkView[] = [
    ...b.views,
    view('exe_pl', 'conversation', 'running', { session_id: 'ses_pl' }),
    view('act_j2', 'job', 'running', { session_id: 'ses_pl', parent: 'exe_pl', title: 'cargo build' }),
  ]
  const records = [
    record('tsk_plan', { title: 'Survey the quays', state: 'in_progress', origin: { session: 'ses_pl' } as never }),
    record('tsk_s1', { title: 'Measure pier one', state: 'done', parent: 'tsk_plan', updated_at_ms: NOW - 30 * MIN, origin: { session: 'ses_pl' } as never }),
    record('tsk_s2', { title: 'Chart the shoals', state: 'accepted', parent: 'tsk_plan', origin: { session: 'ses_pl' } as never }),
    record('tsk_s3', { title: 'Sound the channel', state: 'accepted', parent: 'tsk_s2', origin: { session: 'ses_pl' } as never }),
    record('tsk_sus', { title: 'Paint the beacon', state: 'suspended', parent: 'tsk_plan', origin: { session: 'ses_pl' } as never }),
    record('tsk_blk', { title: 'Wait for the permit', state: 'blocked', parent: 'tsk_plan', origin: { session: 'ses_pl' } as never }),
    record('tsk_b1', { title: 'Order buoys', state: 'accepted', origin: { session: 'ses_pl' } as never }),
  ]
  const loose = confirm('act_c9', { session_id: 'ses_elsewhere', execution_id: 'exe_elsewhere', requested_at_ms: NOW - 5 * MIN })
  const confirms: ConfirmRequest[] = [...b.confirms, confirm('act_c2', { session_id: 'ses_t1', tool: 'fs.delete' }), loose]
  const sessions = [...b.sessions, session('ses_pl'), session('ses_rest', { last_active_ms: NOW - 3 * HOUR })]
  // oldest first, as the ledger's rows are: yesterday's call is not today's spend
  const history = [
    { kind: 'provider.call', at_unix_ms: NOW - 30 * HOUR, session_id: 'ses_home', data: { cost_usd: 9 } },
    { kind: 'provider.call', at_unix_ms: NOW - 2 * HOUR, session_id: 'ses_home', data: { cost_usd: 0.4 } },
  ]
  return { views, tasks: b.tasks, sessions, records, confirms, history, now: NOW }
}

