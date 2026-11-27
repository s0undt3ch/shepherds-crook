# hc and herdr-app spikes (2026-11-26)

Versions: hc 0.1.22, Claude Code 2.1.332. Children ran `claude --model haiku` to keep cost down.

Most spikes ran in a scratch hc core (`HC_SESSION` set, scratch `XDG_STATE_HOME` and `XDG_RUNTIME_DIR`),
as the hc docs ask for tests. Working folders were under `$TMPDIR/crook-e2e-spk.*`. S3 ran in the
maintainer's real core, because only that core is watched by herdr-app.

## 1. Does a child's turn end send a note to its parent?

**No.** A child replied "OK" and went `done`. With `draft_wait = 0`, the parent received nothing in 30
seconds.

Exit and blocked do send notes:

```text
<hc>t5 exiter exited (exit 3). Read: hc read t5</hc>
<hc>t6 kidq is blocked (question). See: hc read t6</hc>
```

Used by [0009](../decisions/0009-child-stop-hook-reports-completion.md).

## 2. What happens to `hc send` aimed at a blocked, dead or reused name?

**It fails loudly and never queues.**

| Target | Exit | `kind` |
| --- | --- | --- |
| Blocked (question on screen) | 9 | `blocked` |
| Killed, record kept | 11 | `finished` |
| Removed name | 3 | `not_found` |

A name relaunched after removal gets a new address and no stale messages.

Used by [0008](../decisions/0008-messages-over-hc-send-results-folder-inbox.md).

## 3. S1: does AskUserQuestion show as `blocked`?

**Yes.** Status:

```json
{"app":"claude-code","kind":"question","msg":"answer: Do you prefer red or blue? (Red · Blue)","state":"blocked"}
```

The parent got `<hc>t6 kidq is blocked (question). See: hc read t6</hc>`. Used by
[0010](../decisions/0010-human-input-through-hc-blocked.md).

## 4. S4: do children survive a coordinator restart?

- By default a child ends with its parent's program: two children ended when the parent was killed.
- With `--outlive-parent` the child lives, but its parent stays the dead address. Its later blocked note
  never reached the relaunched coordinator. `hc schema` has no re-parent call (only `parent`, `no_parent`,
  `outlive_parent`, `die_with_parent`).
- With the coordinator hosted in a shell (terminal program `sh`, Claude started inside it), a child survived
  Claude's `/exit` without `--outlive-parent`. After Claude was started again in the same terminal, the
  child's blocked note arrived there.

## 5. What reaches a host shell while Claude is not running in it?

- A child's exit note was not typed at all: it is lost.
- `hc send host2 "/tmp/results/kid.md"` returned exit 0, and the shell ran the text:
  `sh: /tmp/results/kid.md: No such file or directory`.

Used by [0011](../decisions/0011-shell-hosted-coordinator.md).

## 6. S2 (recast): do `hc run` children get a clean environment and a fixed session id?

v1's S2 asked about a `crook launch` binary, which the redesign drops. Recast:

- The real herdr-app core gives its shells only `CLAUDE_CONFIG_DIR` and the `HC_*` identity variables
  (`ps -E`, names only). No `CLAUDE_CODE_CHILD_SESSION`.
- A scratch core started from inside a Claude session passed that session's variables to every child.
  Children showed "Transcript saving is off — inherited CLAUDE_CODE_CHILD_SESSION marker".
- A scratch core started with those variables removed: two children had no session variables, their own
  `HC_TERMINAL_ID` (t1, t2), and no transcript warning.
- `claude --session-id <uuid>` was honoured: `<uuid>.jsonl` existed for both children.

Used by [0016](../decisions/0016-hc-preflight.md).

## 7. S3: can crook invent herdr-app space and tab ids?

**Yes, with the app open.**

- `hc run --label herdr.space=crook-s3 --label herdr.tab=crook-s3-a -- sh` made a space at once.
  `herdr-app ls` showed id `crook-s3`, named after its folder.
- `herdr-app do space.rename --id crook-s3 --name "crook effort s3"` worked.
- A second invented tab id gave a second tab in the same space. `tab.rename` exists (`herdr-app commands`).
- `herdr-app do space.close --id crook-s3 --confirmed true` ended both terminals.

Not tested: the app closed, then opened.

Used by [0015](../decisions/0015-spaces-from-herdr-labels.md).

## 8. S5: does `hc kill` leave a Claude session resumable?

**Yes.** After `hc kill` (exit 129), a new terminal running `claude --resume <uuid>` answered with the word
the killed session had replied earlier.

## 9. S6: hc API churn

hc 0.1.22. `hc schema` has `terminal.run` with `labels`, `outlive_parent` and `env`. Used by
[0016](../decisions/0016-hc-preflight.md).

## 10. Do directory grants prompt in every session?

| Case | Prompt? |
| --- | --- |
| `--add-dir D`, D never trusted | Yes: "Your answer holds for this session." |
| Same `--add-dir D` in a new session | Yes, again |
| Claude started in D and trusted, then `--add-dir D` and `--add-dir D/new-sub` | No |
| `--settings '{"permissions":{"additionalDirectories":["D2"]}}'`, D2 never trusted | No |

The prompt's own text: "Starting Claude Code in this directory and trusting it there lasts, and covers its
whole repository or everything below it."

Not checked: whether `additionalDirectories` without a prompt actually grants access.

Used by [0013](../decisions/0013-directory-access-through-trusted-roots.md).

## 11. Coordinator dies, children die, everything is resumed

Setup: a coordinator with a fixed session id, told a codename. Two children with fixed session ids:

- `busy`: mid-way through a shell loop writing `step 1` to `step 6`, five seconds apart;
- `asker`: had written `apple.txt`, then blocked on an AskUserQuestion.

Then `hc kill coord`:

- All three ended together (default parent lifetime).
- `busy`'s shell loop ran on for about 10 to 15 seconds after the kill and wrote two more steps (3 and 4),
  then stopped. In-flight commands are cut off part-way, not cleanly.

Then each was started again with `claude --resume <id>` in its original folder, the children with
`--parent coord` and this prompt: "You were killed by a restart and have just been resumed. Check what you
already did on disk, then finish your original task without redoing completed work."

- The coordinator remembered the codename. When the resumed `asker` blocked, the coordinator said it was
  "the same task as t13, resumed after a restart".
- `busy`'s transcript showed its command as "Interrupted". It read `steps.txt`, said "Steps 1–4 are
  already in steps.txt, so I'll append only steps 5 and 6", and did exactly that.
- `asker` saw `apple.txt` was done and asked its question again.

Caveat: the children redid nothing because the prompt told them to check the disk first, and the task's
state was easy to read from the disk. A task with hidden progress would need an explicit progress record.

## 12. Coordinator in a respawn loop

Host program: `sh -c "claude --session-id L '<first task>'; while :; do claude --resume L; done"`.

- After `/exit`, the Claude process under the loop changed (pid 62863 to 64523), running `--resume L`.
  The terminal address stayed the same (t17).
- The restarted coordinator still knew its codename.
- A child (`busy2`, an 8-step shell loop) kept running through two coordinator restarts and finished all 8
  steps.
- Sent during the restart gap: a child's exit note and an `hc send` message. Both reached the new Claude,
  merged onto one prompt line:
  `<hc>t19 ex3 exited (exit 5). Read: hc read t19</hc>Gap message: reply GAPOK if you can read this.`
  It answered `GAPOK`. Observed once; not a guarantee.
- `/exit` and Ctrl+C no longer end the coordinator: the loop restarts it. Ending it means ending the
  terminal (closing the pane or `hc kill`), which also ends its children.

## 13. hc templates, session ids and resuming

A template `worker.md` in the scratch core's `herdr-core/templates/`, with
`command: claude --model haiku --settings <hooks.json>`. The settings add a SessionStart hook that saves its
input to `roster/$HC_TERMINAL_ID.json`.

- `hc run --bg --name w1 worker "Reply with the single word TEMPLATED."` worked: `first_message.sent` was
  true, the first message was the template body followed by the task, and the child replied `TEMPLATED`.
- hc runs the template command as `sh -c '<command>'` (the terminal's argv).
- The hook wrote `t1.json` with `session_id`, `cwd`, `hook_event_name: SessionStart`, `source: startup`. The
  transcript `<session_id>.jsonl` existed.
- Extra arguments after the template name are dropped without an error:
  `hc run --bg --name w2 worker --session-id 1111… "Reply X."` kept the template's argv unchanged and got a
  new session id.
- Resume: `hc run --bg --name w1r sh -c "<template command> --resume <id> '<prompt>'"` answered from memory.
  The hook wrote `t3.json` with the same `session_id` and `source: resume`.

## 14. Child Stop hook and coordinator rescan, end to end

Throwaway hooks, not the real implementation.

- **Child Stop hook** (in the template's settings): finds its parent and name with `hc get $HC_TERMINAL_ID`,
  checksums `results/<name>.md`, and returns at once if the checksum matches its "delivered" mark. Otherwise,
  if the parent's status app is `claude-code`, it runs `hc send <parent> "Result updated: <path>"` and, only
  if that succeeds, saves the checksum as the mark.
- **Coordinator UserPromptSubmit hook**: for each result whose checksum differs from its mark, prints
  `Undelivered child result: <path>` and saves the mark.

Results:

| Case | Outcome |
| --- | --- |
| Child writes `answer: 42` | Coordinator got `<hc from="t5 wk">Result updated: …/results/wk.md</hc>`, read it, replied `answer: 42` |
| Child turn that changes nothing | Nothing sent |
| Child appends `answer: 43` while the coordinator is blocked on a question | Send refused; mark not updated |
| New coordinator prompt after that | The rescan hook's output reached the coordinator, which replied `last line: answer: 43`; mark updated |

## 15. Core stop, then the coordinator resumes its children from the roster (2026-11-27)

Stands in for a reboot: everything hc runs is lost at once. Scratch core, Haiku throughout.

Setup:

- A template `worker` whose settings add a SessionStart hook. The hook writes `roster/<hc name>.json` with
  the name, session id, folder, template and state (`active` unless already set).
- A coordinator with a fixed session id, told the codename MARMALADE.
- Three children under it: `steps` (a shell loop writing `step 1` to `step 10`, six seconds apart),
  `asker` (wrote `apple.txt`, then blocked on an AskUserQuestion), and `donekid` (replied and went `done`;
  its roster entry was then set to `done` by hand).

Then `hc server stop`, with `steps` at step 4:

- Every terminal ended at once. `hc ls --all` on the next core lists all four as `finished`,
  `reason: server_stop`, `signal: 9`.
- `steps.txt` stopped at step 4 and never moved. Unlike `hc kill` (spike 11), nothing ran on afterwards.
- No spike process was left (`pgrep` on the session ids and settings path found none).

Then the coordinator was resumed with `claude --resume <id>` and told to follow a resume file: read the
roster, check `hc ls`, and for each `active` entry with no live terminal run the template command plus
`--resume <session id>` through `sh -c`, as its own child.

- It remembered MARMALADE.
- It resumed `steps` and `asker`, and skipped `donekid` because its state was `done`.
- `steps` saw its command as "Interrupted", found steps 1 to 4 on disk, appended 5 to 10 and replied
  `STEPS DONE`. The file ended with exactly ten lines.
- `asker` saw `apple.txt`, did not rewrite it, and asked its question again.
- The new coordinator got `<hc>t6 asker-r is blocked (question). See: hc read t6</hc>`: parent notes work
  from resumed children.
- No trust prompts on resume: the folders had been trusted on the first start.

Other findings:

- Names of finished terminals can be used again after a core restart (`hc run --name coord` and
  `--name donekid` both succeeded). The spike resumed children as `asker-r` and `steps-r`, which made the
  name-keyed roster hook write duplicate entries. Resume under the original name.
- The coordinator's prompts never reached Claude, on first start or on resume. Cause, found in spike 16: the
  prompt came right after `--allowedTools "Bash Read"`, which takes several values and swallowed it.

Not covered: what hc and herdr-app restore when the maintainer logs in after a real reboot.

## 16. Roles as Claude Code agents from crook's plugin (2026-11-27)

hc reads templates only from `~/.config/herdr-core/templates` (`hc templates --help`); `hc run --help` has
no option to change it. This spike tests the alternative: a throwaway plugin with
`agents/tester.md` (`model: haiku`, body "Begin every reply with the exact token ROLE-OK"), launched as:

```sh
hc run --bg --name <n> --cwd <dir> \
  claude --plugin-dir <plugin> --agent tester --session-id <uuid> "Reply with the word PINEAPPLE."
```

| Case | Result |
| --- | --- |
| `--agent tester`, trusted folder | Haiku; replied `ROLE-OK PINEAPPLE`; `<uuid>.jsonl` exists |
| `--agent crookspk:tester` (plugin-qualified name) | Same |
| Control: `--plugin-dir` but no `--agent` | Opus (the default); replied `PINEAPPLE`, no token |
| `--agent tester --resume <uuid> "What word did you reply with before? Then say BANANA."` | `ROLE-OK My previous reply was PINEAPPLE. BANANA`, on Haiku |
| `--agent tester`, folder never trusted | Blocked on the trust dialog; after it was answered, the prompt ran: `ROLE-OK PINEAPPLE` |
| `claude --model haiku --allowedTools "Bash Read" "Reply with the word PINEAPPLE."` | Prompt never ran; Claude sat idle |

`first_message` is null for plain launches: hc does not track a prompt passed on Claude's command line.

## 17. A herdr mod that carries crook's plugin (2026-11-27)

herdr-app 0.1.18. A scratch mod `mods/crookspk/` under a scratch `HERDR_APP_HOME`, holding `mod.json`,
`main.js` and a `plugin/` folder (the spike 16 plugin). `init()` logged `appHome()`, `home()`, `checking()`
and `import.meta.url`, and wrote them to a file.

| Check | Result |
| --- | --- |
| `herdr-app check <mod>` with `plugin/` inside the mod folder | `check passed` |
| `appHome()`, no `HERDR_APP_HOME` | `~/.config/herdr-app` |
| `appHome()`, `HERDR_APP_HOME=<scratch>` | `<scratch>` |
| `import.meta.url` | `undefined` |
| `herdr.d.ts`: an API for the mod's own name or folder | None found |
| `claude -p --plugin-dir <mod>/plugin --agent tester "Reply with the word PINEAPPLE."` | `ROLE-OK PINEAPPLE` |

So a mod cannot learn its own folder name at run time. herdr's install rules say a renamed mod has "the
mod's own `mods/<name>/` paths" rewritten too, so a literal `mods/crook/plugin` in `main.js` follows a
rename. That rewrite is done by the installing agent, from the docs; not tested.

Not tested: the mod loaded live in the maintainer's real app; the warning `check` printed about a stranded
async task came from the spike's own file write.

## 18. crook installed by mise through packslip (2026-11-27)

mise 2026.11.14. The maintainer will install crook globally with mise's `packslip:` backend.

### What mise keeps from a release

- mise's packslip backend unpacks the whole archive into a per-version install folder, not just the
  executable (`install_artifact(…, ArchiveLayout::Declared, …)` in
  [`src/backend/packslip.rs`](https://github.com/jdx/mise/blob/HEAD/src/backend/packslip.rs)). A lone
  top-level folder in the archive is stripped.
- `packslip:github.com/jdx/hk@latest` installed into a scratch `MISE_DATA_DIR`. The hk tarball holds `hk`
  and `skills/`; the install folder `installs/packslip-github-com-jdx-hk/2.14.0/` held the same, plus mise's
  own `.mise-*` files.
- mise links `latest`, `2` and `2.14` to `2.14.0` in the tool's install folder.
- `mise upgrade` removes the version it upgraded from after `upgrade.prune_after` (24h here), or at once
  with `--prune`.

### PATH seen by a herdr-app mod

- The running herdr-app process has `PATH=/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin`.
- herdr-app's docs say Finder launches load the login shell's PATH for `exec`.
- The maintainer's login shell is fish. `fish -l` from an empty environment finds mise-installed tools
  (`jq` resolved to `~/.local/share/mise/installs/jq/1.8.2/jq`) and has the mise shims folder on PATH.
- Not tested: `exec("crook", …)` from a mod in the real app.

### Plugin folders that move or disappear

A fake mise layout: `1.0.0/plugin`, `1.1.0/plugin`, then `1.2.0/plugin`, and a `latest` link. Each plugin
has a `tester` agent ("Begin every reply with ROLE-<version>") and a Stop hook script at
`${CLAUDE_PLUGIN_ROOT}/hooks/stop.sh` that logs its version.

| Case | Result |
| --- | --- |
| `claude -p --plugin-dir <latest>/plugin` (link to 1.0.0) | `ROLE-1.0.0 PINEAPPLE`; the 1.0.0 hook ran |
| Live session started on `<latest>/plugin`; `latest` moved to 1.1.0 and 1.0.0 deleted; next prompt | Replied `ROLE-1.0.0 TWO` (agent text is read at start); the Stop hook ran 1.1.0's script |
| Live session started on `1.1.0/plugin` directly; 1.1.0 deleted; next prompt | Replied, then `Stop hook failed` |
| The first session resumed with `--plugin-dir <latest>/plugin` (now 1.2.0) after 1.0.0 was gone | Remembered ONE and TWO; the 1.2.0 hook ran. It still began replies with `ROLE-1.0.0`, likely copying its own history |

So a session must load the plugin through a path that survives a version being removed. A live session
can run an old agent text next to a newer version's hooks.

## 19. Picking up a new plugin version in a live session (2026-11-27)

Claude Code 2.1.332. The fake mise layout from spike 18, with `latest` moved to a new version and the old
folder deleted while a session runs. Each version also has a `crookver` skill that says "The crook version
is <version>".

| Case | Result |
| --- | --- |
| `/reload-plugins` typed into a live session (`hc send <t> "/reload-plugins"`) | `Reloaded: 22 plugins · 185 skills · 32 agents · 48 hooks …` |
| Prompt after the reload, told to use the token its current instructions give | Still `ROLE-1.2.0`: the `--agent` role text did not change |
| Kill, then `--resume` with `--plugin-dir` pointing at the new version | Still `ROLE-1.3.0`. The transcript holds only the old role text (`agentSetting: crookspk:tester`); resume reuses it |
| The session runs `hc send "$HC_TERMINAL_ID" "/reload-plugins"` itself | Accepted while it was working; `/reload-plugins` ran as soon as its turn ended |
| The `crookver` skill after that reload | `VERSION 1.5.0`; the old version's skill text was 1.4.0 |

So `/reload-plugins` brings in new skills and hooks, and a session can trigger it on itself. A session's
role text (`--agent`) is fixed for the session's whole life, resumes included.

`/reload-plugins` also reported "1 error during load"; not looked into, the spike plugin loaded.

## 20. A hook reloads its own session after an upgrade (2026-11-27)

Same fake layout, versions 2.0.0 and 2.1.0, with `latest` as the link. Each plugin has a `crookver` skill and
one hook script, `${CLAUDE_PLUGIN_ROOT}/hooks/version.sh`:

- on SessionStart it saves the plugin's version for the session id;
- on Stop it reads the version again through `${CLAUDE_PLUGIN_ROOT}`; if it differs, it runs
  `hc send "$HC_TERMINAL_ID" "/reload-plugins"` and, if that succeeds, saves the new version.

Hook log and screen:

| Time | Event |
| --- | --- |
| 10:18:40 | `start 2.0.0`; the skill answered `VERSION 2.0.0` |
| (then) | `latest` moved to 2.1.0, 2.0.0 deleted, prompt "Reply with the word TWO." |
| 10:18:52 | `check had=2.0.0 now=2.1.0`, `sent rc=0 {"started":true,"status":"working","turn":1}` |
| (then) | `/reload-plugins` ran in the session: `Reloaded: … 49 hooks …` |
| 10:19:12 | Next turn: the skill answered `VERSION 2.1.0`; `check had=2.1.0 now=2.1.0`, nothing sent |

The model was not involved. The send from inside the Stop hook did not block the hook. `/reload-plugins` did
not fire SessionStart again. The turn running when the upgrade lands still uses the old skills; the reload
applies from the next turn.

## 21. `exec` from a mod in the maintainer's real herdr-app (2026-11-27)

herdr-app 0.1.18, started by launchd (parent pid 1), as from the Finder or Dock. A throwaway overlay mod
`mods/crookspk-probe/` passed `herdr-app check`, was opened with `herdr-app do mod.open --name crookspk-probe`,
and wrote what its `exec` calls returned to a scratch file. Then it was closed and its folder, and the empty
`data/crookspk-probe/` the app made for it, were deleted. `herdr-app status` reported no errors afterwards.

| Call | Result |
| --- | --- |
| `exec("sh", ["-c", "command -v jq; command -v mise; command -v claude; command -v hc"])` | `~/.local/share/mise/installs/jq/1.8.2/jq`, `/opt/homebrew/bin/mise`, `~/.local/bin/claude`, `~/.local/bin/hc` |
| `exec("jq", ["--version"])` (a global mise tool) | `jq-1.8.2` |
| `exec("mise", ["--version"])` | `2026.11.14 macos-arm64 (2026-11-24)` |
| `PATH` seen by `exec` | the mise global install folders and `~/.local/share/mise/shims` |

So a `crook` installed with `mise use -g` will be found by the mod's `exec("crook", …)`, as long as the
login shell activates mise, as the maintainer's does.

## 22. `PATH` in a terminal of the maintainer's real hc core (2026-11-27)

One throwaway terminal in the real core (herdr-app's), started from a stripped environment
(`env -i HOME USER TMPDIR PATH=/usr/bin:/bin`) so the caller's `PATH` could not leak in:

```sh
hc run --bg --no-parent --no-notify --name crook-pathcheck --cwd ~ -- env
```

It ran `env` directly, as `hc run … claude` runs `claude`, and was removed with `hc rm` after reading.

- `PATH` was the core's, not the caller's: 67 entries, the mise global install folders first (for example
  `~/.local/share/mise/installs/ast-grep/0.51.3`), then `~/.local/share/mise/shims` at entry 47, then
  `~/.local/bin` and Homebrew.
- `SHELL` was `/opt/homebrew/bin/fish`; `HC_*` were the new terminal's own.

So a Claude session started by `hc run`, and the hooks it runs, find a `crook` installed with `mise use -g`:
through its install folder if it was installed when the core started, through the shims otherwise.

The install folders are fixed when the core starts. After an upgrade, the old version's folder stays ahead of
the shims in this `PATH` until mise prunes it (24 hours, spike 18) or the core restarts. Until then, hooks in
hc sessions run the old `crook`, while the mod and new shells run the new one. [inferred from the `PATH`
order; not run with two crook versions]

Follow-up: a shim is not tied to the `PATH` order. `~/.local/share/mise/shims/ast-grep --version` ran the
global version (`ast-grep 0.51.3`, rc 0) from a folder whose `mise.toml` was never trusted. Calling the shim
by its full path avoids the stale-folder problem above. Used by [0019](../decisions/0019-ship-crook-as-a-binary.md).

## 23. Passing `CROOK_BIN` to children, and a core started without Claude's variables (2026-11-27)

### `mise link` for a dev build

In a scratch mise (`MISE_DATA_DIR` and friends under `$TMPDIR`):

| Step | Result |
| --- | --- |
| `mise link packslip:github.com/s0undt3ch/shepherds-crook@dev <dev folder>` | Linked; `mise ls` shows `dev (symlink)` |
| `mise x -- crook`, `mise which crook` | Both found and ran the dev build |
| `mise reshim --force` | No `crook` shim, with the binary in the folder's root or in `bin/` |
| Control: `mise link jq@dev` with a fake `bin/jq`, same reshim | `jq` shim created |

So the shim cannot point at a dev build. Why only packslip versions get no shim is not known.

### A core started without Claude's variables

A scratch core (`HC_SESSION=cenv`), started with `hc server start` from inside this Claude session with the
[0020](../decisions/0020-mechanical-steps-run-through-crook.md) list removed (`CLAUDECODE`, `CLAUDE_PID`,
`CLAUDE_EFFORT`, `CLAUDE_CODE_SESSION_*`, `CLAUDE_CODE_CHILD_*`, `CLAUDE_CODE_MESSAGING_*`,
`CLAUDE_CODE_ENTRYPOINT`, `CLAUDE_CODE_EXECPATH`):

- A child started by a cleaned `hc run` had only `CLAUDE_CONFIG_DIR` and four user settings
  (`CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY`, `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS`, `CLAUDE_CODE_ENABLE_TELEMETRY`,
  `CLAUDE_CODE_SUBAGENT_MODEL`).
- A child started next by an `hc run` that still had every variable got the same, clean set.
- Three Claude children showed no "Transcript saving is off" warning.

A first attempt that forgot to remove the variables from `hc server start` gave every child the marker and
the warning: the call that starts the core decides the environment of its terminals (the core's `hold`
process carried the caller's variables).

### `CROOK_BIN` to a child's hooks

A throwaway plugin whose Stop hook ran `"${CROOK_BIN:-$HOME/.local/share/mise/shims/crook}" hook stop` and
logged the path it chose. `CROOK_BIN` pointed at a fake dev `crook` that logged its arguments.

| Child | `CROOK_BIN` in its hook |
| --- | --- |
| `hc run … -- env CROOK_BIN=<dev> claude …` | Set; the dev `crook` ran with `hook stop` |
| `CROOK_BIN=<dev> hc run … -- claude …` (caller's environment only) | Unset; the hook fell back to the shim path |
| A parent terminal started with `env CROOK_BIN=<dev>`, which ran `hc run … -- env CROOK_BIN="$CROOK_BIN" claude …` | Set; the dev `crook` ran |

All three replied with their word. `hc run` has no option for one variable; the API's `terminal.run` takes an
`env` map (`hc schema`), but the `env` prefix in the argv is enough.

## 24. An effort's tree in the maintainer's real herdr-app (2026-11-27)

Three Haiku sessions in the real core, folders under `$TMPDIR/crook-e2e-tree.*`:

- `crookt-coord`: `--no-parent`, `herdr.space=crook-treetest`, `herdr.tab=crook-treetest-coord`;
- `crookt-kid1`: `--parent crookt-coord`, no `herdr.*` labels;
- `crookt-kid2`: `--parent crookt-coord`, `herdr.space=crook-treetest`, `herdr.tab=crook-treetest-kid2`.

Seen with `herdr-app shot` and `herdr-app ls`:

- The Agents sidebar drew `coord` with `crookt-kid1` and `crookt-kid2` under it, with tree lines, whether or
  not the child had a tab. The tree comes from `parent` alone (`mods/agents/main.js:173-183`).
- The space was named after the coordinator's folder (`coord`), not the effort; it needs `space.rename`
  ([0015](../decisions/0015-spaces-from-herdr-labels.md)).
- Tabs were named from each session's title ("COORD reply request", "KID2 reply request").
- `crookt-kid2` got its own tab at once. `crookt-kid1` had none.
- Clicking a sub-agent row with no pane peeks at it (`mods/agents/main.js:202`): an overlay with the session
  and a "keep" button. `terminal.keep` (the button) gave `crookt-kid1` a tab named `crookt-kid1` in the same
  space, and hc recorded `herdr.space` and `herdr.tab` labels on it, so the tab stays.
- The trust dialog in each child reached the coordinator as `<hc>t17 crookt-kid1 is blocked (permission)…</hc>`.

`space.close` ended all three; they were then removed with `hc rm`.
