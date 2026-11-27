# hc 0.1.22 fixtures

Real output of `hc` 0.1.22, captured on 2026-11-27 from a scratch core. Each call has up to three files:
`<name>.stdout`, `<name>.stderr` (left out when empty) and `<name>.exit`. The unit tests in `src/hc/client/` parse
them, and `tests/preflight.rs` replays two of them from a fake `hc` script.

## How they were captured

A bash wrapper ran `hc` through `env` with:

- `HC_SESSION=crook-cap-<pid>`, so the core is a scratch one;
- `XDG_STATE_HOME`, `XDG_CONFIG_HOME` and `XDG_DATA_HOME` under `$TMPDIR/crook-e2e-cap.XXXX`;
- `XDG_RUNTIME_DIR=/tmp/hcc.XXXX`, short because the socket path has a length limit;
- `-u` for `HC_TERMINAL_ID`, `HC_TERMINAL_SOCKET`, `HC_SCOPE`, `CLAUDECODE`, `CLAUDE_PID`, `CLAUDE_EFFORT`,
  `CLAUDE_CODE_ENTRYPOINT`, `CLAUDE_CODE_EXECPATH` and every `CLAUDE_CODE_SESSION_*`, `CLAUDE_CODE_CHILD_*` and
  `CLAUDE_CODE_MESSAGING_*`. The list was built as a bash array, not a string, because zsh does not word-split (spike 23).

The programs were `sh -c 'sleep 30'`, `sh -c 'exit 3'` and a `printf` of an OSC 7501 record, never Claude. The blocked
terminal is that record: `printf '\033]7501;:state=blocked:kind=question:app=claude\007'`. The core was stopped and its
folders deleted afterwards.

## Cleaned

The values are illustrative: paths are `/tmp/crook-e2e-fixture`, pids are `4242`, the `*_ms` times are
`1700000000000`, and the core id and host name are placeholders. JSON is re-serialised with sorted keys.

## What each file is

| Fixture | Call |
| --- | --- |
| `version` | `hc --version` |
| `schema` | `hc schema` |
| `server_start_cold`, `server_start_warm` | `hc server start --json`, first and again against the running core |
| `server_status`, `server_stop`, `server_stop_again` | `hc server status --json`, `hc server stop --json` (stopped, then not running) |
| `ls_empty`, `ls`, `ls_all`, `ls_label`, `ls_after_kill`, `ls_all_reuse` | `hc ls --json` with `--all` or `--label` |
| `run_labels`, `run_blocked`, `run_finished`, `run_name_dot`, `run_reuse_name` | `hc run --bg --json …` that worked |
| `run_name_long`, `run_name_id`, `run_name_taken`, `run_missing_cwd` | `hc run --bg --json …` that failed |
| `get_running`, `get_blocked`, `get_finished`, `get_killed`, `get_dot`, `get_missing` | `hc get <t> --json` |
| `send_sent`, `send_blocked`, `send_finished`, `send_missing` | `hc send <t> hello --json` |
| `kill`, `kill_finished`, `kill_missing` | `hc kill <t> --json` |

## What the captures showed

- Exit codes of `hc send` (spike 2 confirmed): live 0, blocked 9, finished 11, missing 3. The error is JSON on stderr
  with a `kind`. `hc get` and `hc kill` of a missing terminal also exit 3 (`not_found`); a bad name exits 6
  (`invalid_params`); a taken name or a failed spawn exits 1; a usage error exits 2.
- `hc kill` of a live or an already finished terminal exits 0 and prints the terminal as `finished`
  (`reason: killed`, `signal: 1`). Killing a parent kills its children (`reason: killed`).
- **Names (Q3):** hc accepts `.` in a name (`demo.child`). Its own error text gives the rule: 1 to 32 of
  `a-z 0-9 - _ .`, starting with a letter or digit, and not shaped like an address (`t7`). `hc run --help` lists the
  characters without the dot. A 33 character name and `t7` are refused (exit 6).
- **`hc ls --json` has no labels.** Its entries are `address`, `agent`, `cwd`, `name`, `parent`, `state`, `status`,
  `turn`, and for finished ones `reason`, `signal` and `exit_code`. There `status` is a bare word (`"blocked"`). Labels
  and the status record (`state`, `kind`, `app`) come only from `hc get`. `hc ls --label k=v` does filter.
- **Every hc command except `--version` starts a core when none is running**, with the caller's environment:
  `hc schema`, `hc ls` and even `hc server status` did, and `hc server stop` on a stopped core prints
  `{"stopped":false}`. So `hc server start` with the scrubbed environment has to come before any other core call.
  Against a running core `hc server start` is harmless: it prints the same status and keeps the pid.
- `hc send` reads text that starts with `-` as a flag unless it follows `--` (`hc send --json -- <t> --force`).
- `hc run --json` prints `address`, `agent`, `cwd`, `name`, `state` and `turn`, not the full record.
- A name can be reused once its terminal has finished (`ls_all_reuse` lists `worker-a` twice, `t1` finished and `t6`
  live).
