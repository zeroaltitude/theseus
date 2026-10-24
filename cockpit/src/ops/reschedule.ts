// `task.reschedule` (ops-verbs, theseus-jid7t.2): a deferred task's start moved before it starts. Not on this lane's
// base yet, so its shape is declared here, exactly as the method takes and answers it; one more call with `previous` as
// `start_at_ms` is the undo. A daemon without it refuses with "unknown method", which the toast says in its words.
// ops-verbs: use @protocol's TaskRescheduleParams/Result at the join
export type TaskRescheduleParams = { task: string; start_at_ms?: number; start_after_ms?: number; start_when?: string }
export type TaskRescheduleResult = { task: string; starts_at_ms: number; previous: number }
