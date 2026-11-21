import type { EngineInterface, Register } from 'claude-code'

import { ports } from './adapter.ts'
import { noteOrigin, turnStart, userTurn } from './core/origin.ts'
import { TOOLS, findTool, runTool, toolsFor } from './core/tools.ts'

// A mod may hand `$` only to top-level functions of this file, so the helpers that take it sit here.

async function registerTools($: EngineInterface, role: string): Promise<void> {
  for (const t of toolsFor(role)) {
    await $.tool.register({
      name: t.name,
      description: t.description,
      inputSchema: t.inputSchema,
      isDeferred: false,
    })
  }
  if (role === 'child') {
    await $.command.register({
      name: 'crook-adopt',
      description: 'Make this session the child, for a fork you carry on in',
    })
  }
}

async function serve($: EngineInterface, name: string, input: Record<string, unknown>) {
  const def = findTool(name)
  if (def === undefined) return { deny: `crook: no such tool ${name}` }
  return runTool(ports($), def, input, await $.session.id(), userTurn())
}

async function adopt($: EngineInterface, origin: string): Promise<string> {
  // The engine stamps the origin of the command itself; only the user's own Enter counts.
  if (origin !== 'composer') return 'crook: /crook-adopt works only when you type it at the prompt'
  const r = await ports($).run('adopt', { session_id: await $.session.id(), user_turn: true })
  return r.ok ? 'crook: this session is now the child' : r.error ?? 'crook: adopt failed'
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    const p = ports($)
    const sessionId = await $.session.id()
    const r = await p.run('ping', { session_id: sessionId, user_turn: false })
    if (!r.ok) {
      // The error is already `crook: ...` for transport failures; a verb's own error is not.
      const why = r.error ?? 'ping failed'
      $.ui.status(why.startsWith('crook:') ? why : `crook: ${why}`)
    } else {
      $.ui.status(`crook: ${String(r.role)}`)
    }
    // Tools register even when ping fails, by the environment's role, so each one can say why it won't run.
    await registerTools($, (r.ok ? String(r.role) : p.env('CROOK_ROLE')) ?? '')
    return next(e)
  })

  // The origin is queued before `next`, which resolves only once the turn has started.
  on('prompt.submit', ($, e, next) => {
    noteOrigin(e.origin.kind, e.command !== undefined, e.text)
    return next(e)
  })

  on('turn.start', ($, e, next) => {
    if (e.agentId === undefined) turnStart(e.text)
    return next(e)
  })

  // One hook per tool: a catch-all `tool.run` hook would turn off background moves and live output for every tool.
  for (const t of TOOLS) {
    on('tool.run', { tool: `mcp__crook__${t.name}` }, ($, e) => serve($, t.name, e))
  }

  on('command.run', { command: 'crook-adopt' }, async ($, e) => ({ text: await adopt($, e.origin.kind) }))
}
