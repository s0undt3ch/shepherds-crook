import { expect, mock, test } from 'claude-code/testing'
import type { On } from 'claude-code'

type Run = { argv: readonly string[]; stdin: unknown }

/** Answers the engine calls the mod makes, below the mod; `reply` is what the fake binary prints. */
function world(
  on: On,
  env: Record<string, string>,
  reply: (verb: string, input: Record<string, unknown>) => object = () => ({ ok: true }),
) {
  const runs: Run[] = []
  const registered: { name: string; isDeferred?: unknown; inputSchema: unknown }[] = []
  const commands: string[] = []
  const submits: { text: string; origin: unknown; context?: readonly string[] }[] = []
  const statuses: (string | undefined)[] = []
  // `order` interleaves verb runs and prompts entering; `failIds` makes that many `session.id` reads throw.
  const order: string[] = []
  const timeouts: Record<string, unknown> = {}
  const session = { id: 's1', failIds: 0 }
  mock.env(on, env)
  on('session.id', () => {
    if (session.failIds > 0) {
      session.failIds--
      throw new Error('id unavailable')
    }
    return { value: session.id }
  })
  on('ui.status', (_, e) => {
    statuses.push(e.text)
    return { value: undefined }
  })
  on('session.start', () => ({ cwd: '/w' }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('prompt.submit', (_, e) => {
    submits.push({ text: e.text, origin: e.origin, context: e.context })
    order.push(`enter:${e.text}`)
    return { text: e.text }
  })
  on('tool.register', (_, e) => {
    registered.push(e)
    return { value: { tool: `mcp__crook__${e.name}` } }
  })
  on('command.register', (_, e) => {
    commands.push(e.name)
    return { value: { command: e.name } }
  })
  on('process.run', (_, e) => {
    const stdin = JSON.parse(String(e.init?.stdin ?? '{}')) as Record<string, unknown>
    runs.push({ argv: e.argv, stdin })
    const verb = e.argv[2] ?? ''
    timeouts[verb] = e.init?.timeoutMs
    order.push(`run:${verb}`)
    return { value: { exitCode: 0, stdout: JSON.stringify(reply(verb, stdin)), stderr: '', isStdoutTruncated: false, isStderrTruncated: false } }
  })
  return { runs, registered, commands, submits, session, statuses, order, timeouts }
}

const PRES = { isFullscreen: false, columns: 80 }
const START = { cwd: '/w', surface: null, isInteractive: false } as const
const CHILD = { CROOK_BIN: '/bin/crook', CROOK_ROLE: 'child', CROOK_PROJECT: 'p', CROOK_CHILD: 'c1' }
const ORCH = { CROOK_BIN: '/bin/crook', CROOK_ROLE: 'orchestrator', CROOK_PROJECT: 'p' }
const ping = (role: string) => (verb: string) => (verb === 'ping' ? { ok: true, role } : { ok: true })

test('registers child tools only', async ($, on) => {
  const w = world(on, CHILD, ping('child'))
  await $.session.start(START)
  expect(w.registered.map(t => t.name).sort()).toEqual(['done', 'need', 'status', 'take_task', 'worktree'])
  expect(w.registered.every(t => t.isDeferred === false)).toBe(true)
})

test('registers orchestrator tools only', async ($, on) => {
  const w = world(on, ORCH, ping('orchestrator'))
  await $.session.start(START)
  expect(w.registered.map(t => t.name).sort()).toEqual(
    [
      'project_add_link',
      'project_add_notes',
      'project_add_repo',
      'project_add_ticket',
      'project_set_status',
      'project_set_what_why',
      'spawn',
      'status',
      'stop',
      'task',
    ].sort(),
  )
  expect(w.registered.every(t => t.isDeferred === false)).toBe(true)
  expect(w.commands).toEqual([])
})

test('registers by the environment role when ping fails, and every tool says binary not found', async ($, on) => {
  const w = world(on, { CROOK_ROLE: 'child', CROOK_PROJECT: 'p', CROOK_CHILD: 'c1' })
  await $.session.start(START)
  expect(w.registered).toHaveLength(5)
  const r = await $.tool.run({ tool: 'mcp__crook__need', tool_use_id: 't1', question: 'q' })
  expect(r.deny).toBe('crook: binary not found')
  expect(w.runs).toEqual([])
})

test('registers nothing when ping fails and the role is unknown', async ($, on) => {
  const w = world(on, {})
  await $.session.start(START)
  expect(w.registered).toEqual([])
})

test('serves at tool.run', async ($, on) => {
  const w = world(on, CHILD, (verb, input) => (verb === 'need' ? { ok: true, message: `asked: ${String(input.question)}` } : ping('child')(verb)))
  await $.session.start(START)
  const r = await $.tool.run({ tool: 'mcp__crook__need', tool_use_id: 't1', question: 'which db?' })
  expect(r.result).toBe('asked: which db?')
  const need = w.runs.find(x => x.argv[2] === 'need')
  expect(need?.argv).toEqual(['/bin/crook', 'mod', 'need'])
  expect(need?.stdin).toEqual({ session_id: 's1', user_turn: false, question: 'which db?' })
})

test('spawn runs with a 120 s timeout and the other verbs with the default', async ($, on) => {
  const w = world(on, ORCH, ping('orchestrator'))
  await $.session.start(START)
  await $.tool.run({ tool: 'mcp__crook__spawn', tool_use_id: 't1', child: 'c1', repo: '/r', brief_text: 'b' })
  await $.tool.run({ tool: 'mcp__crook__task', tool_use_id: 't2', child: 'c1', text: 'go' })
  await $.tool.run({ tool: 'mcp__crook__stop', tool_use_id: 't3', child: 'c1' })
  expect(w.timeouts.spawn).toBe(120000)
  expect(w.timeouts.task).toBeUndefined()
  expect(w.timeouts.stop).toBeUndefined()
})

test('a model call is served at tool.run, not by a tool.call answer', async ($, on) => {
  const w = world(on, CHILD, ping('child'))
  const seen: string[] = []
  // A tool.call answer would skip the permission check; the mod must not give one.
  on('tool.call', { tool: 'mcp__crook__need' }, () => {
    seen.push('tool.call')
    return { result: 'answered at tool.call' }
  })
  await $.session.start(START)
  const r = await $.tool.call({ tool: 'mcp__crook__need', question: 'q' })
  expect(r.result).toBe('ok')
  expect(seen).toEqual([])
  expect(w.runs.some(x => x.argv[2] === 'need')).toBe(true)
})

test('a verb error comes back as a denial', async ($, on) => {
  world(on, ORCH, (verb: string) => (verb === 'ping' ? { ok: true, role: 'orchestrator' } : { ok: false, error: "only on a turn the user started; don't retry" }))
  await $.session.start(START)
  const r = await $.tool.run({ tool: 'mcp__crook__project_add_link', tool_use_id: 't1', value: 'https://x' })
  expect(r.deny).toBe("only on a turn the user started; don't retry")
})

test('userTurn tracking: a composer turn is a user turn, a wake-up is not', async ($, on) => {
  const w = world(on, ORCH, ping('orchestrator'))
  await $.session.start(START)
  const user = () => w.runs.filter(x => x.argv[2] === 'status').at(-1)?.stdin as { user_turn: boolean }
  const asked = async () => {
    await $.tool.run({ tool: 'mcp__crook__status', tool_use_id: 't' })
    return user().user_turn
  }
  await $.prompt.submit({ text: 'hello', origin: { kind: 'composer' }, wait: false })
  await $.turn.start({ text: 'hello', turnId: 'u1' })
  expect(await asked()).toBe(true)
  await $.prompt.submit({ text: 'tick', origin: { kind: 'scheduled-trigger' }, wait: false })
  await $.turn.start({ text: 'tick', turnId: 'u2' })
  expect(await asked()).toBe(false)
})

test('userTurn tracking: a wake-up then a composer prompt gives a machine turn, then a user turn', async ($, on) => {
  const w = world(on, ORCH, ping('orchestrator'))
  await $.session.start(START)
  const asked = async () => {
    await $.tool.run({ tool: 'mcp__crook__status', tool_use_id: 't' })
    return (w.runs.filter(x => x.argv[2] === 'status').at(-1)?.stdin as { user_turn: boolean }).user_turn
  }
  // The mod's own submit is not heard by its own prompt.submit hook, so the wake-up is never queued.
  await $.prompt.submit({ text: 'typed', origin: { kind: 'composer' }, wait: false })
  await $.turn.start({ text: 'crook: wake', turnId: 'a' })
  expect(await asked()).toBe(false)
  await $.turn.start({ text: 'typed', turnId: 'b' })
  expect(await asked()).toBe(true)
})

test('a subagent turn.start does not take a queued origin', async ($, on) => {
  const w = world(on, ORCH, ping('orchestrator'))
  await $.session.start(START)
  await $.prompt.submit({ text: 'typed', origin: { kind: 'composer' }, wait: false })
  await $.turn.start({ text: 'sub task', turnId: 's', agentId: 'agent-1' })
  await $.turn.start({ text: 'typed', turnId: 'b' })
  await $.tool.run({ tool: 'mcp__crook__status', tool_use_id: 't' })
  expect((w.runs.filter(x => x.argv[2] === 'status').at(-1)?.stdin as { user_turn: boolean }).user_turn).toBe(true)
})

test('crook-adopt typed at the composer runs adopt', async ($, on) => {
  const w = world(on, CHILD, ping('child'))
  await $.session.start(START)
  expect(w.commands).toEqual(['crook-adopt'])
  await $.command.run({ command: 'crook-adopt', args: '', origin: { kind: 'composer' }, presentation: PRES })
  const adopt = w.runs.find(x => x.argv[2] === 'adopt')
  expect(adopt?.stdin).toEqual({ session_id: 's1', user_turn: true })
})

test('crook-adopt from any other origin is refused and runs nothing', async ($, on) => {
  const w = world(on, CHILD, ping('child'))
  await $.session.start(START)
  for (const origin of [{ kind: 'bridge' }, { kind: 'scheduled-trigger' }, { kind: 'plugin', name: 'x' }] as const) {
    const r = await $.command.run({ command: 'crook-adopt', args: '', origin, presentation: PRES })
    expect(r.text).toContain('type it')
  }
  expect(w.runs.some(x => x.argv[2] === 'adopt')).toBe(false)
})

test('the child watcher wakes an idle child once per inbox state', async ($, on) => {
  const clock = mock.clock(on)
  let mtime = 1
  on('fs.stat', () => ({ value: { kind: 'file', size: 0, mtimeMs: mtime, isLink: false } }))
  on('turn.complete', (_, e) => ({ text: e.answer }))
  const snap = { untaken: 2, taken: 0, newest_seq: 2, task_open: false, parked: false, is_current: true, current_id: 's1', card_version: 1 }
  const w = world(on, CHILD, verb => {
    if (verb === 'ping') return { ok: true, role: 'child', gen: '/d/crook/p/gen' }
    if (verb === 'snapshot') return { ok: true, ...snap }
    return { ok: true, task: 'x' }
  })
  const wakes = () => w.submits.filter(x => x.text === 'crook: you have tasks waiting. Call take_task.')
  const snapshots = () => w.runs.filter(x => x.argv[2] === 'snapshot')
  const end = (turnId: string) => $.turn.complete({ answer: '', durationMs: 1, isAborted: false, turnId, reason: 'answer' })
  await $.session.start(START)
  await clock.advance(2000)
  expect(wakes()).toHaveLength(1)
  expect(snapshots()[0]?.stdin).toEqual({ session_id: 's1', user_turn: false })

  // The wake-up's turn ends without a take_task: the same inbox is not offered again.
  await $.turn.start({ text: 'crook: you have tasks waiting. Call take_task.', turnId: 'w1' })
  await end('w1')
  mtime = 2
  await clock.advance(6000)
  expect(wakes()).toHaveLength(1)

  // A turn that takes a task re-reads at its end, and the moved state gets one more offer.
  await $.turn.start({ text: 'crook: you have tasks waiting. Call take_task.', turnId: 'w2' })
  await $.tool.run({ tool: 'mcp__crook__take_task', tool_use_id: 't1' })
  const before = snapshots().length
  Object.assign(snap, { untaken: 1, taken: 1 })
  await end('w2')
  await clock.settle()
  expect(snapshots().length).toBeGreaterThan(before)
  expect(wakes()).toHaveLength(2)
})

test('the child watcher reads with the new session id after /clear', async ($, on) => {
  const clock = mock.clock(on)
  let mtime = 1
  on('fs.stat', () => ({ value: { kind: 'file', size: 0, mtimeMs: mtime, isLink: false } }))
  const w = world(on, CHILD, (verb, input) => {
    if (verb === 'ping') return { ok: true, role: 'child', gen: '/d/crook/p/gen' }
    const current = input.session_id === 's2'
    if (verb === 'snapshot') return { ok: true, untaken: 1, taken: 0, newest_seq: 1, task_open: false, parked: false, is_current: current, current_id: 's2', card_version: 1 }
    return { ok: true }
  })
  await $.session.start(START)
  await clock.advance(2000)
  expect(w.submits).toEqual([])
  // `/clear` moves the process to a new id; no session.start fires for it.
  w.session.id = 's2'
  mtime = 2
  await clock.advance(2000)
  expect(w.runs.filter(x => x.argv[2] === 'snapshot').at(-1)?.stdin).toEqual({ session_id: 's2', user_turn: false })
  expect(w.submits.map(x => x.text)).toEqual(['crook: you have tasks waiting. Call take_task.'])
})

test('park filter: a composer prompt in a child runs lift before it enters', async ($, on) => {
  const w = world(on, CHILD, ping('child'))
  await $.session.start(START)
  w.order.length = 0
  await $.prompt.submit({ text: 'postgres', origin: { kind: 'composer' }, wait: false })
  expect(w.order).toEqual(['run:lift', 'enter:postgres'])
  expect(w.runs.find(x => x.argv[2] === 'lift')?.stdin).toEqual({ session_id: 's1', user_turn: true })
})

test('park filter: a bridge prompt in a child runs lift', async ($, on) => {
  const w = world(on, CHILD, ping('child'))
  await $.session.start(START)
  w.order.length = 0
  await $.prompt.submit({ text: 'yes', origin: { kind: 'bridge' }, wait: false })
  expect(w.order).toEqual(['run:lift', 'enter:yes'])
})

test('park filter: machine prompts and slash commands in a child pass without lift', async ($, on) => {
  const w = world(on, CHILD, ping('child'))
  await $.session.start(START)
  await $.prompt.submit({ text: 'done', origin: { kind: 'task-notification' }, wait: false })
  await $.prompt.submit({ text: 'report', origin: { kind: 'peer' }, wait: false })
  await $.prompt.submit({ text: 'tick', origin: { kind: 'scheduled-trigger' }, wait: false })
  await $.prompt.submit({ text: '/compact', command: { name: 'compact', args: '' }, origin: { kind: 'composer' }, wait: false })
  expect(w.runs.some(x => x.argv[2] === 'lift')).toBe(false)
  expect(w.submits.map(x => x.text)).toEqual(['done', 'report', 'tick', '/compact'])
})

test('park filter: /loop in a child is dropped with a status line', async ($, on) => {
  const w = world(on, CHILD, ping('child'))
  await $.session.start(START)
  const r = await $.prompt.submit({ text: '/loop 5m /x', command: { name: 'loop', args: '5m /x' }, origin: { kind: 'composer' }, wait: false })
  expect(r).toEqual({ drop: 'crook: /loop is not allowed in a child' })
  expect(w.statuses.at(-1)).toBe('crook: /loop is not allowed in a child')
  expect(w.submits).toEqual([])
  expect(w.runs.some(x => x.argv[2] === 'lift')).toBe(false)
})

test('park filter: a failed lift shows on the status line and the prompt still enters', async ($, on) => {
  const w = world(on, CHILD, verb => (verb === 'lift' ? { ok: false, error: 'refused' } : { ok: true, role: 'child' }))
  await $.session.start(START)
  await $.prompt.submit({ text: 'yes', origin: { kind: 'composer' }, wait: false })
  expect(w.statuses.at(-1)).toBe('crook: park lift failed')
  expect(w.submits.map(x => x.text)).toEqual(['yes'])
})

test('park filter: when the filter throws, its catch still lifts and lets a composer prompt in', async ($, on) => {
  const w = world(on, CHILD, ping('child'))
  await $.session.start(START)
  w.session.failIds = 1
  w.order.length = 0
  await $.prompt.submit({ text: 'answer', origin: { kind: 'composer' }, wait: false })
  expect(w.order).toEqual(['run:lift', 'enter:answer'])
})

test('park filter: an orchestrator composer prompt runs no lift', async ($, on) => {
  const w = world(on, ORCH, ping('orchestrator'))
  await $.session.start(START)
  await $.prompt.submit({ text: 'hello', origin: { kind: 'composer' }, wait: false })
  await $.prompt.submit({ text: '/loop', command: { name: 'loop', args: '' }, origin: { kind: 'composer' }, wait: false })
  expect(w.runs.some(x => x.argv[2] === 'lift')).toBe(false)
  expect(w.submits.map(x => x.text)).toEqual(['hello', '/loop'])
})

test('the orchestrator watcher wakes once for done children, acks it and shows the status line', async ($, on) => {
  const clock = mock.clock(on)
  on('fs.stat', () => ({ value: { kind: 'file', size: 0, mtimeMs: 1, isLink: false } }))
  on('turn.complete', (_, e) => ({ text: e.answer }))
  const acked: Record<string, number>[] = []
  const w = world(on, ORCH, (verb, input) => {
    if (verb === 'ping') return { ok: true, role: 'orchestrator', gen: '/d/crook/p/gen' }
    if (verb === 'snapshot') {
      const ack = input.ack as Record<string, number> | undefined
      if (ack !== undefined) acked.push(ack)
      return {
        ok: true,
        done_unnotified: ack === undefined ? [{ child: 'c1', done_seq: 4 }] : [],
        waiting: 1,
        working: 2,
        done: 1,
        untaken: { c1: 3 },
        card_stale: true,
        card_version: 1,
        is_current: true,
      }
    }
    return { ok: true }
  })
  await $.session.start(START)
  await clock.advance(2000)
  await clock.settle()
  expect(w.submits.map(x => x.text)).toEqual(['crook: children are done. Call status().'])
  expect(acked).toEqual([{ c1: 4 }])
  expect(w.statuses.at(-1)).toBe('crook: 1 waiting on you · 2 working · card stale · c1: 3 untaken')
})

test('orchestrator context: a composer prompt carries orch-context lines, a scheduled prompt none', async ($, on) => {
  const w = world(on, ORCH, verb => {
    if (verb === 'ping') return { ok: true, role: 'orchestrator' }
    if (verb === 'orch-context') return { ok: true, lines: ['c1: waiting on you'], stale: true }
    return { ok: true }
  })
  await $.session.start(START)
  await $.prompt.submit({ text: 'hello', origin: { kind: 'composer' }, wait: false })
  await $.prompt.submit({ text: 'tick', origin: { kind: 'scheduled-trigger' }, wait: false })
  expect(w.submits[0]?.context).toEqual(['c1: waiting on you', 'the card is stale: move Where it stands on, keeping what is still true'])
  expect(w.submits[1]?.context).toBeUndefined()
  expect(w.runs.filter(x => x.argv[2] === 'orch-context')).toHaveLength(1)
  expect(w.runs.find(x => x.argv[2] === 'orch-context')?.stdin).toEqual({ session_id: 's1', user_turn: true })
})
