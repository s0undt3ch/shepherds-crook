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
  mock.env(on, env)
  on('session.id', () => ({ value: 's1' }))
  on('ui.status', () => ({ value: undefined }))
  on('session.start', () => ({ cwd: '/w' }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('prompt.submit', (_, e) => ({ text: e.text }))
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
    return { value: { exitCode: 0, stdout: JSON.stringify(reply(verb, stdin)), stderr: '', isStdoutTruncated: false, isStderrTruncated: false } }
  })
  return { runs, registered, commands }
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
