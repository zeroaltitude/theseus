// 18a's cancel verdicts in the daemon's own words, as `tool.ended`'s `verified` says them: "verified: pid namespace, 3
// processes", or "not verified: …".
import type { CancelVerdict } from '@protocol'

const BY: Record<string, string> = {
  pidns: 'pid namespace', cgroup: 'cgroup', tree: 'process tree', group: 'process group', task: 'task', ecs: 'ECS task STOPPED', none: 'nothing',
}

export function verdictWords(v: CancelVerdict): string {
  if (v.state === 'termination_verified') {
    const n = v.killed !== undefined && v.killed !== null ? `, ${v.killed} process${v.killed === 1 ? '' : 'es'}` : ''
    return `verified: ${BY[v.verified_by] ?? v.verified_by}${n}`
  }
  if (v.state === 'unsupported') return `cannot be stopped${v.why ? `: ${v.why}` : ''}`
  return `not verified${v.why ? `: ${v.why}` : ''}`
}
