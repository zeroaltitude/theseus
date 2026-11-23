import test from 'node:test'
import assert from 'node:assert/strict'
import { budgetWords, dropOf, type DragItem } from '../src/ops/rehome.ts'

test('rehome labels do not veto any destination; confirmation and original budget stay', () => {
  const task: DragItem = { id: 'ses_task', kind: 'task', title: 'Prepare tides', sessionId: 'ses_task', ended: false, home: 'ses_original', homeTitle: 'Original chat' }
  for (const data of ['open', 'private', 'sensitive', undefined] as const) {
    for (const target of ['open', 'private', 'sensitive', undefined] as const) {
      assert.deepEqual(dropOf({ ...task, data }, { kind: 'conversation', sessionId: 'ses_next', title: 'Next chat', data: target }, false), {
        method: 'task.rehome', params: { task: 'ses_task', home: 'ses_next' }, posture: 'ask',
        words: `Its report goes to the conversation “Next chat” from now on. ${budgetWords('Original chat')}`,
      }, `${data} to ${target}`)
    }
  }
})
