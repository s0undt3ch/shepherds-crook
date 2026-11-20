import type { Register } from 'claude-code'

import { ports } from './adapter.ts'

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
    return next(e)
  })
}
