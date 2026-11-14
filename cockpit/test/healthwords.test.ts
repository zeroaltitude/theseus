// The header's health lamps (`src/lib/healthwords.ts`), run by `npm test` (theseus-hnof.5): each system's state in a
// word beside its lamp, its card in a sentence, and the bar's summary of the ones that need a look. Invented health.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import {
  binaryLamp, configLamp, discordLamp, diskLamp, freeWords, healthSummary, kernelLamp, linkLamp, providerLamp, secretsLamp,
  memoryLamp, webLamp, type Lamp,
} from '../src/lib/healthwords.ts'

type D = Record<string, any>
const health = (over: D = {}): any => ({
  kernel: { accepting: true, admission_ceiling: 8, turns_held: 0, executions_by_state: { running: 2 } },
  provider_errors: 0,
  bindings: [{ kind: 'discord', state: 'ready', latency_ms: 41 }],
  config: { source: 'file', state: 'confirmed' },
  secrets: { state: 'ready', ready: ['a', 'b', 'c'], resolving: [], failed: [] },
  web: { refused_host: 0, refused_origin: 0, refused_peer: 0 },
  ...over,
})

test('the link says its round trip, or that it is connecting or down', () => {
  assert.deepEqual([linkLamp({ status: 'open', rtt: 64.6 }).word, linkLamp({ status: 'open', rtt: 64.6 }).tone], ['65 ms', 'ok'])
  assert.match(linkLamp({ status: 'open', rtt: 64.6 }).detail, /65 ms round trip \(the median of the last 12 pings\)/)
  assert.equal(linkLamp({ status: 'open', rtt: null }).word, 'open')
  assert.deepEqual([linkLamp({ status: 'connecting', rtt: null }).word, linkLamp({ status: 'connecting', rtt: null }).tone], ['connecting', 'wait'])
  assert.deepEqual([linkLamp({ status: 'closed', rtt: 12 }).word, linkLamp({ status: 'closed', rtt: 12 }).tone], ['down', 'fault'])
})

test('the kernel, the provider and the secrets say their state in a word, and the whole in their card', () => {
  const k = kernelLamp(health())
  assert.deepEqual([k.word, k.tone], ['open', 'ok'])
  assert.equal(k.detail, 'kernel accepting · 2 running of 8 at once · 0 turns held')
  const held = kernelLamp(health({ kernel: { accepting: false, admission_ceiling: 8, turns_held: 1, executions_by_state: {} } }))
  assert.deepEqual([held.word, held.tone, held.detail], ['holding', 'wait', 'kernel holding new turns · 0 running of 8 at once · 1 turn held'])
  assert.deepEqual([providerLamp(health()).word, providerLamp(health()).tone], ['no errors', 'ok'])
  const p = providerLamp(health({ provider_errors: 3 }))
  assert.deepEqual([p.word, p.tone], ['3 errors', 'fault'])
  assert.match(p.detail, /^3 provider errors: model calls the provider failed since the daemon started/)
  assert.equal(providerLamp(health({ provider_errors: 1 })).word, '1 error')
  assert.deepEqual([secretsLamp(health()).word, secretsLamp(health()).detail], ['ready', 'secrets ready · 3 ready'])
  const failed = secretsLamp(health({ secrets: { state: 'failed', ready: [], resolving: [], failed: [{ name: 'jev_api_key', error: 'x' }] } }))
  assert.deepEqual([failed.word, failed.tone, failed.detail], ['1 failed', 'fault', 'secrets failed · 0 ready · failed: jev_api_key'])
})

test('Discord, the config and the web UI: well, off, waiting, or at fault', () => {
  assert.deepEqual([discordLamp(health()).word, discordLamp(health()).tone, discordLamp(health()).detail], ['41 ms', 'ok', 'Discord ready · 41 ms'])
  const off = discordLamp(health({ bindings: [{ kind: 'discord', state: 'disabled' }] }))
  assert.deepEqual([off.word, off.tone], ['off', 'idle'])
  assert.deepEqual([discordLamp(health({ bindings: [] })).word, discordLamp(health({ bindings: [] })).tone], ['none', 'idle'])
  assert.equal(discordLamp(health({ bindings: [{ kind: 'discord', state: 'failed', detail: 'close 4004' }] })).detail, 'Discord failed · close 4004')
  assert.deepEqual([configLamp(health()).word, configLamp(health()).detail], ['ok', 'config confirmed (file)'])
  assert.deepEqual([configLamp(health({ config: { source: 'vault', state: 'held' } })).word, configLamp(health({ config: { source: 'vault', state: 'held' } })).tone], ['held', 'wait'])
  assert.deepEqual([webLamp(health()).word, webLamp(health()).tone], ['guarded', 'ok'])
  const dev = webLamp(health({ web: { refused_host: 0, refused_origin: 0, refused_peer: 0, dev_origin: 'http://127.0.0.1:5174', dev_origin_served: 3 } }))
  assert.deepEqual([dev.word, dev.tone], ['dev page', 'wait'])
  assert.equal(dev.detail, 'web UI refused 0 by address, 0 by page, 0 by user · dev origin open: http://127.0.0.1:5174 (3 served)')
  assert.deepEqual([webLamp(health({ web: { refused_host: 2, refused_origin: 1, refused_peer: 0 } })).word], ['3 refused'])
  assert.deepEqual([webLamp(health({ web: { refused_host: 0, refused_origin: 0, refused_peer: 0, peer_unchecked: 'config' } })).tone], ['fault'])
})

test('the binary and the disk read their own lines, and say their state in a word', () => {
  assert.deepEqual([binaryLamp({ state: 'ok' } as any, 'binary: ok').word, binaryLamp({ state: 'ok' } as any, 'binary: ok').detail], ['safe', 'binary: ok'])
  assert.deepEqual([binaryLamp({ state: 'jobs_can_write' } as any, 'x').word, binaryLamp({ state: 'jobs_can_write' } as any, 'x').tone], ['jobs can write', 'fault'])
  assert.equal(binaryLamp(undefined, 'binary: not reported by this daemon').tone, 'idle')
  const disk = (state: string, free_mb: number) => ({ path: '/s', state, free_mb, total_mb: 500_000, warn_mb: 2048, floor_mb: 512 })
  assert.deepEqual([diskLamp(disk('ok', 186_368), 's').word, diskLamp(disk('ok', 186_368), 's').tone], ['182 GB free', 'ok'])
  assert.deepEqual([diskLamp(disk('low', 1500), 's').word, diskLamp(disk('low', 1500), 's').tone], ['1.5 GB left', 'wait'])
  assert.deepEqual([diskLamp(disk('below_floor', 300), 's').word, diskLamp(disk('below_floor', 300), 's').tone], ['jobs refused', 'fault'])
  assert.deepEqual([freeWords(512), freeWords(3481), freeWords(186_368)], ['512 MB', '3.4 GB', '182 GB'])
})

test('the summary names the lamps that need a look, the worst first, or says all is well', () => {
  const lamp = (id: string, tone: any, word: string): Lamp => ({ id, name: id, tone, word, detail: '' })
  const well = healthSummary([lamp('link', 'ok', '9 ms'), lamp('discord', 'idle', 'off'), lamp('config', 'ok', 'ok')])
  assert.deepEqual([well.tone, well.words, well.trouble.length], ['ok', 'all 3 well', 0])
  const bad = healthSummary([lamp('link', 'ok', '9 ms'), lamp('web', 'wait', 'dev page'), lamp('provider', 'fault', '1 error'), lamp('disk', 'wait', '1.5 GB left')])
  assert.equal(bad.tone, 'fault')
  assert.equal(bad.words, 'provider 1 error · web dev page · disk 1.5 GB left')
})

// Theseus's memory search (theseus-43g1): health carries the index process's state, counts, lag, vectors, last error and
// restarts; the bar shows them as a lamp named for what a person knows it as, never for the process.
const index = (over: D = {}): any => ({
  state: 'ready',
  tender: { name: 'index', state: 'running', restarts: 0, adopted: false, started_at_ms: 1, last_exit_ms: 0, backoff_ms: 0 },
  status: {
    state: 'ready', nodes: 12400, lag: { bytes: 0, ms: 0 }, vectors: { vectors: 31000, pending: 0 }, last_error: undefined, rebuilds: 0,
  },
  ...over,
})

test('the memory lamp says ready, catching up, down or off in plain words, with its numbers in the card', () => {
  const ready = memoryLamp(health({ index: index() }))
  assert.deepEqual([ready.id, ready.name, ready.word, ready.tone], ['memory', 'memory', 'ready', 'ok'])
  assert.equal(ready.detail, "Theseus's memory search: ready · 12,400 notes held · searching by meaning is on (31,000 pieces)")
  const behind = memoryLamp(health({ index: index({ status: { ...index().status, lag: { bytes: 900, ms: 2400 } } }) }))
  assert.match(behind.detail, / · 2 s behind the latest/)
  const later = memoryLamp(health({ index: index({ status: { ...index().status, lag: { bytes: 9, ms: 600_000 } } }) }))
  assert.match(later.detail, / · 10 min behind the latest/)
  const catching = memoryLamp(health({ index: index({ state: 'backfilling', status: { ...index().status, state: 'backfilling', vectors: { vectors: 100, pending: 4000 } } }) }))
  assert.deepEqual([catching.word, catching.tone], ['catching up', 'wait'])
  assert.match(catching.detail, /searching by meaning is on \(100 pieces, 4,000 still to do\)/)
  const down = memoryLamp(health({ index: { state: 'down', why: 'the tender exited: signal 9', tender: { ...index().tender, restarts: 3, state: 'backoff' } } }))
  assert.deepEqual([down.word, down.tone], ['down', 'fault'])
  assert.equal(down.detail, "Theseus's memory search: not running · restarted 3 times", 'the daemon\'s own why is not shown')
  const sick = memoryLamp(health({ index: index({ status: { ...index().status, last_error: 'a frame would not read', last_error_ms: 1_000_000 - 180_000 } }) }), 1_000_000)
  assert.match(sick.detail, / · last problem 3 min ago: a frame would not read$/)
  // Switched off is nothing wrong, whatever the daemon says of why.
  const off = memoryLamp(health({ index: { state: 'off', why: '[index] enabled = false' } }))
  assert.deepEqual([off.word, off.tone, off.detail], ['off', 'idle', "Theseus's memory search: switched off"])
  assert.deepEqual([memoryLamp(health()).word, memoryLamp(health()).tone], ['—', 'idle'])
  assert.deepEqual([memoryLamp(health({ index: index({ state: 'strange' }) })).word, memoryLamp(health({ index: index({ state: 'strange' }) })).tone], ['strange', 'wait'])
  assert.equal(memoryLamp(health({ index: index({ status: { ...index().status, vectors: undefined } }) })).detail.includes('searching by meaning is off'), true)
})

test('the memory lamp never names the process, and a fault in it reaches the bar\'s summary', () => {
  for (const state of ['ready', 'backfilling', 'starting', 'stalled', 'down', 'off']) {
    const l = memoryLamp(health({ index: index({ state }) }))
    assert.doesNotMatch(`${l.name} ${l.word} ${l.detail}`.replace(/restarted/, ''), /index|tender|\[|vector/i, state)
  }
  const down = memoryLamp(health({ index: { state: 'down' } }))
  assert.equal(healthSummary([linkLamp({ status: 'open', rtt: 9 }), down]).words, 'memory down')
  const bar = readFileSync(new URL('../src/components/Heartbeat.tsx', import.meta.url), 'utf8')
  assert.match(bar, /webLamp\(h\), memoryLamp\(h\),/, 'the bar draws it')
})
