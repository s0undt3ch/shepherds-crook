# Spike 9: can a mod run `crook` through `$.process.run`?

Claude Code 2.1.329, Haiku, `crook mod ping` (release build of HEAD 059261a) with
`CROOK_PROJECT=spike9 CROOK_ROLE=child CROOK_CHILD=c1`, a throwaway
`XDG_DATA_HOME`, and a hand-written `--settings` file that denies `Read` and
`Edit` on that data dir and carries a `PreToolUse` command hook that logs `cwd`.
The probe mod is `plugin/hooks/probe.ts`; `run/setup.sh` and `run/launch.sh`
reproduce the runs; `mkdb/` is the throwaway that creates the database and `gen`.

Three sessions: an interactive herdr pane (launched from a shell with no
`CLAUDECODE`), the same pane launched with `CLAUDECODE=1` forced in its
environment, and `claude --bg`, started from a shell that had `CLAUDECODE=1`.

## Results

| Question | Answer |
| --- | --- |
| `ping` from `prompt.submit` | Pass (pane, bg) |
| `ping` from a `tool.run` hook on tool `probe` | Pass (pane, bg) |
| `ping` from a `$.clock.every(2000, …)` tick | Pass (pane, bg; 6 ticks each) |
| `CLAUDECODE` in the mod's process | Unset in the pane and in `--bg`. Set to `1` only when `claude` itself was started with `CLAUDECODE=1` (the forced-env pane run); then every `ping` from every call site was refused with `refused: mod verbs cannot run from a Claude Code shell (CLAUDECODE=1)` |
| `$.process.run` `env` | Merges. With `env: { SPIKE_PROBE: "1" }`, `/usr/bin/env` printed the inherited `CROOK_*`, `PATH`, `HOME`, `XDG_DATA_HOME` plus `SPIKE_PROBE=1`. With `env: { CLAUDECODE: "" }`, `ping` passed, so `CROOK_PROJECT` still reached `crook`; this also fixed the forced `CLAUDECODE=1` run |
| `$.fs.stat(<data dir>/spike9/gen)` under the Read/Edit deny | Pass from all three call sites (`kind: file`, size 1). The deny was in force: the model's `Read` of the same file failed with "File is in a directory that is denied by your permission settings" |
| Hook `cwd` follows the Bash shell | Yes, in the pane and in `--bg`. After `cd sub` (a subdirectory of the session's working directory) the next Bash call's `PreToolUse` `cwd` was `<work>/sub`, and `pwd` printed `<work>/sub` |
| Latency of a `ping` through `$.process.run` | p50 11 ms, max 70 ms over 28 passing calls. By site: `prompt.submit` p50 56 ms, max 70 (n=7; includes the adapter's first env fetches); `tool.run` p50 12 ms, max 21 (n=3); `clock.every` p50 8 ms, max 17 (n=18) |
| Sessions | All stopped and removed (`claude stop`, `claude rm`); the pane was closed |

## Notes

- A first `--bg` run had both Bash calls in one assistant turn, so both hooks
  ran before the `cd` took effect and the second `cwd` was the old one. That
  was the model batching, not a failure; the rerun that forced one call per turn
  showed the `cwd` following.
- A mod may hand `$` only to top-level function declarations of its own file
  (`$ is passed to "log", which is not a function declared at the top of this
  file`); a helper declared inside `register` stops the module loading. The real
  mod's `ports($)` is imported from a file of the plugin's own, which is allowed.
- `claude --bg` did not pass `CLAUDECODE` on to the mod even when the launching
  shell had it, so the fallback matters for an interactive launch from inside a
  Claude Code shell, not for `--bg`.

## Gate

No call site failed, `stat` on `gen` was not denied, and the hook `cwd`
follows the shell. The gate passes.

`CLAUDECODE=1` was not set in the mod's process in the ordinary runs, so the
Step 4 change to `adapter.ts` was not applied. It is cheap and safe (`env`
merges): `run` would pass `env: { CLAUDECODE: "" }`. It would matter only when
`crook open` is run from a shell that carries `CLAUDECODE=1`.
