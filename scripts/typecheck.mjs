// Lays the engine's `claude-code` types where plugin/tsconfig.json expects
// them, then runs `tsc -p plugin/`. Claude Code's own declaration file is not
// vendored (licence), and neither `claude plugin validate` nor `tsc` writes it:
// the engine writes it when a session loads a mod or the plugin-authoring skill.
import { spawnSync } from 'node:child_process'
import { copyFileSync, existsSync, mkdirSync, readdirSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const dest = 'plugin/.claude-plugin/types/claude-code/index.d.ts'

function findTypes() {
  if (process.env.CLAUDE_CODE_TYPES) return process.env.CLAUDE_CODE_TYPES
  const ver = spawnSync('claude', ['--version'], { encoding: 'utf8' }).stdout?.split(' ')[0]
  const roots = new Set(['/tmp', '/private/tmp', tmpdir()])
  for (const root of roots) {
    if (!existsSync(root)) continue
    for (const session of readdirSync(root).filter(n => n.startsWith('claude-'))) {
      const bundled = join(root, session, 'bundled-skills', ver ?? '')
      if (!ver || !existsSync(bundled)) continue
      for (const hash of readdirSync(bundled)) {
        const f = join(bundled, hash, 'plugin-authoring/types/claude-code.d.ts')
        if (existsSync(f)) return f
      }
    }
  }
  return undefined
}

const src = findTypes()
if (!src || !existsSync(src)) {
  console.error(
    'typecheck: cannot find claude-code.d.ts for this Claude Code build.\n' +
      'Set CLAUDE_CODE_TYPES to its path, or start Claude Code once with the plugin-authoring\n' +
      'skill loaded (it writes <tmp>/claude-*/bundled-skills/<version>/*/plugin-authoring/types/).',
  )
  process.exit(1)
}
mkdirSync(join(dest, '..'), { recursive: true })
copyFileSync(src, dest)
console.log(`typecheck: types from ${src}`)
const r = spawnSync('npx', ['tsc', '-p', 'plugin/'], { stdio: 'inherit' })
process.exit(r.status ?? 1)
