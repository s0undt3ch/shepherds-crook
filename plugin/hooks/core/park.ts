import type { Ports } from './ports.ts'

export const LIFT_FAILED = 'crook: park lift failed'
export const LOOP_REFUSED = 'crook: /loop is not allowed in a child'

const USER_KINDS = new Set(['composer', 'bridge'])

/** The parts of a child's submitted prompt the park filter reads. */
export interface Prompt {
  origin: { kind: string }
  command?: { name: string }
}

export type Drop = { drop: string }

/**
 * `/loop` is denied by settings too; this is the backstop, from any origin. Any other slash command
 * passes without lifting, checked before the origin. Only the user's own typing lifts the park:
 * machine prompts (a task's notification, a subagent's report) carry results, so they pass.
 */
function decide(e: Prompt): 'drop' | 'lift' | 'pass' {
  if (e.command?.name === 'loop') return 'drop'
  if (e.command !== undefined) return 'pass'
  return USER_KINDS.has(e.origin.kind) ? 'lift' : 'pass'
}

// Lift is a no-op in crook when the child is not parked, so it runs for every user prompt.
async function lift(p: Ports, sessionId: () => Promise<string>): Promise<void> {
  const r = await p.run('lift', { session_id: await sessionId(), user_turn: true })
  if (!r.ok) p.status(LIFT_FAILED)
}

/**
 * The child's `prompt.submit` filter. The lift is awaited before `next`, so the turn's first tool
 * call doesn't race the settings hook's park check.
 */
export async function filter<E extends Prompt, R>(
  p: Ports,
  sessionId: () => Promise<string>,
  e: E,
  next: (e: E) => Promise<R>,
): Promise<R | Drop> {
  const d = decide(e)
  if (d === 'drop') {
    p.status(LOOP_REFUSED)
    return { drop: LOOP_REFUSED }
  }
  if (d === 'lift') await lift(p, sessionId)
  return next(e)
}

/**
 * The filter's `.catch`: the same decision, but it never throws, so the user's answer still lifts
 * the park and gets through. When the filter had already called `next`, that call is replayed.
 */
export async function rescue<E extends Prompt, R>(
  p: Ports,
  sessionId: () => Promise<string>,
  e: E,
  next: (e: E) => Promise<R>,
  called: boolean,
): Promise<R | Drop> {
  if (called) return next(e)
  const d = decide(e)
  if (d === 'drop') {
    try {
      p.status(LOOP_REFUSED)
    } catch {
      // The drop matters more than the status line.
    }
    return { drop: LOOP_REFUSED }
  }
  if (d === 'lift') {
    try {
      await lift(p, sessionId)
    } catch {
      try {
        p.status(LIFT_FAILED)
      } catch {
        // Nothing more to tell; the prompt still goes through.
      }
    }
  }
  return next(e)
}
