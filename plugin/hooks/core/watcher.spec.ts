import { describe, expect, it } from 'vitest'

import type { Ports, VerbResult } from './ports.ts'
import { CARD_CHANGED, NET_MS, POLL_MS, WAKE_UP, createChildWatcher } from './watcher.ts'

const IDS = { sessionId: 's1', gen: '/d/crook/p/gen' }
const START = { kind: 'start', calledTake: false, calledDone: false } as const
const END = { kind: 'complete', calledTake: false, calledDone: false } as const

type Snap = {
  untaken: number
  taken: number
  newest_seq: number
  task_open: boolean
  parked: boolean
  is_current: boolean
  current_id: string
  card_version: number
}

const IDLE_INBOX: Snap = {
  untaken: 3,
  taken: 0,
  newest_seq: 3,
  task_open: false,
  parked: false,
  is_current: true,
  current_id: 's1',
  card_version: 1,
}

/** Fake ports: a clock that moves only when the test moves it, a `gen` mtime and a snapshot the test edits. */
function world(snap: Partial<Snap> = {}) {
  const w = {
    t: 0,
    mtime: 1 as number | null,
    snap: { ...IDLE_INBOX, ...snap } as Snap,
    reads: 0,
    runs: [] as { verb: string; input: object }[],
    submits: [] as string[],
    own: [] as (string | undefined)[],
    every: [] as number[],
    /** When set, `run` waits on it, so a read stays in flight. */
    hold: undefined as Promise<void> | undefined,
  }
  const p: Ports = {
    async run(verb, input): Promise<VerbResult> {
      w.runs.push({ verb, input })
      if (w.hold !== undefined) await w.hold
      w.reads++
      return { ok: true, ...w.snap }
    },
    stat: async () => (w.mtime === null ? null : { mtimeMs: w.mtime }),
    submit: async text => {
      w.submits.push(text)
    },
    status: () => {},
    every: ms => {
      w.every.push(ms)
    },
    now: () => w.t,
    env: () => undefined,
  }
  const watcher = createChildWatcher(p, IDS, text => w.own.push(text))
  return { w, watcher }
}

/** Lets fire-and-forget work (`onTurn`'s tick) finish. */
async function flush(): Promise<void> {
  for (let i = 0; i < 20; i++) await Promise.resolve()
}

/** One poll, `POLL_MS` after the last. */
async function poll(w: { t: number }, watcher: { tick(): Promise<void> }) {
  w.t += POLL_MS
  await watcher.tick()
}

/** A main-loop turn that the session ran and ended. */
async function turn(watcher: ReturnType<typeof createChildWatcher>, end: Partial<typeof END> = {}) {
  watcher.onTurn(START)
  watcher.onTurn({ ...END, ...end })
  await flush()
}

describe('child watcher', () => {
  it('reads on the first tick, then skips while gen is unchanged until the safety net', async () => {
    const { w, watcher } = world({ untaken: 0 })
    await watcher.tick()
    expect(w.reads).toBe(1)
    expect(w.runs[0]).toEqual({ verb: 'snapshot', input: { session_id: 's1', user_turn: false } })
    for (let i = 0; i < 10; i++) await poll(w, watcher)
    expect(w.reads).toBe(1)
    w.mtime = 2
    await poll(w, watcher)
    expect(w.reads).toBe(2)
    w.t += NET_MS
    await watcher.tick()
    expect(w.reads).toBe(3)
  })

  it('takes the gen mtime before the read, so a change during the read is read next tick', async () => {
    const { w, watcher } = world({ untaken: 0 })
    let release = () => {}
    w.hold = new Promise(r => (release = r))
    const first = watcher.tick()
    await flush()
    w.mtime = 2
    release()
    await first
    w.hold = undefined
    await poll(w, watcher)
    expect(w.reads).toBe(2)
    await poll(w, watcher)
    expect(w.reads).toBe(2)
  })

  it('skips a tick while a read is in flight', async () => {
    const { w, watcher } = world({ untaken: 0 })
    let release = () => {}
    w.hold = new Promise(r => (release = r))
    const first = watcher.tick()
    await flush()
    w.mtime = 2
    await poll(w, watcher)
    w.t += NET_MS
    await watcher.tick()
    expect(w.runs).toHaveLength(1)
    release()
    await first
  })

  it('submits the fixed text once, and notes it as its own', async () => {
    const { w, watcher } = world()
    await watcher.tick()
    await poll(w, watcher)
    w.mtime = 2
    await poll(w, watcher)
    expect(w.submits).toEqual([WAKE_UP])
    expect(w.own).toEqual([WAKE_UP])
  })

  it.each([
    ['parked', { parked: true }],
    ['a task is open', { task_open: true }],
    ['not the current session', { is_current: false, current_id: 's2' }],
    ['the inbox is empty', { untaken: 0 }],
  ])('does not submit when %s', async (_, snap) => {
    const { w, watcher } = world(snap)
    await watcher.tick()
    expect(w.submits).toEqual([])
  })

  it('does not submit while busy, and offers once the turn ends', async () => {
    const { w, watcher } = world({ untaken: 0 })
    await watcher.tick()
    watcher.onTurn(START)
    w.snap = { ...w.snap, untaken: 1, newest_seq: 1 }
    w.mtime = 2
    await poll(w, watcher)
    expect(w.reads).toBe(2)
    expect(w.submits).toEqual([])
    // gen does not move again: the end of the turn decides from what was read while busy.
    watcher.onTurn(END)
    await flush()
    expect(w.submits).toEqual([WAKE_UP])
  })

  it('ignores subagent turns', async () => {
    const { w, watcher } = world({ untaken: 0 })
    await watcher.tick()
    watcher.onTurn({ ...START, agentId: 'a1' })
    w.snap = { ...w.snap, untaken: 1, newest_seq: 1 }
    w.mtime = 2
    await poll(w, watcher)
    expect(w.submits).toEqual([WAKE_UP])
    // A subagent's end does not make a busy main loop idle.
    watcher.onTurn(START)
    w.snap = { ...w.snap, untaken: 2, newest_seq: 2 }
    w.mtime = 3
    watcher.onTurn({ ...END, agentId: 'a1' })
    await poll(w, watcher)
    expect(w.submits).toHaveLength(1)
  })

  it('offers once per inbox state', async () => {
    const { w, watcher } = world()
    await watcher.tick()
    expect(w.submits).toHaveLength(1)
    // The turn ends with no take_task: no second offer for the same state.
    await turn(watcher)
    w.mtime = 2
    await poll(w, watcher)
    expect(w.submits).toHaveLength(1)
    // A new task is a new state.
    w.snap = { ...w.snap, untaken: 4, newest_seq: 4 }
    w.mtime = 3
    await poll(w, watcher)
    expect(w.submits).toHaveLength(2)
    // A take_task (taken moves) is a new state.
    await turn(watcher)
    w.snap = { ...w.snap, untaken: 3, taken: 1 }
    w.mtime = 4
    await poll(w, watcher)
    expect(w.submits).toHaveLength(3)
  })

  it('offers the same state again after a park lifts', async () => {
    const { w, watcher } = world()
    await watcher.tick()
    expect(w.submits).toHaveLength(1)
    await turn(watcher)
    w.snap = { ...w.snap, parked: true }
    w.mtime = 2
    await poll(w, watcher)
    w.snap = { ...w.snap, parked: false }
    w.mtime = 3
    await poll(w, watcher)
    expect(w.submits).toHaveLength(2)
  })

  it('offers the same state again after a restart', async () => {
    const { w, watcher } = world()
    await watcher.tick()
    expect(w.submits).toHaveLength(1)
    const again = createChildWatcher(stubPorts(w), IDS, () => {})
    await again.tick()
    expect(w.submits).toHaveLength(2)
  })

  it('offers at once after a turn that took a task and called done', async () => {
    const { w, watcher } = world()
    await watcher.tick()
    expect(w.submits).toHaveLength(1)
    watcher.onTurn(START)
    w.snap = { ...w.snap, untaken: 2, taken: 1, task_open: false }
    w.mtime = 2
    watcher.onTurn({ ...END, calledTake: true, calledDone: true })
    await flush()
    expect(w.submits).toEqual([WAKE_UP, WAKE_UP])
  })

  it('re-reads before deciding after a turn that took a task', async () => {
    const { w, watcher } = world({ untaken: 0 })
    await watcher.tick()
    // A user turn: a task arrives and is taken within it; the cache holds the arrival, not the take.
    watcher.onTurn(START)
    w.snap = { ...w.snap, untaken: 1, newest_seq: 1 }
    w.mtime = 2
    await poll(w, watcher)
    // gen has not moved yet: only the take_task flag forces the re-read.
    w.snap = { ...w.snap, untaken: 0, taken: 1, task_open: true }
    watcher.onTurn({ ...END, calledTake: true })
    await flush()
    expect(w.submits).toEqual([])
    expect(w.reads).toBe(3)
  })

  it('does not loop: ten idle turn ends with an unchanged inbox give one submit', async () => {
    const { w, watcher } = world()
    await watcher.tick()
    for (let i = 0; i < 10; i++) {
      await turn(watcher)
      await poll(w, watcher)
    }
    w.t += NET_MS
    await watcher.tick()
    expect(w.submits).toEqual([WAKE_UP])
  })

  it('adds the card line to the next wake-up after the card changed, never alone', async () => {
    const { w, watcher } = world({ untaken: 0 })
    await watcher.tick()
    w.snap = { ...w.snap, card_version: 2 }
    w.mtime = 2
    await poll(w, watcher)
    expect(w.submits).toEqual([])
    w.snap = { ...w.snap, untaken: 1, newest_seq: 1 }
    w.mtime = 3
    await poll(w, watcher)
    expect(w.submits).toEqual([`${WAKE_UP}\n${CARD_CHANGED}`])
    expect(w.own).toEqual([`${WAKE_UP}\n${CARD_CHANGED}`])
    await turn(watcher)
    w.snap = { ...w.snap, untaken: 2, newest_seq: 2 }
    w.mtime = 4
    await poll(w, watcher)
    expect(w.submits.at(-1)).toBe(WAKE_UP)
  })

  it('sets the card baseline from the first snapshot', async () => {
    const { w, watcher } = world({ card_version: 9 })
    await watcher.tick()
    expect(w.submits).toEqual([WAKE_UP])
  })

  it('decides on the last good snapshot when a read fails, and reads again when gen moves', async () => {
    const { w } = world({ untaken: 0 })
    const p = stubPorts(w)
    let fail = false
    const watcher = createChildWatcher(
      { ...p, run: async (verb, input) => (fail ? { ok: false, error: 'crook: binary not found' } : p.run(verb, input)) },
      IDS,
      () => {},
    )
    await watcher.tick()
    watcher.onTurn(START)
    fail = true
    w.snap = { ...w.snap, untaken: 1, newest_seq: 1 }
    w.mtime = 2
    await poll(w, watcher)
    watcher.onTurn(END)
    await flush()
    expect(w.submits).toEqual([])
    fail = false
    w.mtime = 3
    await poll(w, watcher)
    expect(w.submits).toEqual([WAKE_UP])
  })

  it('polls every 2 s', () => {
    expect(POLL_MS).toBe(2000)
    expect(NET_MS).toBe(60_000)
  })
})

/** Ports over the same fake world, for a second watcher (a restart). */
function stubPorts(w: ReturnType<typeof world>['w']): Ports {
  return {
    run: async () => {
      w.reads++
      return { ok: true, ...w.snap }
    },
    stat: async () => (w.mtime === null ? null : { mtimeMs: w.mtime }),
    submit: async text => {
      w.submits.push(text)
    },
    status: () => {},
    every: () => {},
    now: () => w.t,
    env: () => undefined,
  }
}
