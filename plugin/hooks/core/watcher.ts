import { noteOwnSubmit } from './origin.ts'
import type { Ports, VerbResult } from './ports.ts'

export const WAKE_UP = 'crook: you have tasks waiting. Call take_task.'
export const CARD_CHANGED = 'crook: the project card changed: call status()'
export const POLL_MS = 2000
export const NET_MS = 60_000

export interface WatcherIds {
  sessionId: string
  /** The `gen` file `ping` names; crook rewrites it after each coordination commit. */
  gen: string
}

export interface TurnEvent {
  kind: 'start' | 'complete'
  agentId?: string
  calledTake: boolean
  calledDone: boolean
}

interface ChildSnap {
  untaken: number
  taken: number
  newest_seq: number
  task_open: boolean
  parked: boolean
  is_current: boolean
  card_version: number
}

function parseSnap(r: VerbResult): ChildSnap | undefined {
  if (!r.ok) return undefined
  const nums = ['untaken', 'taken', 'newest_seq', 'card_version'] as const
  const flags = ['task_open', 'parked', 'is_current'] as const
  if (!nums.every(k => typeof r[k] === 'number') || !flags.every(k => typeof r[k] === 'boolean')) return undefined
  return r as unknown as ChildSnap
}

/**
 * The child's inbox watcher. Each tick stats `gen` and reads a snapshot only when its mtime moved
 * (or `NET_MS` passed); every tick then decides from the last snapshot. It submits one fixed-text
 * wake-up per inbox state (newest seq plus taken count), and only when idle, not parked, current,
 * with untaken tasks and none open.
 */
export function createChildWatcher(p: Ports, ids: WatcherIds, noteOwn: (text: string) => void = noteOwnSubmit) {
  let idle = true
  let inFlight = false
  let read = false
  let lastMtime: number | null = null
  let lastReadAt = 0
  // Set when a turn took a task or called done: the cached snapshot predates that, so wait for a re-read.
  let stale = false
  let snap: ChildSnap | undefined
  let offered: string | undefined
  let cardSeen: number | undefined

  async function readSnap(mtime: number | null): Promise<void> {
    // The mtime is the one taken before the read, so a commit landing during it is seen next tick.
    lastMtime = mtime
    lastReadAt = p.now()
    read = true
    stale = false
    const s = parseSnap(await p.run('snapshot', { session_id: ids.sessionId, user_turn: false }))
    if (s === undefined) return
    if (snap?.parked === true && !s.parked) offered = undefined
    cardSeen ??= s.card_version
    snap = s
  }

  function decide(): void {
    const s = snap
    if (s === undefined || stale || !idle) return
    if (s.parked || !s.is_current || s.untaken <= 0 || s.task_open) return
    const key = `${s.newest_seq}:${s.taken}`
    if (key === offered) return
    offered = key
    const text = s.card_version === cardSeen ? WAKE_UP : `${WAKE_UP}\n${CARD_CHANGED}`
    cardSeen = s.card_version
    // The mod's own prompt.submit hook never hears this submit, so the origin is queued here.
    noteOwn(text)
    // Not awaited: the submit may resolve only once its turn starts, and reads must go on meanwhile.
    void p.submit(text).catch(() => {})
  }

  async function tick(): Promise<void> {
    if (inFlight) return
    inFlight = true
    try {
      const mtime = (await p.stat(ids.gen))?.mtimeMs ?? null
      if (!read || stale || mtime !== lastMtime || p.now() - lastReadAt >= NET_MS) await readSnap(mtime)
      decide()
    } catch {
      // A failed stat or read leaves the last snapshot; the next tick tries again.
    } finally {
      inFlight = false
    }
  }

  return {
    tick,
    onTurn(e: TurnEvent): void {
      if (e.agentId !== undefined) return
      if (e.kind === 'start') {
        idle = false
        return
      }
      idle = true
      if (e.calledTake || e.calledDone) stale = true
      void tick()
    },
  }
}

let shared: ReturnType<typeof createChildWatcher> | undefined

/** Starts the watcher; a second start (a new session) replaces its state but keeps one interval. */
export function startChildWatcher(p: Ports, ids: WatcherIds): void {
  const first = shared === undefined
  shared = createChildWatcher(p, ids)
  if (first) p.every(POLL_MS, () => void shared?.tick())
}

export function onTurn(e: TurnEvent): void {
  shared?.onTurn(e)
}
