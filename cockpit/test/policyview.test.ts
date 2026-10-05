// `policy.explain` as the Policy view reads it (`src/lib/policyview.ts`, M7 42b), run by `npm test`.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { familyOf, filterTools, placeSummary, sortTools, toolLine } from '../src/lib/policyview.ts'

const layer = (layer: string, result: string, raised = false, setting?: string) => ({ layer, says: layer, result, raised, ...(setting ? { setting } : {}) })
const tool = (name: string, result: string, layers: any[] = [], extra: Record<string, unknown> = {}) => ({ tool: name, class: 'run', offered: true, layers, conditions: [], result, reason: '', ...extra }) as any

const procRun = tool('proc.run', 'approve', [
  layer('place', 'open'), layer('posture', 'notify', true, 'enforcement = notify'), layer('tightening', 'approve', true, '[tightened] proc.run'),
], { conditions: [{ layer: 'allow_argv', when: 'its argv starts with an entry', entries: ['git status'], then: 'open' }] })
const fsRead = tool('fs.read', 'open', [layer('place', 'open'), layer('posture', 'open')])
const gone = tool('web.fetch', 'refused', [layer('place', 'refused', true)], { offered: false, refused: 'not offered in a shared place' })
const mcp = tool('mcp:docs/search', 'notify', [layer('mcp_client', 'notify', true, '[policy.mcp] docs = notify')])

test("a tool's line says where it ended and the layers that raised it", () => {
  assert.deepEqual(toolLine(procRun), {
    tool: 'proc.run', class: 'run', offered: true, result: 'approve', raisedBy: ['posture', 'tightening'],
    settings: ['enforcement = notify', '[tightened] proc.run'], conditions: 1,
  })
  assert.equal(toolLine(fsRead).raisedBy.length, 0)
  assert.equal(toolLine(gone).refused, 'not offered in a shared place')
})

test("a place's summary counts the tools by result and names the tightened ones", () => {
  const p = { place: 'cli', name: 'CLI', class: 'private', tools: [procRun, fsRead, gone, mcp] } as any
  const s = placeSummary(p)
  assert.deepEqual(s.counts, { refused: 1, approve: 1, notify: 1, open: 1 })
  assert.equal(s.notOffered, 1)
  assert.deepEqual(s.tightened, ['proc.run'])
})

test('the tools list the strictest first, then by name, and filter by search and result', () => {
  const all = [fsRead, mcp, procRun, gone]
  assert.deepEqual(sortTools(all).map((t) => t.tool), ['web.fetch', 'proc.run', 'mcp:docs/search', 'fs.read'])
  assert.deepEqual(filterTools(all, 'PROC', null).map((t) => t.tool), ['proc.run'])
  assert.deepEqual(filterTools(all, '', 'notify').map((t) => t.tool), ['mcp:docs/search'])
  assert.deepEqual(filterTools(all, 'tightened', null).map((t) => t.tool), ['proc.run'])
  assert.equal(filterTools(all, 'fs', 'approve').length, 0)
})

test('a tightening layer counts only when it names who tightened', () => {
  // The daemon flags `raised` on the tightening row from the whole decision, so a call outside the roots shows it too.
  const outside = tool('fs.glob', 'approve', [layer('posture', 'open'), layer('tightening', 'approve', true)])
  assert.deepEqual(toolLine(outside).raisedBy, [])
  assert.deepEqual(placeSummary({ place: 'cli', name: 'CLI', class: 'private', tools: [outside, procRun] } as any).tightened, ['proc.run'])
})

test('a tool belongs to its first segment, or its MCP server', () => {
  assert.equal(familyOf('fs.read'), 'fs')
  assert.equal(familyOf('mcp:docs/search'), 'mcp:docs')
})
