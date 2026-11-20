import type { VerbResult } from './ports.ts'

/** Exit code the adapter reports when the process never started. */
export const BINARY_NOT_RUN = 127

const BAD_REPLY: VerbResult = { ok: false, error: 'crook: bad reply' }
const NOT_FOUND: VerbResult = { ok: false, error: 'crook: binary not found' }

// 126 (found, not executable) and 127 (not found) come from a shell; negative is a spawn failure.
const binaryDidNotRun = (exitCode: number): boolean => exitCode < 0 || exitCode === 126 || exitCode === 127

/** Turns a `crook mod <verb>` exit code and stdout into a `VerbResult`; never throws. */
export function parseVerb(exitCode: number, stdout: string): VerbResult {
  let parsed: unknown
  try {
    parsed = JSON.parse(stdout)
  } catch {
    return binaryDidNotRun(exitCode) ? { ...NOT_FOUND } : { ...BAD_REPLY }
  }
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) return { ...BAD_REPLY }
  const reply = parsed as Record<string, unknown>
  if (typeof reply.ok !== 'boolean') return { ...BAD_REPLY }
  if (!reply.ok && typeof reply.error !== 'string') return { ...BAD_REPLY }
  return reply as VerbResult
}

/** The reply for a `process.run` that rejected: it timed out, or the command could not start. */
export function parseRejected(elapsedMs: number, timeoutMs: number): VerbResult {
  return elapsedMs >= timeoutMs ? { ok: false, error: 'crook: timed out' } : parseVerb(BINARY_NOT_RUN, '')
}

/** `$.process.run`'s timeout when the caller passes none. */
export const DEFAULT_TIMEOUT_MS = 30_000
