import { noteOwnSubmit } from './origin.ts'
import type { Ports, VerbResult } from './ports.ts'
import { POLL_MS, createWatcher } from './watcher.ts'
import type { Role, TurnEvent, WatcherIds } from './watcher.ts'

export const DONE_WAKE_UP = 'crook: children are done. Call status().'
export const STALE_NUDGE = 'the card is stale: move Where it stands on, keeping what is still true'
/** Wake-ups with no typed (`composer` or `bridge`) prompt between them, after which the orchestrator stops waking. */
export const MAX_MACHINE_TURNS = 5

const USER_KINDS = new Set(['composer', 'bridge'])

interface OrchSnap {
  done_unnotified: { child: string; done_seq: number }[]
  waiting: number
  working: number
  done: number
  untaken: Record<string, number>
  card_stale: boolean
  card_version: number
  is_current: boolean
}

function parseSnap(r: VerbResult): OrchSnap | undefined {
  if (!r.ok) return undefined
  const nums = ['waiting', 'working', 'done', 'card_version'] as const
  const flags = ['card_stale', 'is_current'] as const
  if (!nums.every(k => typeof r[k] === 'number') || !flags.every(k => typeof r[k] === 'boolean')) return undefined
  const { done_unnotified: d, untaken: u } = r
  if (!Array.isArray(d) || u === null || typeof u !== 'object' || Array.isArray(u)) return undefined
  const noteOk = (n: unknown): boolean => {
    const x = n as { child?: unknown; done_seq?: unknown } | null
    return typeof x?.child === 'string' && typeof x.done_seq === 'number'
  }
  if (!d.every(noteOk) || !Object.values(u).every(n => typeof n === 'number')) return undefined
  return r as unknown as OrchSnap
}

/** The status line, from a snapshot alone. */
export function renderStatus(s: OrchSnap): string {
  const parts = [`crook: ${s.waiting} waiting on you · ${s.working} working`]
  if (s.card_stale) parts.push('card stale')
  for (const [child, n] of Object.entries(s.untaken).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))) {
    if (n > 0) parts.push(`${child}: ${n} untaken`)
  }
  return parts.join(' · ')
}

/**
 * The orchestrator's watcher: the child's loop under another role. It offers one fixed-text wake-up per
 * set of unannounced `done`s, at most `MAX_MACHINE_TURNS` in a row, and acks what each announced.
 */
export function createOrchestratorWatcher(p: Ports, ids: WatcherIds, noteOwn: (text: string) => void = noteOwnSubmit) {
  let machineTurns = 0
  let last: OrchSnap | undefined
  let shown: string | undefined

  const role: Role<OrchSnap> = {
    parse: parseSnap,
    cardVersion: s => s.card_version,
    ready: s => s.is_current && s.done_unnotified.length > 0 && machineTurns < MAX_MACHINE_TURNS,
    key: s => s.done_unnotified.map(d => `${d.child}:${d.done_seq}`).join(','),
    text: () => DONE_WAKE_UP,
    onRead(s) {
      last = s
      const text = renderStatus(s)
      if (text === shown) return
      shown = text
      p.status(text)
    },
    onOffer(s) {
      machineTurns++
      return { ack: Object.fromEntries(s.done_unnotified.map(d => [d.child, d.done_seq])) }
    },
  }
  const watcher = createWatcher(p, ids, role, noteOwn)

  return {
    ...watcher,
    /** The status line from the last snapshot read; never runs a verb. */
    line: (): string | undefined => (last === undefined ? undefined : renderStatus(last)),
    /** A typed prompt (`composer` or `bridge`): the user is back, so wake-ups may go on. */
    onTypedPrompt(): void {
      machineTurns = 0
    },
  }
}

let shared: ReturnType<typeof createOrchestratorWatcher> | undefined

/** Starts the watcher; a second start (a new session) replaces its state but keeps one interval. */
export function startOrchestratorWatcher(p: Ports, ids: WatcherIds): void {
  const first = shared === undefined
  shared = createOrchestratorWatcher(p, ids)
  if (first) p.every(POLL_MS, () => void shared?.tick())
}

export function onOrchestratorTurn(e: TurnEvent): void {
  shared?.onTurn(e)
}

export function onTypedPrompt(): void {
  shared?.onTypedPrompt()
}

interface Prompt {
  text: string
  origin: { kind: string }
  command?: unknown
  context?: readonly string[]
}

/**
 * The orchestrator's `prompt.submit` filter. On a prompt the user typed it attaches `orch-context`'s
 * lines, and the stale nudge when the card is overdue, after any context already there. Machine prompts
 * and slash commands pass untouched; a failed verb passes the prompt on without context.
 */
export async function filter<E extends Prompt, R>(
  p: Ports,
  sessionId: () => Promise<string>,
  e: E,
  next: (e: E) => Promise<R>,
): Promise<R> {
  const typed = USER_KINDS.has(e.origin.kind)
  if (typed) onTypedPrompt()
  if (e.command !== undefined || !typed) return next(e)
  let add: string[] = []
  try {
    const r = await p.run('orch-context', { session_id: await sessionId(), user_turn: true })
    const lines = Array.isArray(r.lines) ? r.lines.filter((l): l is string => typeof l === 'string') : []
    if (r.ok) add = [...lines, ...(r.stale === true ? [STALE_NUDGE] : [])]
  } catch {
    // No context beats a lost prompt.
  }
  return next(add.length === 0 ? e : { ...e, context: [...(e.context ?? []), ...add] })
}
