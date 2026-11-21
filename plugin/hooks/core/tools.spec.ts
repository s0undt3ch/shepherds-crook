import { describe, expect, it } from 'vitest'

import type { Ports, VerbResult } from './ports.ts'
import { TOOLS, findTool, runTool, toolsFor } from './tools.ts'

function fake(reply: VerbResult) {
  const calls: { verb: string; input: Record<string, unknown> }[] = []
  const timeouts: (number | undefined)[] = []
  const ports = {
    run: async (verb: string, input: object, timeoutMs?: number) => {
      calls.push({ verb, input: input as Record<string, unknown> })
      timeouts.push(timeoutMs)
      return reply
    },
  } as unknown as Ports
  return { ports, calls, timeouts }
}

const names = (role: string) => toolsFor(role).map(t => t.name)

describe('toolsFor', () => {
  it('gives a child its five tools', () => {
    expect(names('child').sort()).toEqual(['done', 'need', 'status', 'take_task', 'worktree'])
  })

  it('gives the orchestrator its ten tools', () => {
    expect(names('orchestrator').sort()).toEqual(
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
  })

  it('gives an unknown role nothing', () => {
    expect(toolsFor('none')).toEqual([])
  })

  it('never offers lift or adopt to the model', () => {
    const all = TOOLS.map(t => t.name)
    expect(all).not.toContain('lift')
    expect(all).not.toContain('adopt')
    expect(TOOLS.map(t => t.verb)).not.toContain('lift')
    expect(TOOLS.map(t => t.verb)).not.toContain('adopt')
  })

  it('has unique names, and a description and object schema for each', () => {
    expect(new Set(TOOLS.map(t => t.name)).size).toBe(TOOLS.length)
    for (const t of TOOLS) {
      expect(t.description.length).toBeGreaterThan(10)
      expect(t.inputSchema.type).toBe('object')
    }
  })
})

describe('schemas', () => {
  it('done.outcome is an enum and pr_url is optional', () => {
    const s = findTool('done')?.inputSchema as { properties: Record<string, { enum?: string[] }>; required: string[] }
    expect(s.properties.outcome?.enum).toEqual(['ok', 'failed', 'dropped'])
    expect(s.required).toEqual(['outcome'])
  })

  it('need takes the question', () => {
    const s = findTool('need')?.inputSchema as { required: string[] }
    expect(s.required).toEqual(['question'])
  })

  it('worktree takes a name and an optional branch, never a path', () => {
    const s = findTool('worktree')?.inputSchema as { properties: object; required: string[] }
    expect(Object.keys(s.properties).sort()).toEqual(['branch', 'name'])
    expect(s.required).toEqual(['name'])
  })

  it('spawn takes child, repo, brief_text and an optional branch', () => {
    const s = findTool('spawn')?.inputSchema as { properties: object; required: string[] }
    expect(Object.keys(s.properties).sort()).toEqual(['branch', 'brief_text', 'child', 'repo'])
    expect(s.required).toEqual(['child', 'repo', 'brief_text'])
  })

  it('the card writes take the version, the appends do not', () => {
    expect(findTool('project_set_what_why')?.inputSchema.required).toEqual(['what', 'why', 'version'])
    expect(findTool('project_set_status')?.inputSchema.required).toEqual(['text', 'version'])
    expect(findTool('project_add_link')?.inputSchema.required).toEqual(['value'])
  })
})

describe('runTool', () => {
  it('sends session_id, user_turn and the declared inputs to the verb', async () => {
    const { ports, calls } = fake({ ok: true })
    await runTool(ports, findTool('need')!, { question: 'which db?' }, 's1', true)
    expect(calls).toEqual([{ verb: 'need', input: { question: 'which db?', session_id: 's1', user_turn: true } }])
  })

  it('maps tool names to the verb names', async () => {
    const { ports, calls } = fake({ ok: true })
    await runTool(ports, findTool('take_task')!, {}, 's1', false)
    await runTool(ports, findTool('project_set_what_why')!, { what: 'w', why: 'y', version: 3 }, 's1', true)
    expect(calls.map(c => c.verb)).toEqual(['take-task', 'project-set-what-why'])
  })

  it('sends project_add_* to project-add with its kind', async () => {
    const { ports, calls } = fake({ ok: true })
    for (const k of ['link', 'ticket', 'repo', 'notes']) {
      await runTool(ports, findTool(`project_add_${k}`)!, { value: 'v' }, 's1', true)
    }
    expect(calls.map(c => [c.verb, c.input.kind, c.input.value])).toEqual([
      ['project-add', 'link', 'v'],
      ['project-add', 'ticket', 'v'],
      ['project-add', 'repo', 'v'],
      ['project-add', 'notes', 'v'],
    ])
  })

  it('gives spawn, worktree and stop their timeouts and every other tool the default', async () => {
    const { ports, timeouts } = fake({ ok: true })
    const input = { child: 'c1', repo: '/r', brief_text: 'b', text: 't', name: 'n', value: 'v' }
    for (const t of TOOLS) await runTool(ports, t, input, 's1', true)
    const byName = new Map(TOOLS.map((t, i) => [t.name, timeouts[i]]))
    expect(byName.get('spawn')).toBe(120000)
    // Above crook's own limits: git worktree add 90 s; claude agents 5 s plus claude stop 30 s.
    expect(byName.get('worktree')).toBeGreaterThan(90_000)
    expect(byName.get('stop')).toBeGreaterThan(35_000)
    expect([...byName].filter(([, ms]) => ms !== undefined).map(([n]) => n).sort()).toEqual(['spawn', 'stop', 'worktree'])
  })

  it('sends task, stop and worktree to their verbs with the declared inputs only', async () => {
    const { ports, calls } = fake({ ok: true })
    await runTool(ports, findTool('task')!, { child: 'c1', text: 'go', path: '/x' }, 's1', false)
    await runTool(ports, findTool('stop')!, { child: 'c1' }, 's1', false)
    await runTool(ports, findTool('worktree')!, { name: 'fix', path: '/etc/passwd' }, 's1', false)
    expect(calls).toEqual([
      { verb: 'task', input: { child: 'c1', text: 'go', session_id: 's1', user_turn: false } },
      { verb: 'stop', input: { child: 'c1', session_id: 's1', user_turn: false } },
      { verb: 'worktree', input: { name: 'fix', session_id: 's1', user_turn: false } },
    ])
  })

  it('drops keys the schema does not declare, so the model cannot set identity', async () => {
    const { ports, calls } = fake({ ok: true })
    await runTool(
      ports,
      findTool('need')!,
      { question: 'q', session_id: 'forged', user_turn: true, tool: 'x', tool_use_id: 't', path: '/etc' },
      's1',
      false,
    )
    expect(calls[0]?.input).toEqual({ question: 'q', session_id: 's1', user_turn: false })
  })

  it('returns the verb message, or the task text, or the rest as JSON', async () => {
    expect(await runTool(fake({ ok: true, message: 'noted' }).ports, findTool('need')!, { question: 'q' }, 's', false)).toEqual({ result: 'noted' })
    expect(await runTool(fake({ ok: true, seq: 2, text: 'do it' }).ports, findTool('take_task')!, {}, 's', false)).toEqual({ result: 'do it' })
    expect(await runTool(fake({ ok: true, version: 4 }).ports, findTool('project_set_status')!, { text: 't', version: 3 }, 's', true)).toEqual({ result: '{"version":4}' })
    expect(await runTool(fake({ ok: true }).ports, findTool('need')!, { question: 'q' }, 's', false)).toEqual({ result: 'ok' })
  })

  it('denies with the verb error', async () => {
    const r = await runTool(fake({ ok: false, error: 'only on a turn the user started; don\'t retry' }).ports, findTool('project_add_link')!, { value: 'v' }, 's', false)
    expect(r).toEqual({ deny: 'only on a turn the user started; don\'t retry' })
  })

  it('passes crook: binary not found through', async () => {
    const r = await runTool(fake({ ok: false, error: 'crook: binary not found' }).ports, findTool('status')!, {}, 's', false)
    expect(r).toEqual({ deny: 'crook: binary not found' })
  })
})
