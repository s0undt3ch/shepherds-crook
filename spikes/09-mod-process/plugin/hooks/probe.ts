// Spike 9 probe: throwaway. Runs `crook mod ping` from three call sites and logs what it sees.
// `$` may only be handed to top-level function declarations of this file, so the helpers sit here.
import type { Register } from 'claude-code'

import { ports } from './adapter.ts'

type Rec = Record<string, unknown>

let chain: Promise<void> = Promise.resolve()

// `$.fs` has no append: read, then write, one at a time so lines don't clobber each other.
function log($: any, rec: Rec): Promise<void> {
  chain = chain
    .then(async () => {
      const path = await $.env.get('SPIKE_LOG')
      if (!path) return
      let cur = ''
      try {
        cur = await $.fs.read(path)
      } catch {
        // first line
      }
      await $.fs.write(path, cur + JSON.stringify({ t: Date.now(), ...rec }) + '\n')
    })
    .catch(() => undefined)
  return chain
}

async function statGen($: any): Promise<Rec> {
  const xdg = await $.env.get('XDG_DATA_HOME')
  const slug = await $.env.get('CROOK_PROJECT')
  const path = `${xdg}/crook/${slug}/gen`
  try {
    const s = await $.fs.stat(path)
    return { path, ok: true, kind: s.kind, size: s.size, mtimeMs: s.mtimeMs }
  } catch (err) {
    return { path, ok: false, error: String(err) }
  }
}

async function site($: any, name: string): Promise<void> {
  const p = ports($)
  const view = await $.env.get('CLAUDECODE')
  const t0 = Date.now()
  const r = await p.run('ping', { session_id: await $.session.id(), user_turn: false })
  const ms = Date.now() - t0
  await log($, { site: name, ping: r, ms, claudecodeInMod: view ?? null, stat: await statGen($) })
}

async function raw($: any, name: string, argv: string[], init: Rec): Promise<void> {
  const t0 = Date.now()
  try {
    const r = await $.process.run(argv, init)
    await log($, { raw: name, ms: Date.now() - t0, exit: r.exitCode, stdout: r.stdout, stderr: r.stderr })
  } catch (err) {
    await log($, { raw: name, ms: Date.now() - t0, rejected: String(err) })
  }
}

async function startup($: any): Promise<void> {
  await $.tool.register({
    name: 'probe',
    description: 'Spike probe: call it once with no arguments.',
    inputSchema: { type: 'object', properties: {} },
    isDeferred: false,
  })
  await log($, { site: 'session.start', claudecodeInMod: (await $.env.get('CLAUDECODE')) ?? null })

  const bin = await $.env.get('CROOK_BIN')
  const stdin = JSON.stringify({ session_id: 'spike', user_turn: false })
  await raw($, 'ping-plain', [bin, 'mod', 'ping'], { stdin })
  await raw($, 'ping-env-empty-claudecode', [bin, 'mod', 'ping'], { stdin, env: { CLAUDECODE: '' } })
  await raw($, 'env-plain', ['/usr/bin/env'], {})
  await raw($, 'env-with-override', ['/usr/bin/env'], { env: { SPIKE_PROBE: '1' } })
}

let ticks = 0
async function tick($: any): Promise<void> {
  ticks += 1
  if (ticks <= 6) await site($, `clock.every#${ticks}`)
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    await startup($)
    $.clock.every(2000, () => tick($))
    return next(e)
  })

  on('prompt.submit', async ($, e, next) => {
    await site($, 'prompt.submit')
    return next(e)
  })

  on('tool.run', { tool: 'mcp__crookspike__probe' }, async $ => {
    await site($, 'tool.run')
    return { result: 'probe ran' }
  })
}
