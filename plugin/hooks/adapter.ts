import type { EngineInterface } from 'claude-code'

import type { Ports, VerbResult } from './core/ports.ts'
import { BINARY_NOT_RUN, parseVerb } from './core/verb.ts'

/**
 * Builds the ports from `$`. `$.env.get` is async and takes a literal name, so
 * the names a mod reads are fetched here once; `env` then answers from that
 * snapshot, and `run` waits for it before it needs `CROOK_BIN`.
 */
export function ports($: EngineInterface): Ports {
  const cache = new Map<string, string>()
  const remember = async (name: string, value: Promise<string | undefined>): Promise<void> => {
    const v = await value.catch(() => undefined)
    if (v !== undefined) cache.set(name, v)
  }
  const loaded = Promise.all([
    remember('CROOK_BIN', $.env.get('CROOK_BIN')),
    remember('CROOK_ROLE', $.env.get('CROOK_ROLE')),
    remember('CROOK_PROJECT', $.env.get('CROOK_PROJECT')),
    remember('CROOK_CHILD', $.env.get('CROOK_CHILD')),
  ])

  return {
    async run(verb: string, input: object, timeoutMs?: number): Promise<VerbResult> {
      await loaded
      const bin = cache.get('CROOK_BIN')
      if (bin === undefined || bin === '') return parseVerb(BINARY_NOT_RUN, '')
      try {
        const r = await $.process.run([bin, 'mod', verb], { stdin: JSON.stringify(input), timeoutMs })
        return parseVerb(r.exitCode, r.stdout)
      } catch {
        // `process.run` rejects when the command cannot start, or outlives the timeout.
        return parseVerb(BINARY_NOT_RUN, '')
      }
    },
    async stat(path: string) {
      const s = await $.fs.stat(path).catch(() => undefined)
      return s === undefined ? null : { mtimeMs: s.mtimeMs }
    },
    async submit(text: string) {
      await $.prompt.submit({ text })
    },
    status(text: string | undefined) {
      $.ui.status(text)
    },
    every(ms: number, fn: () => void) {
      $.clock.every(ms, fn)
    },
    now: () => Date.now(),
    env: (name: string) => cache.get(name),
  }
}
