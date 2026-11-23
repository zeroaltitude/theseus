import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
const src = (p: string) => readFileSync(new URL('../src/' + p, import.meta.url), 'utf8')
test('job tails restart on reconnect and loss, ignoring earlier peek replies', () => {
  const s = src('ops/jobout.ts'); assert.match(s, /client.onOpen\(restart\)/); assert.match(s, /m === 'events.lost'/); assert.match(s, /epoch !== generation/); assert.match(s, /offOpen\(\)/)
})

import ts from 'typescript'
import { runInNewContext } from 'node:vm'
import * as follow from '../src/ops/follow.ts'
import * as live from '../src/ops/live.ts'
test('a reconnect and a lost-event notice re-peek and re-follow the selected job', async () => {
  const requests: { method: string; params: any }[] = []
  const peeks: ((x: any) => void)[] = []
  let open = () => {}
  let notify = (_m: string, _p: unknown) => {}
  let cleanup = () => {}
  let out: any = null
  let removed = 0
  const mocks: Record<string, any> = {
    react: { useState: () => [null, (v: any) => { out = typeof v === 'function' ? v(out) : v }], useEffect: (f: any) => { cleanup = f() } },
    '@/lib/rpc': {
      call: (method: string, params: any) => {
        requests.push({ method, params })
        return method === 'work.peek' ? new Promise((resolve) => peeks.push(resolve)) : Promise.resolve({})
      },
      client: { onOpen: (f: any) => { open = f; return () => { removed++ } }, onNotify: (f: any) => { notify = f; return () => { removed++ } } },
    }, './follow': follow, './live': live,
  }
  const exports: any = {}
  runInNewContext(ts.transpileModule(src('ops/jobout.ts'), { compilerOptions: { module: ts.ModuleKind.CommonJS } }).outputText, { exports, require: (m: string) => mocks[m] })
  exports.useJobOutput('job')
  open()
  assert.equal(peeks.length, 2)
  peeks[0]({ lines: ['stale'], bytes: 5 })
  await Promise.resolve()
  assert.equal(out, null)
  peeks[1]({ lines: ['fresh'], bytes: 10 })
  await Promise.resolve()
  assert.equal(requests.filter((r) => r.method === 'action.output').length, 1)
  notify('events.lost', {})
  assert.equal(peeks.length, 3)
  peeks[2]({ lines: ['latest'], bytes: 20 })
  await Promise.resolve()
  assert.equal(out.lines[0], 'latest')
  assert.equal(requests.filter((r) => r.method === 'action.output').length, 2)
  cleanup()
  assert.equal(removed, 2)
  assert.equal(requests.at(-1)?.method, 'action.unfollow')
})
