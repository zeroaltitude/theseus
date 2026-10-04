// L1 in words (`src/lib/sandboxwords.ts`), run by `npm test`: the same cases as the CLI's `sandbox:` line
// (`crates/theseus/src/render/sandbox.rs`), so the cockpit's Boundaries and `theseus health` say one thing
// (theseus-vm3n.6). Invented hosts and paths.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { SandboxHealth } from '@protocol'
import { egressWords, l1Head, launchWords, sandboxLine } from '../src/lib/sandboxwords.ts'

const base = (): SandboxHealth => ({ default: 'l0', l1_argv: [], pids: 512, scratch_mb: 1024, output_mb: 64, jobs_l0: 12, jobs_l1: 3 })

test('the line says how the last L1 launch went and what a job gets', () => {
  const s = base()
  assert.ok(sandboxLine(s).startsWith('no L1 job yet since start · default l0'), sandboxLine(s))
  assert.equal(l1Head(s).tone, 'idle')
  s.last_launch = { ok: true, at_ms: 1, start_ms: 3.08, sys: true, lo: false, skipped: ['/opt/gone'] }
  const line = sandboxLine(s)
  assert.ok(line.startsWith('the last L1 launch worked (start 3.1 ms; lo down) · '), line)
  assert.ok(line.includes('jobs: 12 at L0, 3 in L1'), line)
  assert.ok(line.includes('an L1 job gets 512 processes, 1024 MB of scratch, files up to 64 MB'), line)
  assert.ok(line.endsWith(' · ro_paths missing: /opt/gone'), line)
  assert.equal(l1Head(s).tone, 'ok')
  s.last_launch = { ok: false, at_ms: 2, why: 'cloning the init into its namespaces: EPERM', skipped: [] }
  assert.ok(sandboxLine(s).includes('the last L1 launch failed: cloning the init'), sandboxLine(s))
  assert.ok(sandboxLine(s).includes('never runs at L0'))
  assert.ok(sandboxLine(s).includes(' · no egress listed'))
  assert.equal(l1Head(s).tone, 'fault')
  s.refuses = 'the daemon runs as root'
  assert.ok(sandboxLine(s).startsWith('L1 is unavailable: the daemon runs as root · '), sandboxLine(s))
  s.l1_argv = ['cargo build', 'npm install']
  assert.ok(sandboxLine(s).includes(' · always L1: cargo build; npm install · '), sandboxLine(s))
})

test('the egress says the list, and since the start its connections, bytes, and refusals', () => {
  const s: SandboxHealth = {
    ...base(), egress: ['github.com:443', '*.crates.io:443'], egress_connections: 5, egress_up: 1_200, egress_down: 340_100,
    egress_refused: 1, egress_last_refused: "pypi.org:443 is not on this job's egress list",
  }
  assert.equal(egressWords(s),
    "egress: 2 hosts listed (github.com:443, *.crates.io:443); 5 connections, 1.2 KB up, 340.1 KB down; 1 refused (latest: pypi.org:443 is not on this job's egress list)")
  assert.equal(launchWords({ ok: true, at_ms: 0, sys: false, skipped: [] }), 'worked (no sysfs: the kernel refused it)')
  assert.equal(launchWords({ ok: true, at_ms: 0, skipped: [] }), 'worked')
})
