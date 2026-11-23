// The Join pane's composer (theseus-jid7t.6): ⏎ steers a running turn, ⌘⏎ holds a message until the turn ends, esc drops it.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { composerHint, composerStep, submitOf, type ComposerState } from '../src/ops/composer.ts'
import { attachRefusal, attachmentOf, base64, MAX_ATTACH_BYTES, overCap, readAttachment, withoutSent } from '../src/ops/attach.ts'

const idle: ComposerState = { kind: 'idle' }
const running: ComposerState = { kind: 'running' }

test('⏎ with nothing running sends; while a turn runs it steers', () => {
  assert.deepEqual(composerStep(idle, { kind: 'enter', text: ' hello ' }).effect, { send: 'hello', steer: false })
  assert.deepEqual(composerStep(running, { kind: 'enter', text: 'go left' }).effect, { send: 'go left', steer: true })
  assert.equal(composerStep(running, { kind: 'enter', text: '  ' }).effect, null, 'nothing typed, nothing sent')
})

test('⌘⏎ while running holds the message until the turn ends, then sends it once', () => {
  const held = composerStep(running, { kind: 'mod_enter', text: 'then this' })
  assert.deepEqual(held.state, { kind: 'queued', text: 'then this' })
  assert.equal(held.effect, null, 'nothing is sent yet')
  assert.deepEqual(composerStep(held.state, { kind: 'turn_started' }).state, held.state, 'a start does not release it')
  const end = composerStep(held.state, { kind: 'turn_ended' })
  assert.deepEqual(end.effect, { send: 'then this', steer: false })
  assert.deepEqual(end.state, { kind: 'idle' })
  assert.equal(composerStep(end.state, { kind: 'turn_ended' }).effect, null, 'never twice')
})

test('⌘⏎ with nothing running is a plain send, and esc drops a held message', () => {
  assert.deepEqual(composerStep(idle, { kind: 'mod_enter', text: 'hi' }).effect, { send: 'hi', steer: false })
  const dropped = composerStep({ kind: 'queued', text: 'x' }, { kind: 'escape' })
  assert.deepEqual(dropped.state, running)
  assert.equal(composerStep(dropped.state, { kind: 'turn_ended' }).effect, null, 'dropped: nothing goes at the end')
  assert.deepEqual(composerStep(idle, { kind: 'escape' }).state, idle)
})

test('the composer says what ⏎ and ⌘⏎ do', () => {
  assert.match(composerHint(running), /⏎ steers.*⌘⏎ queues/)
  assert.match(composerHint({ kind: 'queued', text: 'x' }), /esc drops/)
  assert.match(composerHint(idle), /⏎ sends/)
})

test('a steer sends no profile and keeps the choice; a send clears it only when the daemon answers ok', () => {
  const steer = submitOf({ sid: 'ses_a', input: 'wait', turnProfile: 'deep', steer: true })
  assert.deepEqual(steer.params, { session_id: 'ses_a', input: 'wait' })
  assert.equal(steer.clearOnOk, false, 'the choice is for the next turn')
  const send = submitOf({ sid: 'ses_a', input: 'go', turnProfile: 'deep', steer: false })
  assert.deepEqual(send.params, { session_id: 'ses_a', input: 'go', profile: 'deep' })
  assert.equal(send.clearOnOk, true)
  assert.equal(submitOf({ sid: 'ses_a', input: 'go', turnProfile: null, steer: false }).clearOnOk, false, 'nothing chosen, nothing to clear')
})

test('the pane clears the choice in the answer’s then, never before the call', () => {
  const src = readFileSync(new URL('../src/ops/JoinPane.tsx', import.meta.url), 'utf8')
  const send = src.slice(src.indexOf('const send ='), src.indexOf('const step ='))
  assert.ok(send.indexOf("post('join'") < send.indexOf('setTurnProfile(sid, null)'), 'cleared after the post is made')
  assert.match(send, /\.then\(\(\) => \{[^}]*clearOnOk[^}]*setTurnProfile\(sid, null\)/)
})

// The file a message carries (theseus-qgsv): the browser's half of `crates/theseus/src/attach.rs`, whose own test
// (`attach_sends_a_text_files_text_and_lists_anything_else`) pins these same two files and the same base64.
test('a text file travels as text and anything else as its bytes, in the shape the CLI builds', () => {
  const text = new TextEncoder().encode('The fact is 42.\n')
  assert.deepEqual(attachmentOf({ name: 'notes.txt', size: text.length }, text), {
    name: 'notes.txt', media_type: 'text/plain', size: 16, text: 'The fact is 42.\n',
  })
  const png = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])
  assert.deepEqual(attachmentOf({ name: 'shot.png', size: png.length }, png), {
    name: 'shot.png', media_type: '', size: 8, data: 'iVBORw0KGgo=',
  })
  // A notebook and an RTF file are text the daemon reads as a document: bytes, as that builder sends them.
  const nb = new TextEncoder().encode('{"cells": []}')
  assert.equal(attachmentOf({ name: 'run.ipynb', size: nb.length }, nb).text, undefined)
  assert.equal(attachmentOf({ name: 'run.ipynb', size: nb.length }, nb).data, base64(nb))
  // Bytes that are not UTF-8, and text with a NUL in it, are bytes too.
  assert.equal(attachmentOf({ name: 'x.bin', size: 2 }, new Uint8Array([0xff, 0xfe])).data, '//4=')
  assert.equal(attachmentOf({ name: 'x.txt', size: 2 }, new Uint8Array([0x61, 0x00])).text, undefined)
  assert.equal(base64(new TextEncoder().encode('any')), 'YW55')
})

test('a file over the cap is listed, not read, and the composer refuses it in the chat’s words', async () => {
  const big = { name: 'swell.mov', size: MAX_ATTACH_BYTES + 1 }
  const a = await readAttachment({ ...big, arrayBuffer: async () => { throw new Error('an oversized file must not be read') } })
  assert.deepEqual(a, { name: 'swell.mov', media_type: '', size: MAX_ATTACH_BYTES + 1, not_read: overCap('the composer') })
  assert.equal(a.data, undefined, 'its bytes are never sent')
  assert.equal(overCap('the composer'), 'over the 32 MiB limit for the composer')
  assert.equal(attachRefusal([a]), 'swell.mov: over the 32 MiB limit for the composer')
  const ok = attachmentOf({ name: 'notes.txt', size: 3 }, new TextEncoder().encode('hey'))
  assert.equal(attachRefusal([ok]), null, 'a file that was read is no refusal')
  assert.equal(attachRefusal([]), null)
})

test('a chosen file rides turn.submit as an attachment, and none leaves the params as they were', () => {
  const a = attachmentOf({ name: 'shot.png', size: 8 }, new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]))
  const sent = submitOf({ sid: 'ses_a', input: 'look at this', turnProfile: null, steer: false, attachments: [a] })
  assert.deepEqual(sent.params, { session_id: 'ses_a', input: 'look at this', attachments: [a] })
  assert.deepEqual(submitOf({ sid: 'ses_a', input: 'go', turnProfile: 'deep', steer: false, attachments: [] }).params,
    { session_id: 'ses_a', input: 'go', profile: 'deep' }, 'no file chosen: the params of before, with no attachments key')
  assert.deepEqual(submitOf({ sid: 'ses_a', input: 'go', turnProfile: 'deep', steer: false }).params,
    { session_id: 'ses_a', input: 'go', profile: 'deep' })
})

test('the pane reads the file off the page and refuses before it sends', () => {
  const pane = readFileSync(new URL('../src/ops/JoinPane.tsx', import.meta.url), 'utf8')
  assert.match(pane, /readAttachment\(/, 'the pane reads through the capped reader')
  const reader = readFileSync(new URL('../src/ops/attach.ts', import.meta.url), 'utf8')
  assert.match(reader, /attachmentOf\(file, new Uint8Array\(await file.arrayBuffer\(\)\)\)/, 'the browser’s bytes still go through the one builder')
  const send = pane.slice(pane.indexOf('const send ='), pane.indexOf('const step ='))
  assert.ok(send.indexOf('attachRefusal(') < send.indexOf("post('join'"), 'a file that was not read is refused before any call')
})

// A send that was refused or failed keeps the files the person chose, as it keeps the draft's text.
test('the files a send carried leave the composer when it resolves, and a file added meanwhile stays', () => {
  const a = attachmentOf({ name: 'a.txt', size: 1 }, new TextEncoder().encode('a'))
  const b = attachmentOf({ name: 'b.txt', size: 1 }, new TextEncoder().encode('b'))
  const c = attachmentOf({ name: 'c.txt', size: 1 }, new TextEncoder().encode('c'))
  assert.deepEqual(withoutSent([a, b, c], [a, b]), [c], 'only what the send carried goes')
  assert.deepEqual(withoutSent([a], []), [a], 'a send with no file clears none')
  const pane = readFileSync(new URL('../src/ops/JoinPane.tsx', import.meta.url), 'utf8')
  const send = pane.slice(pane.indexOf('const send = ('), pane.indexOf('const step = ('))
  assert.ok(!/^\s*setFiles\(\[\]\)/m.test(send), 'the files are not cleared before the send resolves')
  const resolved = send.slice(send.indexOf('.then('), send.indexOf('.catch('))
  assert.match(resolved, /setFiles\(\(was\) => withoutSent\(was, sent\)\)/, 'they go in the resolved branch')
  assert.ok(!/setFiles/.test(send.slice(send.indexOf('.catch('))), 'a failed send keeps them')
})
