import type { EngineInterface, PromptSubmitInput, PromptSubmitResult, Register } from 'claude-code'

import { ports } from './adapter.ts'
import { noteOrigin, turnStart, userTurn } from './core/origin.ts'
import { filter as orchFilter, onOrchestratorTurn, startOrchestratorWatcher } from './core/orchestrator.ts'
import { filter, rescue } from './core/park.ts'
import { TOOLS, findTool, runTool, toolsFor } from './core/tools.ts'
import { onTurn, startChildWatcher } from './core/watcher.ts'

// A mod may hand `$` only to top-level functions of this file, so the helpers that take it sit here.

// What the main-loop turn now running has done, for the watcher at its end.
let calledTake = false
let calledDone = false

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

/** The session id of the moment: after `/clear` it is a new one, and no `session.start` fires for it. */
function currentId($: EngineInterface): () => Promise<string> {
  return () => $.session.id()
}

async function serve($: EngineInterface, name: string, input: Record<string, unknown>) {
  const def = findTool(name)
  if (def === undefined) return { deny: `crook: no such tool ${name}` }
  const answer = await runTool(ports($), def, input, await $.session.id(), userTurn())
  if ('result' in answer) {
    if (name === 'take_task') calledTake = true
    if (name === 'done') calledDone = true
  }
  return answer
}

async function adopt($: EngineInterface, origin: string): Promise<string> {
  // The engine stamps the origin of the command itself; only the user's own Enter counts.
  if (origin !== 'composer') return 'crook: /crook-adopt works only when you type it at the prompt'
  const r = await ports($).run('adopt', { session_id: await $.session.id(), user_turn: true })
  return r.ok ? 'crook: this session is now the child' : r.error ?? 'crook: adopt failed'
}

// The launcher sets the role in the environment, so it is known before `session.start`'s ping returns.
async function isChild($: EngineInterface): Promise<boolean> {
  return (await $.env.get('CROOK_ROLE').catch(() => undefined)) === 'child'
}

async function isOrchestrator($: EngineInterface): Promise<boolean> {
  return (await $.env.get('CROOK_ROLE').catch(() => undefined)) === 'orchestrator'
}

async function roleFilter($: EngineInterface, e: PromptSubmitInput, next: (e: PromptSubmitInput) => Promise<PromptSubmitResult>) {
  if (await isChild($)) return filter(ports($), currentId($), e, next)
  if (await isOrchestrator($)) return orchFilter(ports($), currentId($), e, next)
  return next(e)
}

async function parkRescue($: EngineInterface, e: PromptSubmitInput, next: (e: PromptSubmitInput) => Promise<PromptSubmitResult>, called: boolean) {
  return called || !(await isChild($)) ? next(e) : rescue(ports($), currentId($), e, next, false)
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    const p = ports($)
    const sessionId = await $.session.id()
    const r = await p.run('ping', { session_id: sessionId, user_turn: false })
    if (!r.ok) {
      p.status(r.error ?? 'ping failed')
    } else {
      p.status(String(r.role))
      if (typeof r.gen === 'string') {
        if (r.role === 'child') startChildWatcher(p, { sessionId: currentId($), gen: r.gen })
        if (r.role === 'orchestrator') startOrchestratorWatcher(p, { sessionId: currentId($), gen: r.gen })
      }
    }
    // Tools register even when ping fails, by the environment's role, so each one can say why it won't run.
    await registerTools($, (r.ok ? String(r.role) : p.env('CROOK_ROLE')) ?? '')
    return next(e)
  })

  // The origin is queued before `next`, which resolves only once the turn has started.
  // The role's filter then runs (the park filter in a child, the context in the orchestrator); `noteOrigin` stays first and synchronous to keep queue order.
  on('prompt.submit', ($, e, next) => {
    noteOrigin(e.origin.kind, e.command !== undefined, e.text)
    return roleFilter($, e, next)
  }).catch(($, e, next) => parkRescue($, e, next, next.called))

  on('turn.start', ($, e, next) => {
    if (e.agentId === undefined) {
      turnStart(e.text)
      calledTake = false
      calledDone = false
    }
    const start = { kind: 'start', agentId: e.agentId, calledTake: false, calledDone: false } as const
    onTurn(start)
    onOrchestratorTurn(start)
    return next(e)
  })

  on('turn.complete', ($, e, next) => {
    const end = { kind: 'complete', agentId: e.agentId, calledTake, calledDone } as const
    onTurn(end)
    onOrchestratorTurn(end)
    return next(e)
  })

  // One hook per tool: a catch-all `tool.run` hook would turn off background moves and live output for every tool.
  for (const t of TOOLS) {
    on('tool.run', { tool: `mcp__crook__${t.name}` }, ($, e) => serve($, t.name, e))
  }

  on('command.run', { command: 'crook-adopt' }, async ($, e) => ({ text: await adopt($, e.origin.kind) }))
}
