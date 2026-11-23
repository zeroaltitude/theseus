import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { runInNewContext } from 'node:vm'
import ts from 'typescript'
import { createReader, drawnIds } from '../src/ship/readplan.ts'
const src = (p: string) => readFileSync(new URL('../src/' + p, import.meta.url), 'utf8')

test('Ship rebuilds its bounded reader after reconnect and loss', () => {
  const s = src('ship/useShipData.ts')
  assert.match(s, /\[open, store, epoch\]/)
  assert.match(s, /\[open, planKey, store, epoch\]/)
  assert.match(s, /case 'events.lost':[\s\S]*?setEpoch\(\(n\) => n \+ 1\)/)
})

test('reconnect keeps drawn ships drawn while a fresh reader re-asks the whole plan', async () => {
  // Run the hook's two actual reader effects with deferred reads, without mounting the rest of the Ship.
  const s = src('ship/useShipData.ts')
  const effects = s.slice(s.indexOf('  useEffect(() => {', s.indexOf('const [epoch,')), s.indexOf('  const loadAll ='))
  let state: any = { read: new Set(['a', 'b']), known: new Set(), nodes: new Map(), progress: 1 }
  const sessions: any[] = ['a', 'b'].map((session_id) => ({ session_id, kind: 'conversation', state: 'live', pending_confirms: 0 }))
  const asks: string[] = []
  const finishes: (() => void)[] = []
  const reader: any = { current: null }
  const freshReader = { current: false }
  let cleanup = () => {}
  const context = {
    open: true, store: { getState: () => state, setState: (v: any) => { state = { ...state, ...(typeof v === 'function' ? v(state) : v) } } },
    epoch: 0, planKey: 'a,b', reader, freshReader, createReader,
    call: (_m: string, p: any) => { asks.push(p.session_id); return new Promise((resolve) => finishes.push(() => resolve({ nodes: [] }))) },
    mergeRead: (st: any, sid: string) => ({ read: new Set([...st.read, sid]) }),
    useEffect: (f: () => any) => { const c = f(); if (c) cleanup = c },
  }
  const run = () => runInNewContext(ts.transpileModule(effects, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context)
  run()
  assert.deepEqual([...drawnIds(sessions, 'live', [], state.read)], ['a', 'b'], 'drawn ships stay visible before refresh replies')
  assert.deepEqual(asks, ['a', 'b'], 'a new reader asks even for already drawn sessions')
  finishes.splice(0).forEach((f) => f())
  // Drain the reader promise chain, without a clock or network.
  for (let i = 0; i < 5; i++) await Promise.resolve()
  cleanup()
  context.epoch++
  run()
  assert.deepEqual([...drawnIds(sessions, 'live', [], state.read)], ['a', 'b'], 'another gap does not blank the fleet')
  assert.deepEqual(asks, ['a', 'b', 'a', 'b'], 'each replacement reader refreshes the full plan')
  assert.equal(freshReader.current, false, 'the whole-plan request is only for the new reader')
  cleanup()
  finishes.splice(0).forEach((f) => f())
})
