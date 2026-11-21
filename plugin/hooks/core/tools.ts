import type { Ports } from './ports.ts'

type Role = 'child' | 'orchestrator'

export interface ToolDef {
  /** The name `$.tool.register` takes; the model calls it `mcp__crook__<name>`. */
  name: string
  /** The `crook mod <verb>` it runs. */
  verb: string
  roles: readonly Role[]
  description: string
  inputSchema: { type: 'object'; properties: Record<string, unknown>; required?: string[] }
  /** Fixed verb input the model does not choose. */
  fixed?: Record<string, unknown>
  /** How long the verb may run, where `$.process.run`'s 30 s default is too short. */
  timeoutMs?: number
}

const SPAWN_TIMEOUT_MS = 120_000

const str = (description: string) => ({ type: 'string', description })
const none = { type: 'object', properties: {} } as const
const CARD = 'Refuses unless the turn began with a prompt the user typed; if it refuses, do not retry.'

const add = (kind: string, what: string): ToolDef => ({
  name: `project_add_${kind}`,
  verb: 'project-add',
  roles: ['orchestrator'],
  description: `Append ${what} to the project record. ${CARD}`,
  inputSchema: { type: 'object', properties: { value: str(`The ${kind} to add.`) }, required: ['value'] },
  fixed: { kind },
})

// `lift` and `adopt` are deliberately absent: lift belongs to the park filter, adopt to `/crook-adopt`.
export const TOOLS: readonly ToolDef[] = [
  {
    name: 'take_task',
    verb: 'take-task',
    roles: ['child'],
    description: 'Take the oldest task waiting in your inbox. Refuses while a task is still open: call done first.',
    inputSchema: none,
  },
  {
    name: 'need',
    verb: 'need',
    roles: ['child'],
    description: 'Ask the user a question you cannot answer yourself, then stop and wait for the answer.',
    inputSchema: { type: 'object', properties: { question: str('The question for the user.') }, required: ['question'] },
  },
  {
    name: 'done',
    verb: 'done',
    roles: ['child'],
    description: 'Report the current task finished.',
    inputSchema: {
      type: 'object',
      properties: {
        outcome: { type: 'string', enum: ['ok', 'failed', 'dropped'], description: 'How the task ended.' },
        pr_url: str('The pull request URL, if there is one.'),
      },
      required: ['outcome'],
    },
  },
  {
    name: 'status',
    verb: 'status',
    roles: ['child', 'orchestrator'],
    description: 'Show where things stand: your current task and open questions, or the project and its children.',
    inputSchema: none,
  },
  {
    name: 'worktree',
    verb: 'worktree',
    roles: ['child'],
    description:
      'Make an extra git worktree in your own repo, as <repo>/.worktrees/<project>/<you>.<name>. Returns its path. Never run git worktree add yourself.',
    inputSchema: {
      type: 'object',
      properties: {
        name: str('Lowercase letters, digits and hyphens.'),
        branch: str('The branch to create; defaults to crook/<project>/<you>.<name>.'),
      },
      required: ['name'],
    },
  },
  {
    name: 'spawn',
    verb: 'spawn',
    roles: ['orchestrator'],
    // It runs `git worktree add`, which checks out the whole tree, then starts `claude`.
    timeoutMs: SPAWN_TIMEOUT_MS,
    description: 'Start a child session in a new worktree of one of the project repos, with a brief.',
    inputSchema: {
      type: 'object',
      properties: {
        child: str('The child name.'),
        repo: str('A repo in the project record.'),
        brief_text: str('The brief the child starts from.'),
        branch: str('The branch to create; defaults to crook/<project>/<child>.'),
      },
      required: ['child', 'repo', 'brief_text'],
    },
  },
  {
    name: 'task',
    verb: 'task',
    roles: ['orchestrator'],
    description: "Queue a task in a child's inbox.",
    inputSchema: {
      type: 'object',
      properties: { child: str('The child name.'), text: str('The task.') },
      required: ['child', 'text'],
    },
  },
  {
    name: 'stop',
    verb: 'stop',
    roles: ['orchestrator'],
    description: 'Stop a child session and archive it.',
    inputSchema: { type: 'object', properties: { child: str('The child name.') }, required: ['child'] },
  },
  {
    name: 'project_set_what_why',
    verb: 'project-set-what-why',
    roles: ['orchestrator'],
    description: `Set the project's what and why. Pass the version status() returned. ${CARD}`,
    inputSchema: {
      type: 'object',
      properties: {
        what: str('What the project is.'),
        why: str('Why it exists.'),
        version: { type: 'integer', description: "The card version from status()." },
      },
      required: ['what', 'why', 'version'],
    },
  },
  {
    name: 'project_set_status',
    verb: 'project-set-status',
    roles: ['orchestrator'],
    description: `Set where the project stands. Pass the version status() returned. ${CARD}`,
    inputSchema: {
      type: 'object',
      properties: {
        text: str('Where the project stands.'),
        version: { type: 'integer', description: 'The card version from status().' },
      },
      required: ['text', 'version'],
    },
  },
  add('link', 'a link'),
  add('ticket', 'a ticket'),
  add('repo', 'a repo'),
  add('notes', 'a note'),
]

export const toolsFor = (role: string): ToolDef[] => TOOLS.filter(t => (t.roles as readonly string[]).includes(role))

export const findTool = (name: string): ToolDef | undefined => TOOLS.find(t => t.name === name)

export type ToolAnswer = { result: string } | { deny: string }

function render(def: ToolDef, reply: Record<string, unknown>): string {
  if (typeof reply.message === 'string') return reply.message
  if (def.name === 'take_task' && typeof reply.text === 'string') return reply.text
  const { ok: _ok, ...rest } = reply
  return Object.keys(rest).length === 0 ? 'ok' : JSON.stringify(rest)
}

/**
 * Runs a tool's verb. Only the keys the schema declares reach the verb, and `session_id` and
 * `user_turn` come last, so the model's input can never set them.
 */
export async function runTool(
  ports: Ports,
  def: ToolDef,
  input: Record<string, unknown>,
  sessionId: string,
  userTurn: boolean,
): Promise<ToolAnswer> {
  const declared: Record<string, unknown> = {}
  for (const key of Object.keys(def.inputSchema.properties)) {
    if (input[key] !== undefined) declared[key] = input[key]
  }
  const reply = await ports.run(def.verb, { ...declared, ...def.fixed, session_id: sessionId, user_turn: userTurn }, def.timeoutMs)
  return reply.ok ? { result: render(def, reply) } : { deny: reply.error ?? 'crook: bad reply' }
}
