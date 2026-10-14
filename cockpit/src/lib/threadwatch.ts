// The work board's threads on the page (theseus-5i8i): one `work.watch` on the page's connection, kept by the position
// rule (`threadboard.ts`) and read by the Threads views. The page asks only while a Threads view is mounted
// (`useThreadBoard`), and stops asking when the last one goes: the watch is dropped with `work.unwatch`.
import { useEffect } from 'react'
import { create } from 'zustand'
import type { WorkChanged, WorkWatchResult } from '@protocol'
import { client, useConn } from './rpc'
import { applyChanged, applySnapshot, EMPTY, type BoardState } from './threadboard'

interface WatchState { board: BoardState; ready: boolean; error: string | null }

const useWatch = create<WatchState>(() => ({ board: EMPTY, ready: false, error: null }))

let users = 0
let off: Array<() => void> = []

async function read(reset: boolean) {
  try {
    const snap = await client.call<WorkWatchResult>('work.watch', { limit: 0, interactive: true })
    useWatch.setState((s) => ({ board: applySnapshot(reset ? EMPTY : s.board, snap), ready: true, error: null }))
  } catch (e) {
    useWatch.setState({ ready: true, error: e instanceof Error ? e.message : String(e) })
  }
}

function start() {
  off = [
    // After a (re)connect the first snapshot is the truth, whatever positions the last daemon had.
    client.onOpen(() => void read(true)),
    client.onNotify((method, params) => {
      if (method === 'work.changed') useWatch.setState((s) => ({ board: applyChanged(s.board, params as WorkChanged) }))
      else if (method === 'events.lost') void read(false)
    }),
  ]
  if (useConn.getState().status === 'open') void read(true)
}

function stop() {
  off.forEach((f) => f())
  off = []
  useWatch.setState({ board: EMPTY, ready: false, error: null })
  if (useConn.getState().status === 'open') client.call('work.unwatch').catch(() => {})
}

/** The board's threads and work while the calling view is mounted. */
export function useThreadBoard() {
  useEffect(() => {
    if (users++ === 0) start()
    return () => {
      if (--users === 0) stop()
    }
  }, [])
  return useWatch()
}
