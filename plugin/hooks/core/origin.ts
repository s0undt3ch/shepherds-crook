const USER_KINDS = new Set(['composer', 'bridge'])

// Entries whose turn never starts (a prompt delivered into a running turn) are skipped by a text match;
// the cap keeps a stream of them from growing the queue.
const MAX_PENDING = 64

interface Entry {
  user: boolean
  text: string | undefined
}

/**
 * Tracks whether the turn now running began with a prompt the user typed.
 *
 * `turn.start` carries the prompt's text but not its origin, so each submitted prompt's origin is
 * queued and the turn that starts takes the newest entry with its text (dropping older entries,
 * whose turn never started). A turn matching no entry, or with no text, is a machine turn.
 */
export function createOrigin() {
  let queue: Entry[] = []
  let latched = false

  const push = (entry: Entry): void => {
    queue.push(entry)
    if (queue.length > MAX_PENDING) queue = queue.slice(-MAX_PENDING)
  }

  return {
    /** A prompt was submitted. A slash command starts no turn, so it is not queued. */
    noteOrigin(kind: string, isCommand: boolean, text?: string): void {
      if (!isCommand) push({ user: USER_KINDS.has(kind), text })
    },
    /** The mod submitted a prompt itself: never a user turn. */
    noteOwnSubmit(text?: string): void {
      push({ user: false, text })
    },
    /** A main-loop `turn.start`: latches the origin of the prompt that started it. */
    turnStart(text: string): void {
      if (text === '') {
        latched = false
        return
      }
      // The newest match wins, so a stale entry with the same text cannot. No match fails closed: the
      // mod's own wake-ups are never queued by the hook, and must not take a user's entry.
      const at = queue.findLastIndex(en => en.text === text)
      latched = at >= 0 ? (queue.splice(0, at + 1).at(-1)?.user ?? false) : false
    },
    userTurn: (): boolean => latched,
    pending: (): number => queue.length,
  }
}

const shared = createOrigin()

export const noteOrigin = shared.noteOrigin
export const noteOwnSubmit = shared.noteOwnSubmit
export const turnStart = shared.turnStart
export const userTurn = shared.userTurn
