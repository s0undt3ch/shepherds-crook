import { noteOwnSubmit } from './origin.ts'
import type { Ports, VerbResult } from './ports.ts'

export const WAKE_UP = 'crook: you have tasks waiting. Call take_task.'
export const CARD_CHANGED = 'crook: the project card changed: call status()'
export const POLL_MS = 2000
export const NET_MS = 60_000

export interface WatcherIds {
  /** Asked on every read: `/clear` moves the process to a new session id with no new `session.start`. */
  sessionId: () => Promise<string>
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

/** What differs between the child's loop and the orchestrator's: the snapshot and when to offer. */
export interface Role<S> {
  parse(r: VerbResult): S | undefined
  cardVersion(s: S): number
  /** Whether the snapshot calls for an offer, apart from the loop's own idle, stale and failed gates. */
  ready(s: S): boolean
  /** One offer goes out per distinct key. */
  key(s: S): string
  text(s: S): string
  /** True when the move from `prev` to `s` makes the last offered state offerable again. */
  reoffer?(prev: S, s: S): boolean
  /** Called with each good snapshot. */
  onRead?(s: S): void
  /** Called as an offer goes out; what it returns is added to the next snapshot call. */
  onOffer?(s: S): object | undefined
}

const childRole: Role<ChildSnap> = {
  parse: parseSnap,
  cardVersion: s => s.card_version,
  ready: s => !s.parked && s.is_current && s.untaken > 0 && !s.task_open,
  key: s => `${s.newest_seq}:${s.taken}`,
  text: () => WAKE_UP,
  reoffer: (prev, s) => prev.parked && !s.parked,
}

/**
 * The inbox watcher loop. Each tick stats `gen` and reads a snapshot only when its mtime moved
 * (or `NET_MS` passed); every tick then decides from the last snapshot. It submits one fixed-text
 * wake-up per state (the role's key), and only when idle and the role says the snapshot is ready.
 */
export function createWatcher<S extends { card_version: number }>(
  p: Ports,
  ids: WatcherIds,
  role: Role<S>,
  noteOwn: (text: string) => void = noteOwnSubmit,
) {
  let idle = true
  let inFlight = false
  let read = false
  let lastMtime: number | null = null
  let lastReadAt = 0
  // Set when a turn took a task or called done: the cached snapshot predates that, so wait for a re-read.
  let stale = false
  let snap: S | undefined
  // Set by a failed read: the cache may be older than the state, so nothing is decided until a good read.
  let failed = false
  let offered: string | undefined
  let cardSeen: number | undefined
  // Extra verb arguments from the last offer, sent on the next read and dropped once a read succeeds.
  let pending: object | undefined

  async function readSnap(mtime: number | null): Promise<void> {
    // The mtime is the one taken before the read, so a commit landing during it is seen next tick.
    lastMtime = mtime
    lastReadAt = p.now()
    read = true
    stale = false
    failed = true
    const sent = pending
    const input = { session_id: await ids.sessionId(), user_turn: false, ...sent }
    const s = role.parse(await p.run('snapshot', input))
    if (s === undefined) return
    failed = false
    if (sent !== undefined && pending === sent) pending = undefined
    if (snap !== undefined && role.reoffer?.(snap, s) === true) offered = undefined
    cardSeen ??= role.cardVersion(s)
    snap = s
    role.onRead?.(s)
  }

  function decide(): void {
    const s = snap
    if (s === undefined || stale || failed || !idle || !role.ready(s)) return
    const key = role.key(s)
    if (key === offered) return
    offered = key
    const base = role.text(s)
    const text = role.cardVersion(s) === cardSeen ? base : `${base}\n${CARD_CHANGED}`
    cardSeen = role.cardVersion(s)
    // The mod's own prompt.submit hook never hears this submit, so the origin is queued here.
    noteOwn(text)
    // Busy until the wake-up's turn ends, so a change before its turn starts queues no second one.
    idle = false
    const extra = role.onOffer?.(s)
    // Not awaited: the submit may resolve only once its turn starts, and reads must go on meanwhile.
    void p.submit(text).then(
      () => {
        // Sent after the submit, so a submit that fails leaves the work unmarked.
        if (extra === undefined) return
        pending = extra
        stale = true
        void tick()
      },
      () => {
        idle = true
      },
    )
  }

  async function tick(): Promise<void> {
    if (inFlight) return
    inFlight = true
    try {
      const mtime = (await p.stat(ids.gen))?.mtimeMs ?? null
      if (!read || stale || mtime !== lastMtime || p.now() - lastReadAt >= NET_MS) await readSnap(mtime)
      decide()
    } catch {
      // A failed stat decides nothing this tick; a failed read blocks decisions until a good one.
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

export function createChildWatcher(p: Ports, ids: WatcherIds, noteOwn: (text: string) => void = noteOwnSubmit) {
  return createWatcher(p, ids, childRole, noteOwn)
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
