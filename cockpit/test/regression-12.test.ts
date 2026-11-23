import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
const src = (p: string) => readFileSync(new URL('../src/' + p, import.meta.url), 'utf8')
test('the re-home sheet says the budget the daemon keeps: the original parent pays, not the new home', () => {
  // task.rehome moves only where the report goes; the budget stays carved from the parent. The sheet said the new
  // home's budget would pay, the drag's words (budgetWords) already said the parent's.
  const t = src('ops/TaskSheets.tsx')
  const sheet = t.slice(t.indexOf('export function RehomeSheet'), t.indexOf('export function RescheduleSheet'))
  assert.doesNotMatch(sheet, /home's budget|home’s budget|counts against/, 'the sheet says the new home pays')
  assert.match(sheet, /const parent = \(data\?\.sessions \?\? \[\]\)\.find\(\(s\) => s\.session_id === home\)/)
  assert.match(sheet, /note=\{`Its report goes to the new home from now on\. \$\{budgetWords\(parent \? /)
  assert.match(src('ops/Inspector.tsx'), /<RehomeSheet task=\{r\.id\} title=\{f\.title\} home=\{r\.task\?\.info\?\.parent_session_id\}/, 'the sheet is given the parent')
})
