import { describe, expect, it } from 'vitest'

import { filter, createOrchestratorWatcher, DONE_WAKE_UP, STALE_NUDGE, renderStatus } from './orchestrator.ts'
import type { Ports, VerbResult } from './ports.ts'
import { CARD_CHANGED, NET_MS, POLL_MS } from './watcher.ts'

const IDS = { sessionId: async () => 's1', gen: '/d/crook/p/gen' }
const START = { kind: 'start', calledTake: false, calledDone: false } as const
const END = { kind: 'complete', calledTake: false, calledDone: false } as const

type Snap = {
  done_unnotified: { child: string; done_seq: number }[]
  waiting: number
  working: number
  done: number
  untaken: Record<string, number>
  card_stale: boolean
  card_version: number
  is_current: boolean
}

const QUIET: Snap = {
  done_unnotified: [],
  waiting: 1,
  working: 2,
  done: 0,
  untaken: {},
  card_stale: false,
  card_version: 1,
  is_current: true,
}

function world(snap: Partial<Snap> = {}) {
  const w = {
    t: 0,
    mtime: 1,
    snap: { ...QUIET, ...snap } as Snap,
    runs: [] as { verb: string; input: Record<string, unknown> }[],
    submits: [] as string[],
    own: [] as (string | undefined)[],
    statuses: [] as (string | undefined)[],
    /** What an ack makes of `done_unnotified`, as crook does: marks exactly those seqs notified. */
    acked: {} as Record<string, number>,
  }
  const p: Ports = {
    async run(verb, input): Promise<VerbResult> {
      w.runs.push({ verb, input: input as Record<string, unknown> })
      if (verb === 'snapshot') {
        const ack = (input as { ack?: Record<string, number> }).ack ?? {}
        Object.assign(w.acked, ack)
        const out = { ...w.snap, done_unnotified: w.snap.done_unnotified.filter(d => d.done_seq > (w.acked[d.child] ?? 0)) }
        return { ok: true, ...out }
      }
      return { ok: true, lines: ['c1 is done'], stale: w.snap.card_stale }
    },
    stat: async () => ({ mtimeMs: w.mtime }),
    submit: async text => {
      w.submits.push(text)
    },
    status: t => {
      w.statuses.push(t)
    },
    every: () => {},
    now: () => w.t,
    env: () => undefined,
  }
  const watcher = createOrchestratorWatcher(p, IDS, text => w.own.push(text))
  return { w, p, watcher }
}

async function flush(): Promise<void> {
  for (let i = 0; i < 20; i++) await Promise.resolve()
}

async function poll(w: { t: number }, watcher: { tick(): Promise<void> }) {
  w.t += POLL_MS
  await watcher.tick()
}

async function turn(watcher: ReturnType<typeof world>['watcher']) {
  watcher.onTurn(START)
  watcher.onTurn(END)
  await flush()
}

const snapshots = (w: ReturnType<typeof world>['w']) => w.runs.filter(r => r.verb === 'snapshot')
const done = (...pairs: [string, number][]) => pairs.map(([child, done_seq]) => ({ child, done_seq }))

describe('orchestrator watcher', () => {
  it('reads snapshots with no ack and a machine turn marker', async () => {
    const { w, watcher } = world()
    await watcher.tick()
    expect(w.runs[0]).toEqual({ verb: 'snapshot', input: { session_id: 's1', user_turn: false } })
    expect(w.submits).toEqual([])
  })

  it('wakes once after idle for two children done while busy, then acks what it announced', async () => {
    const { w, watcher } = world()
    await watcher.tick()
    watcher.onTurn(START)
    w.snap = { ...w.snap, done_unnotified: done(['c1', 4], ['c2', 7]) }
    w.mtime = 2
    await poll(w, watcher)
    expect(w.submits).toEqual([])
    watcher.onTurn(END)
    await flush()
    expect(w.submits).toEqual([DONE_WAKE_UP])
    expect(w.own).toEqual([DONE_WAKE_UP])
    const last = snapshots(w).at(-1)
    expect(last?.input).toEqual({ session_id: 's1', user_turn: false, ack: { c1: 4, c2: 7 } })
    // The ack is sent once.
    await poll(w, watcher)
    w.t += NET_MS
    await watcher.tick()
    expect(snapshots(w).filter(r => 'ack' in r.input)).toHaveLength(1)
    expect(w.submits).toHaveLength(1)
  })

  it('announces a child that finishes again before the ack lands', async () => {
    const { w, watcher } = world({ done_unnotified: done(['c1', 4]) })
    await watcher.tick()
    expect(w.submits).toEqual([DONE_WAKE_UP])
    // c1 finishes again (seq 6) before the ack read; the ack still carries only 4.
    w.snap = { ...w.snap, done_unnotified: done(['c1', 6]) }
    await flush()
    await watcher.tick()
    expect(snapshots(w).find(r => 'ack' in r.input)?.input.ack).toEqual({ c1: 4 })
    await turn(watcher)
    await poll(w, watcher)
    expect(w.submits).toEqual([DONE_WAKE_UP, DONE_WAKE_UP])
    expect(snapshots(w).filter(r => 'ack' in r.input).at(-1)?.input.ack).toEqual({ c1: 6 })
  })

  it('gives no submit after a reload when nothing is unnotified', async () => {
    const { w, watcher } = world({ done: 3 })
    await watcher.tick()
    await turn(watcher)
    expect(w.submits).toEqual([])
  })

  it('does not wake a session that is not current', async () => {
    const { w, watcher } = world({ done_unnotified: done(['c1', 1]), is_current: false })
    await watcher.tick()
    expect(w.submits).toEqual([])
  })

  it('adds the card line to the next wake-up only', async () => {
    const { w, watcher } = world()
    await watcher.tick()
    w.snap = { ...w.snap, card_version: 2 }
    w.mtime = 2
    await poll(w, watcher)
    expect(w.submits).toEqual([])
    w.snap = { ...w.snap, done_unnotified: done(['c1', 1]) }
    w.mtime = 3
    await poll(w, watcher)
    expect(w.submits).toEqual([`${DONE_WAKE_UP}\n${CARD_CHANGED}`])
    await turn(watcher)
    w.snap = { ...w.snap, done_unnotified: done(['c1', 2]) }
    w.mtime = 4
    await poll(w, watcher)
    expect(w.submits.at(-1)).toBe(DONE_WAKE_UP)
  })

  it('stops after five machine turns in a row, and a composer prompt resets the count', async () => {
    const { w, watcher } = world()
    await watcher.tick()
    for (let i = 1; i <= 5; i++) {
      w.snap = { ...w.snap, done_unnotified: done(['c1', i]) }
      w.mtime = i + 1
      await poll(w, watcher)
      await turn(watcher)
    }
    expect(w.submits).toHaveLength(5)
    w.snap = { ...w.snap, done_unnotified: done(['c1', 6]) }
    w.mtime = 20
    await poll(w, watcher)
    await turn(watcher)
    expect(w.submits).toHaveLength(5)
    watcher.onComposer()
    await poll(w, watcher)
    expect(w.submits).toHaveLength(6)
  })

  it('does not decide after a failed read until a good one', async () => {
    const { w, p } = world()
    let fail = false
    const watcher = createOrchestratorWatcher(
      { ...p, run: async (verb, input) => (fail ? { ok: false, error: 'x' } : p.run(verb, input)) },
      IDS,
      () => {},
    )
    await watcher.tick()
    fail = true
    w.snap = { ...w.snap, done_unnotified: done(['c1', 1]) }
    w.mtime = 2
    await poll(w, watcher)
    expect(w.submits).toEqual([])
    fail = false
    w.mtime = 3
    await poll(w, watcher)
    expect(w.submits).toEqual([DONE_WAKE_UP])
  })
})

describe('status line', () => {
  it('shows the base form from the cache, and never runs a verb to render', async () => {
    const { w, watcher } = world()
    await watcher.tick()
    expect(watcher.line()).toBe('crook: 1 waiting on you · 2 working')
    expect(w.statuses.at(-1)).toBe('crook: 1 waiting on you · 2 working')
    const runs = w.runs.length
    watcher.line()
    expect(w.runs).toHaveLength(runs)
  })

  it('adds card stale, then each child with untaken tasks in name order', () => {
    const base = { ...QUIET, card_stale: true, untaken: { c2: 1, c1: 3, c3: 0 } }
    expect(renderStatus(base)).toBe('crook: 1 waiting on you · 2 working · card stale · c1: 3 untaken · c2: 1 untaken')
    expect(renderStatus({ ...QUIET, untaken: { c1: 3 } })).toBe('crook: 1 waiting on you · 2 working · c1: 3 untaken')
  })

  it('keeps the last good line when a read fails', async () => {
    const { w, p } = world()
    let fail = false
    const watcher = createOrchestratorWatcher({ ...p, run: async (v, i) => (fail ? { ok: false } : p.run(v, i)) }, IDS, () => {})
    await watcher.tick()
    fail = true
    w.mtime = 2
    await poll(w, watcher)
    expect(watcher.line()).toBe('crook: 1 waiting on you · 2 working')
  })

  it('is undefined before the first read', () => {
    expect(world().watcher.line()).toBeUndefined()
  })
})

describe('context on a user turn', () => {
  const prompt = (kind: string, extra: object = {}) => ({ text: 'hi', origin: { kind }, ...extra })

  async function run(kind: string, stale = false, reply?: VerbResult, extra: object = {}) {
    const { w, p } = world({ card_stale: stale })
    const seen: unknown[] = []
    const p2: Ports = reply === undefined ? p : { ...p, run: async () => reply }
    const r = await filter(p2, async () => 's1', prompt(kind, extra), async e => {
      seen.push(e)
      return { text: e.text }
    })
    return { w, seen: seen[0] as { context?: string[] }, r }
  }

  it('attaches the verb lines on a composer prompt', async () => {
    const { w, seen } = await run('composer')
    expect(w.runs).toEqual([{ verb: 'orch-context', input: { session_id: 's1', user_turn: true } }])
    expect(seen.context).toEqual(['c1 is done'])
  })

  it('adds the stale nudge when stale, after any existing context', async () => {
    const { seen } = await run('composer', true, { ok: true, lines: ['a'], stale: true })
    expect(seen.context).toEqual(['a', STALE_NUDGE])
    expect(STALE_NUDGE).toBe('the card is stale: move Where it stands on, keeping what is still true')
  })

  it('keeps context another hook attached', async () => {
    const { seen } = await run('bridge', false, { ok: true, lines: ['a'], stale: false }, { context: ['x'] })
    expect(seen.context).toEqual(['x', 'a'])
  })

  it('gives a scheduled trigger and a slash command no context and no verb run', async () => {
    const a = await run('scheduled-trigger')
    expect(a.w.runs).toEqual([])
    expect(a.seen.context).toBeUndefined()
    const b = await run('composer', false, undefined, { command: { name: 'compact' } })
    expect(b.w.runs).toEqual([])
  })

  it('passes the prompt through unchanged when the verb fails', async () => {
    const { seen } = await run('composer', false, { ok: false, error: 'boom' })
    expect(seen.context).toBeUndefined()
  })

  it('passes it through when nothing is to add', async () => {
    const { seen } = await run('composer', false, { ok: true, lines: [], stale: false })
    expect(seen.context).toBeUndefined()
  })
})
