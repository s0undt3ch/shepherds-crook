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
