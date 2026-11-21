import { describe, expect, it } from 'vitest'

import { LIFT_FAILED, LOOP_REFUSED, type Prompt, filter, rescue } from './park.ts'
import type { Ports, VerbResult } from './ports.ts'

/** Fake ports: records runs, status lines and the order things happen in; `lift` answers `reply`. */
function world(reply: VerbResult = { ok: true, lifted: true }) {
  const w = {
    runs: [] as { verb: string; input: object }[],
    statuses: [] as (string | undefined)[],
    log: [] as string[],
    /** When set, `run` waits on it, so the lift stays in flight. */
    hold: undefined as Promise<void> | undefined,
    throwOnStatus: false,
  }
  const p: Ports = {
    async run(verb, input) {
      w.runs.push({ verb, input })
      if (w.hold !== undefined) await w.hold
      w.log.push(`run:${verb}`)
      return reply
    },
    stat: async () => null,
    submit: async () => undefined,
    status(text) {
      if (w.throwOnStatus) throw new Error('status broke')
      w.statuses.push(text)
    },
    every: () => undefined,
    now: () => 0,
    env: () => undefined,
  }
  const next = async (e: Prompt) => {
    w.log.push('next')
    return { passed: e }
  }
  return { w, p, next }
}

const sid = async () => 's1'
const typed = (kind: string, command?: string): Prompt =>
  command === undefined ? { origin: { kind } } : { origin: { kind }, command: { name: command } }

describe('filter', () => {
  it('composer lifts, and awaits the lift before next', async () => {
    const { w, p, next } = world()
    let release!: () => void
    w.hold = new Promise(r => (release = r))
    const e = typed('composer')
    const out = filter(p, sid, e, next)
    await Promise.resolve()
    await Promise.resolve()
    expect(w.runs).toEqual([{ verb: 'lift', input: { session_id: 's1', user_turn: true } }])
    expect(w.log).toEqual([])
    release()
    expect(await out).toEqual({ passed: e })
    expect(w.log).toEqual(['run:lift', 'next'])
    expect(w.statuses).toEqual([])
  })

  it('bridge lifts', async () => {
    const { w, p, next } = world()
    await filter(p, sid, typed('bridge'), next)
    expect(w.log).toEqual(['run:lift', 'next'])
  })

  it('a slash command from composer or bridge passes without lift', async () => {
    for (const kind of ['composer', 'bridge']) {
      const { w, p, next } = world()
      const e = typed(kind, 'compact')
      expect(await filter(p, sid, e, next)).toEqual({ passed: e })
      expect(w.runs).toEqual([])
    }
  })

  it('machine prompts pass unchanged without lift', async () => {
    for (const kind of ['scheduled-trigger', 'task-notification', 'peer', 'sdk', 'plugin', 'unclassified']) {
      const { w, p, next } = world()
      const e = typed(kind)
      const out = await filter(p, sid, e, next)
      expect(out).toEqual({ passed: e })
      expect((out as { passed: Prompt }).passed).toBe(e)
      expect(w.runs).toEqual([])
    }
  })

  it('/loop is dropped from any origin, with a status line and no lift', async () => {
    for (const kind of ['composer', 'bridge', 'scheduled-trigger', 'peer']) {
      const { w, p, next } = world()
      expect(await filter(p, sid, typed(kind, 'loop'), next)).toEqual({ drop: LOOP_REFUSED })
      expect(w.log).toEqual([])
      expect(w.runs).toEqual([])
      expect(w.statuses).toEqual(['crook: /loop is not allowed in a child'])
    }
  })

  it('a failed lift shows on the status line, and the prompt still passes', async () => {
    const { w, p, next } = world({ ok: false, error: 'crook: binary not found' })
    const e = typed('composer')
    expect(await filter(p, sid, e, next)).toEqual({ passed: e })
    expect(w.statuses).toEqual(['crook: park lift failed'])
    expect(LIFT_FAILED).toBe('crook: park lift failed')
  })

  it('throws when the session id cannot be read, leaving the rest to rescue', async () => {
    const { w, p, next } = world()
    const broken = async (): Promise<string> => {
      throw new Error('no id')
    }
    await expect(filter(p, broken, typed('composer'), next)).rejects.toThrow('no id')
    expect(w.log).toEqual([])
  })
})

describe('rescue (the .catch)', () => {
  it('throw path: when the filter threw before next, rescue still lifts and passes the composer prompt', async () => {
    const { w, p, next } = world({ ok: false, error: 'boom' })
    w.throwOnStatus = true
    const e = typed('composer')
    // The filter lifts, then its status line throws before `next`.
    await expect(filter(p, sid, e, next)).rejects.toThrow('status broke')
    w.throwOnStatus = false
    w.log = []
    w.runs = []
    let release!: () => void
    w.hold = new Promise(r => (release = r))
    const out = rescue(p, sid, e, next, false)
    await Promise.resolve()
    await Promise.resolve()
    expect(w.runs.map(r => r.verb)).toEqual(['lift'])
    expect(w.log).toEqual([])
    release()
    expect(await out).toEqual({ passed: e })
    expect(w.log).toEqual(['run:lift', 'next'])
  })

  it('lets the prompt through without a second lift when next was already called', async () => {
    const { w, p, next } = world()
    const e = typed('composer')
    expect(await rescue(p, sid, e, next, true)).toEqual({ passed: e })
    expect(w.runs).toEqual([])
  })

  it('passes a composer prompt even when the lift itself throws, and shows the failure', async () => {
    const { w, p, next } = world()
    const broken = async (): Promise<string> => {
      throw new Error('no id')
    }
    const e = typed('composer')
    expect(await rescue(p, broken, e, next, false)).toEqual({ passed: e })
    expect(w.statuses).toEqual([LIFT_FAILED])
  })

  it('passes a machine prompt without lift, and still drops /loop', async () => {
    const { w, p, next } = world()
    const e = typed('peer')
    expect(await rescue(p, sid, e, next, false)).toEqual({ passed: e })
    expect(await rescue(p, sid, typed('composer', 'loop'), next, false)).toEqual({ drop: LOOP_REFUSED })
    expect(w.runs).toEqual([])
  })

  it('never throws, even when the status line does', async () => {
    const { w, p, next } = world({ ok: false })
    w.throwOnStatus = true
    const e = typed('composer')
    expect(await rescue(p, sid, e, next, false)).toEqual({ passed: e })
    expect(await rescue(p, sid, typed('bridge', 'loop'), next, false)).toEqual({ drop: LOOP_REFUSED })
  })
})
