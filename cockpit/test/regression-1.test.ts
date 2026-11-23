import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
const src = (p: string) => readFileSync(new URL('../src/' + p, import.meta.url), 'utf8')
import ts from 'typescript'
import { runInNewContext } from 'node:vm'
test('a bubbling task shortcut dispatches once even before busy rerenders', async () => {
  let calls = 0
  let finish: (x: unknown) => void = () => {}
  const request = new Promise((resolve) => { finish = resolve })
  const jsx = (type: unknown, props: any) => ({ type, props })
  const mocks: Record<string, any> = {
    react: { useState: (v: any) => [v, () => {}], useRef: (v: any) => ({ current: v }), useEffect: () => {} },
    'react/jsx-runtime': { jsx, jsxs: jsx },
    'lucide-react': { Send: 'Send', X: 'X' },
    '@/lib/rpc': { useRpc: () => ({}), call: () => { calls++; return request } },
    '@/lib/format': { cn: () => '', usd: String },
    './act': { useOpsAct: () => ({ on: () => true }) },
    './Sheet': { Sheet: 'Sheet', FIELD: '' },
    '@/components/Help': { HelpButton: 'HelpButton' }, './ActBtn': { ActBtn: 'ActBtn' },
    './newtask': { EMPTY_FORM: {}, dollarsOf: () => ({}), profileChoices: () => ({ choices: [] }), startParams: () => ({ brief: 'test' }) },
  }
  const load = (file: string) => {
    const exports: any = {}
    runInNewContext(ts.transpileModule(src(file), { compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX } }).outputText, { exports, require: (m: string) => mocks[m] })
    return exports
  }
  const task = load('ops/NewTask.tsx').NewTask({ onStarted: () => {}, onClose: () => {} })
  const form = load('ops/Sheet.tsx').Sheet(task.props).props.children
  const textarea = task.props.children[0]
  let prevented = false
  const event = { key: 'Enter', ctrlKey: true, preventDefault: () => { prevented = true } }
  textarea.props.onKeyDown?.(event)
  form.props.onKeyDown(event)
  // A second invocation before React commits busy must not duplicate the RPC either.
  task.props.onSubmit()
  assert.equal(calls, 1)
  assert.equal(prevented, true)
  finish({ short: 'task', limit_usd: 1, task_id: 'task' })
  await request
})
