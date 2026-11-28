#!/usr/bin/env bash
# tier2.sh [--keep] [--only S1,S2,...] [--binary <crook>]: the tier 2 scenarios (release plan, "E2E harness"):
# a real scratch hc core, a stub `claude`, a stub `herdr-app`. LOCAL ONLY (Q34): hc and herdr-app cannot run on CI.
#
# Every hc and crook call goes through hc.sh and crook.sh, which give it the run's own core and folders. Run this
# script with bash, never type a raw `hc` or `crook` in a shell that sits inside a real hc core.
#
# Scenarios: S1 core start with the scrub, S2 new and open, S3 spawn, S6 core stop and resume, S8 worktree
# rules, S9 model change on resume (argv), S10 stop and close, S13 mod install and upgrade. Each uses its own
# flock, so --only picks any of them. The run is removed at the end, and on any exit, unless --keep is given.
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
source "$here/lib.sh"

keep=0
only=''
binary="$here/../../target/release/crook"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --keep) keep=1 ;;
    --only) only="${2:-}"; shift ;;
    --binary) binary="${2:-}"; shift ;;
    *) die "usage: tier2.sh [--keep] [--only S1,S2,...] [--binary <crook>]" ;;
  esac
  shift
done
for tool in hc jq git lsof toolr; do command -v "$tool" >/dev/null || die "$tool is needed and not on PATH"; done
[ -x "$binary" ] || die "no crook binary at $binary; run cargo build --release, or pass --binary"
binary="$(cd "$(dirname "$binary")" && pwd -P)/$(basename "$binary")"

VERSION=1.2.0
skip='RECIPE.md preview.png jsconfig.json gpui-kit.d.ts'
ENVF="$("$BASH" "$here/setup.sh" --tier 2)" || die "setup failed"
cleanup() {
  trap - EXIT INT TERM
  if [ "$keep" = 1 ]; then "$BASH" "$here/teardown.sh" "$ENVF" --keep; else "$BASH" "$here/teardown.sh" "$ENVF"; fi
  local rc=$?
  [ "$rc" = 0 ] || echo "tier2: teardown left the run in place; see above" >&2
}
trap cleanup EXIT
trap 'exit 130' INT TERM
load_run "$ENVF"
echo "run: $RUN"
"$BASH" "$here/install.sh" "$ENVF" "$VERSION" "$binary" >/dev/null || die "install failed"

hc() { "$BASH" "$here/hc.sh" "$ENVF" "$@"; }
crook() { "$BASH" "$here/crook.sh" "$ENVF" "$@"; }
calls="$RUN/claude-calls"
PASS=0
FAIL=0
NOTE=0

ok() { PASS=$((PASS + 1)); echo "  PASS $*"; }
bad() { FAIL=$((FAIL + 1)); echo "  FAIL $*"; }
note() { NOTE=$((NOTE + 1)); echo "  note $*"; }
# expect <description> <command...>: passes when the command succeeds.
expect() {
  local desc="$1"
  shift
  if "$@" >/dev/null 2>&1; then ok "$desc"; else bad "$desc"; fi
}
# refuse <description> <command...>: passes when the command fails.
refuse() {
  local desc="$1"
  shift
  if "$@" >/dev/null 2>&1; then bad "$desc"; else ok "$desc"; fi
}
want() { [ -z "$only" ] || case ",$only," in *",$1,"*) return 0 ;; *) return 1 ;; esac; }
# wait_for <seconds> <command...>: polls twice a second.
wait_for() {
  local limit="$1" i=0
  shift
  while [ "$i" -lt $((limit * 2)) ]; do
    "$@" >/dev/null 2>&1 && return 0
    sleep 0.5
    i=$((i + 1))
  done
  return 1
}

# git, only inside the run.
g() {
  local dir="$1"
  shift
  in_run_path "$dir" || die "refusing git in $dir, outside the run"
  (
    # A GIT_DIR the caller exported must not point git at the caller's repository.
    unset_git_env
    export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
    git -C "$dir" -c user.name=e2e -c user.email=e2e@example.invalid -c commit.gpgsign=false "$@"
  )
}

flock_dir() { find "$DATA/crook/flocks" -maxdepth 1 -type d -name "*-$1" 2>/dev/null | head -1; }
launch_count() { find "$calls" -name "*.argv" 2>/dev/null | wc -l | tr -d " "; }
lines() { wc -l <"$1" | tr -d " "; }
roster() { echo "$(flock_dir "$1")/roster.json"; }
# A terminal that is live and has reported itself as claude.
live_claude() { hc ls --json | jq -e --arg n "$1" '.terminals[] | select(.name == $n and .state == "live" and .agent == "claude")'; }
# A failing hc is a failing check: the listing is read first, and only then searched.
not_live() {
  local out
  out="$(hc ls --json)" || return 1
  ! printf '%s' "$out" | jq -e --arg n "$1" '.terminals[] | select(.name == $n)' >/dev/null
}
no_herdr_labels() {
  local out
  out="$(hc get "$1" --json)" || return 1
  [ -z "$(printf '%s' "$out" | jq -r '.labels | keys[] | select(startswith("herdr."))')" ]
}
no_resumed_names() {
  local out
  out="$(hc ls --json)" || return 1
  [ -z "$(printf '%s' "$out" | jq -r '.terminals[].name | select(test("-r$"))')" ]
}
no_sockets() { [ -z "$(find "$RT" -type s 2>/dev/null)" ]; }
no_child() { jq -e --arg c "$2" '[.children[] | select(.name == $c)] | length == 0' "$(roster "$1")"; }
no_branch() {
  g "$REPO" rev-parse --git-dir >/dev/null || return 1
  ! g "$REPO" show-ref --verify --quiet "refs/heads/$1"
}
# The names a launched session saw (its .env file) hold none of ADR 0020's, but hc's own three.
env_names_clean() {
  local name bad=0
  while IFS= read -r name; do
    case "$name" in HC_TERMINAL_ID | HC_TERMINAL_SOCKET | HC_SCOPE) continue ;; esac
    if adr0020_name "$name"; then echo "$name in $1" >&2; bad=1; fi
  done <"$1"
  return "$bad"
}
child_field() { jq -r --arg c "$2" --arg f "$3" '.children[] | select(.name == $c) | .[$f]' "$(roster "$1")"; }
term_status() { hc get "$1" --json | jq -r '.status.state // empty'; }
status_is() { [ "$(term_status "$1")" = "$2" ]; }
parent_addr() { hc get "$1" --json | jq -r '.parent // empty'; }
label_of() { hc get "$1" --json | jq -r --arg k "$2" '.labels[$k] // empty'; }
# The launch of a session: the newest argv file that names it. Arguments are separated by the byte 0x1f.
launch_of() { grep -l "$1" "$calls"/*.argv 2>/dev/null | LC_ALL=C sort | tail -1; }
SEP=$'\037'
argv_flat() { printf '%s%s' "$SEP" "$(cat "$1")"; }
argv_has() {
  local flat needle="$SEP" f="$1"
  shift
  flat="$(argv_flat "$f")"
  for a in "$@"; do needle="$needle$a$SEP"; done
  [[ "$flat" == *"$needle"* ]]
}
argv_before() {
  local flat before_a before_b
  flat="$(argv_flat "$1")"
  before_a="${flat%%"$SEP$2$SEP"*}"
  before_b="${flat%%"$SEP$3$SEP"*}"
  [ "${#before_a}" -lt "${#before_b}" ] && [ "$before_a" != "$flat" ]
}
argv_last() {
  local flat
  flat="$(argv_flat "$1")"
  flat="${flat%"$SEP"}"
  printf '%s' "${flat##*"$SEP"}"
}
set_state() { printf '%s' "$2" >"$calls/$1.state"; }
# The session id of a flock's coordinator or child, from the roster.
coord_sid() { jq -r '.coordinator.session_id' "$(roster "$1")"; }
child_sid() { child_field "$1" "$2" session_id; }
start_flock() {
  crook new "$1" >/dev/null 2>&1 && wait_for 20 live_claude "$1"
}
spawn() { # spawn <flock> <child> [crook spawn options...]
  local f="$1" c="$2"
  shift 2
  local dir
  dir="$(flock_dir "$f")"
  mkdir -p "$dir/briefs"
  echo "e2e brief for $c" >"$dir/briefs/$c.md"
  crook spawn child "$c" "briefs/$c.md" --flock "$f" "$@" && wait_for 20 live_claude "$f.$c"
}
# The files `crook setup` installs from a mod folder: all but dot files and the recipe's extras (D74).
mod_files() {
  local f s
  (cd "$1" && find . -type f) | sed 's|^\./||' | while IFS= read -r f; do
    case "$f" in
      .* | */.*) continue ;;
    esac
    for s in $skip; do
      if [ "$f" = "$s" ]; then continue 2; fi
    done
    echo "$f"
  done
}
tree_sum() {
  (cd "$1" && find . -type f | LC_ALL=C sort | while IFS= read -r f; do printf '%s %s\n' "$f" "$(cksum <"$f")"; done) | cksum
}
run_timeout() {
  local limit="$1" pid watcher rc
  shift
  "$@" &
  pid=$!
  (sleep "$limit"; kill "$pid" 2>/dev/null) &
  watcher=$!
  wait "$pid"
  rc=$?
  kill "$watcher" 2>/dev/null
  wait "$watcher" 2>/dev/null
  return "$rc"
}
# What the committed schema asks of a document: its required keys and, as additionalProperties is false, no more.
shape_ok() {
  jq -e --slurpfile s "$REPO_ROOT/schema/roster.schema.json" '
    def exact($req; $o): ($req - ($o | keys)) == [] and (($o | keys) - $req) == [];
    $s[0] as $sch
    | exact($sch.required; .)
    and exact($sch.properties.coordinator.required; .coordinator)
    and ([.children[] | exact($sch.properties.children.items.required; .)] | all)
    and .version == 1' "$1"
}
REPO_ROOT="$(cd "$here/../.." && pwd)"

# ---------------------------------------------------------------------------------------------------------------
if want S1; then
  echo "S1 core start with the scrub (0016, 0020; spike 23)"
  expect "no core runs for the run's HC_SESSION before crook starts one" test -z "$(find "$RT" -type s)"
  if "$BASH" "$here/core_env.sh" "$ENVF" | sed 's/^/    /'; then
    ok "crook starts the core, and a probe terminal's env has none of 0020's variables"
  else
    bad "core_env.sh failed"
  fi
  expect "the core answers for the run's session" hc server status --json
fi

# ---------------------------------------------------------------------------------------------------------------
if want S2; then
  echo "S2 new and open (0004, 0015, 0018, 0020, 0021, 0024)"
  if crook new s2 --task "e2e task" >/dev/null 2>&1; then ok "crook new s2 starts the flock"; else bad "crook new s2"; fi
  wait_for 20 live_claude s2 || bad "the coordinator never showed as claude in hc ls"
  refuse "a second new with the same slug is refused" crook new s2
  r="$(roster s2)"
  expect "roster.json has the shape the schema asks for" shape_ok "$r"
  expect "roster.json says started after the first launch" jq -e '.coordinator.started == true and .coordinator.state == "active"' "$r"
  sid="$(coord_sid s2)"
  launch="$(launch_of "$sid")"
  fd="$(flock_dir s2)"
  if [ -n "$launch" ]; then
    expect "the coordinator's argv has --add-dir <flock folder>" argv_has "$launch" --add-dir "$fd"
    expect "the coordinator's argv has --add-dir <notes root>/<date>-<slug>" argv_has "$launch" --add-dir "$RUN/notes/$(basename "$fd")"
    expect "--settings comes before --agent crook:coordinator" argv_before "$launch" --settings --agent
    expect "the argv has --agent crook:coordinator --model haiku (the configured coordinator_model)" \
      argv_has "$launch" --agent crook:coordinator --model haiku --session-id "$sid"
    expect "the plugin dir is <data>/crook/current/plugin" argv_has "$launch" --plugin-dir "$DATA/crook/current/plugin"
  else
    bad "the stub claude logged no launch for the coordinator's session $sid"
  fi
  for k in crook.flock crook.version herdr.space herdr.tab; do
    expect "hc get s2 has the label $k" test -n "$(label_of s2 "$k")"
  done
  expect "the stub herdr-app got the space and tab renames, not the real app" grep -q 'tab.rename' "$RUN/herdr-app.log"
  before="$(lines "$RUN/herdr-app.log")"
  ncalls="$(launch_count)"
  out="$(crook open s2 2>&1)"
  case "$out" in *"already open"*) ok "crook open on a live coordinator says so" ;; *) bad "crook open on a live coordinator: $out" ;; esac
  expect "crook open on a live coordinator starts no second claude" test "$(launch_count)" = "$ncalls"
  expect "crook open on a live coordinator redoes the renames" test "$(lines "$RUN/herdr-app.log")" -gt "$before"
  : >"$RUN/herdr-app.fail"
  out="$(crook open s2 2>&1)"
  rc=$?
  rm -f "$RUN/herdr-app.fail"
  expect "crook open exits 0 when the herdr-app call fails" test "$rc" = 0
  case "$out" in *warning*) ok "and prints a warning" ;; *) bad "no warning when herdr-app fails: $out" ;; esac
fi

# ---------------------------------------------------------------------------------------------------------------
if want S3; then
  echo "S3 spawn (0007, 0013, 0020, 0021, 0025; Q3, Q11, Q14, Q15)"
  start_flock s3 || bad "the s3 flock did not start"
  spawn s3 fix-bug --repo "$REPO" --model haiku || bad "spawn fix-bug"
  sid="$(child_sid s3 fix-bug)"
  expect "the roster entry has name, model haiku, a session id and the brief" \
    jq -e '.children[] | select(.name == "fix-bug" and .model == "haiku" and .brief == "briefs/fix-bug.md" and (.session_id | length > 0))' "$(roster s3)"
  expect "the roster still has the schema's shape" shape_ok "$(roster s3)"
  coord="$(hc get s3 --json | jq -r '.address')"
  expect "hc shows s3.fix-bug under the coordinator" test "$(parent_addr s3.fix-bug)" = "$coord"
  expect "the child has the crook.flock label" test "$(label_of s3.fix-bug crook.flock)" = s3
  expect "the child has no herdr.* label" no_herdr_labels s3.fix-bug
  launch="$(launch_of "$sid")"
  if [ -n "$launch" ]; then
    expect "the child's argv has --plugin-dir <data>/crook/current/plugin" argv_has "$launch" --plugin-dir "$DATA/crook/current/plugin"
    expect "the child's argv has --agent crook:child --model haiku --session-id <id>" \
      argv_has "$launch" --agent crook:child --model haiku --session-id "$sid"
    prompt="$(argv_last "$launch")"
    case "$prompt" in *"briefs/fix-bug.md"*) ok "the prompt is the last argument and names the brief" ;; *) bad "the last argument is not a prompt naming the brief" ;; esac
    case "$prompt" in *"results/fix-bug.md"*) ok "the prompt names the result path" ;; *) bad "the prompt does not name results/fix-bug.md" ;; esac
  else
    bad "no launch logged for the child's session"
  fi
  expect "the child's <session id>.jsonl exists where a session would write it" test -n "$(find "$CLAUDE_DIR/projects" -name "$sid.jsonl" 2>/dev/null)"
  # A launch that fails leaves no roster entry (Q11) and no branch (D51): the name is taken by another terminal.
  hc run --bg --json --name s3.dup --no-parent --cwd "$RUN_TMP" -- sh -c 'sleep 600' >/dev/null 2>&1
  echo "e2e brief for dup" >"$(flock_dir s3)/briefs/dup.md"
  crook spawn child dup briefs/dup.md --flock s3 --repo "$REPO" --model haiku >"$RUN_TMP/dup.out" 2>&1
  dup_rc=$?
  expect "a spawn whose hc name is taken exits non-zero (brief present, so the name is the reason)" test "$dup_rc" -ne 0 -a "$dup_rc" -ne 2
  expect "the failed launch left no roster entry" no_child s3 dup
  expect "the failed launch left no branch" no_branch crook/s3/dup
  hc kill s3.dup >/dev/null 2>&1
fi

# ---------------------------------------------------------------------------------------------------------------
if want S6; then
  echo "S6 core stop and resume (0017, 0018, 0020; spike 15)"
  start_flock s6 || bad "the s6 flock did not start"
  spawn s6 a || bad "spawn a"
  spawn s6 b || bad "spawn b"
  spawn s6 d || bad "spawn d"
  crook stop d --flock s6 >/dev/null 2>&1
  csid="$(coord_sid s6)"
  asid="$(child_sid s6 a)"
  bsid="$(child_sid s6 b)"
  rb="$(jq -S '.children | map({name,session_id,model,state})' "$(roster s6)")"
  hc server stop >/dev/null 2>&1
  expect "hc server stop ended every terminal" wait_for 10 no_sockets
  out="$(crook open s6 2>&1)"
  case "$out" in *"opened flock s6"*) ok "crook open s6 after the core stop starts the coordinator" ;; *) bad "open s6: $out" ;; esac
  wait_for 40 live_claude s6.a
  wait_for 40 live_claude s6.b
  expect "the SessionStart hook resumed active child a under its own name" live_claude s6.a
  expect "the SessionStart hook resumed active child b under its own name" live_claude s6.b
  expect "the done child s6.d was not resumed" not_live s6.d
  expect "no resumed child got a new name (-r)" no_resumed_names
  for sid in "$asid" "$bsid"; do
    lf="$(launch_of "$sid")"
    expect "the resumed child's environment names hold none of ADR 0020's (the stub's own session markers stopped at crook)" env_names_clean "${lf%.argv}.env"
  done
  expect "the coordinator restarted with --resume <id>" argv_has "$(launch_of "$csid")" --resume "$csid"
  expect "child a was launched again with --resume <its id>" argv_has "$(launch_of "$asid")" --resume "$asid"
  expect "child b was launched again with --resume <its id>" argv_has "$(launch_of "$bsid")" --resume "$bsid"
  expect "the roster has no new entries (3 children)" jq -e '.children | length == 3' "$(roster s6)"
  expect "the roster's children are unchanged by the resume" test "$(jq -S '.children | map({name,session_id,model,state})' "$(roster s6)")" = "$rb"
  # hc keeps one core socket and one holder socket per core (`<session>.holder.<id>.sock`).
  expect "the hook started no second core: one core socket" test "$(find "$RT" -type s -name '*.sock' ! -name '*.holder.*' | wc -l | tr -d ' ')" = 1
  expect "every socket in the run's folder belongs to the run's session" test -z "$(find "$RT" -type s ! -name "$SESSION.*")"
fi

# ---------------------------------------------------------------------------------------------------------------
if want S8; then
  echo "S8 worktree rules (0023; spike 26; Q13)"
  start_flock s8 || bad "the s8 flock did not start"
  for c in fresh unpushed dirty pushed merged; do spawn s8 $c --repo "$REPO" --model haiku || bad "spawn $c"; done
  expect "spawn --repo makes <repo>/.worktrees/s8-fresh" test -d "$REPO/.worktrees/s8-fresh"
  expect "the worktree is on crook/s8/fresh" test "$(g "$REPO/.worktrees/s8-fresh" rev-parse --abbrev-ref HEAD)" = crook/s8/fresh
  w="$REPO/.worktrees"
  echo unpushed >"$w/s8-unpushed/work.txt" && g "$w/s8-unpushed" add work.txt && g "$w/s8-unpushed" commit -q -m work
  echo x >"$w/s8-dirty/scratch.txt"
  echo pushed >"$w/s8-pushed/work.txt" && g "$w/s8-pushed" add work.txt && g "$w/s8-pushed" commit -q -m work && g "$w/s8-pushed" push -q origin crook/s8/pushed
  echo merged >"$w/s8-merged/work.txt" && g "$w/s8-merged" add work.txt && g "$w/s8-merged" commit -q -m work && g "$REPO" merge -q --ff-only crook/s8/merged
  for c in fresh unpushed dirty pushed merged; do crook stop "$c" --flock s8 >"$RUN_TMP/stop-$c.out" 2>&1; done
  expect "stop removes a worktree whose branch has no new commits (counts as merged)" test ! -d "$w/s8-fresh"
  expect "stop keeps a worktree with an unpushed commit" test -d "$w/s8-unpushed"
  expect "stop keeps a dirty worktree" test -d "$w/s8-dirty"
  expect "stop removes a pushed worktree" test ! -d "$w/s8-pushed"
  expect "stop removes a merged worktree" test ! -d "$w/s8-merged"
  for c in fresh unpushed dirty pushed merged; do
    expect "the branch crook/s8/$c remains" test -n "$(g "$REPO" branch --list "crook/s8/$c")"
    expect "$c is done in the roster" test "$(child_field s8 "$c" state)" = "done"
  done
  for c in unpushed dirty; do
    if grep -q "^warning: kept worktree $w/s8-$c: ." "$RUN_TMP/stop-$c.out"; then
      ok "the refusal for $c says why: $(sed -n "s|^warning: kept worktree $w/s8-$c: ||p" "$RUN_TMP/stop-$c.out")"
    else
      bad "no 'warning: kept worktree' line with a reason for $c"
    fi
  done
fi

# ---------------------------------------------------------------------------------------------------------------
if want S9; then
  echo "S9 model change on resume, argv only (0025; spike 26; Q10)"
  start_flock s9 || bad "the s9 flock did not start"
  spawn s9 m --model haiku || bad "spawn m"
  sid="$(child_sid s9 m)"
  set_state "$sid" working
  wait_for 10 status_is s9.m working || note "the stub never showed as working"
  refuse "resume --model is refused while the child works" crook resume m --model sonnet --flock s9
  expect "the roster still says haiku after the refusal" test "$(child_field s9 m model)" = haiku
  set_state "$sid" idle
  wait_for 10 status_is s9.m idle
  old="$(hc get s9.m --json | jq -r .address)"
  if crook resume m --model sonnet --flock s9 >/dev/null 2>&1; then ok "resume --model sonnet works once the child is idle"; else bad "resume --model sonnet"; fi
  wait_for 20 live_claude s9.m
  expect "the child is on a new terminal under the same name" test "$(hc get s9.m --json | jq -r .address)" != "$old"
  expect "the roster says sonnet and the child is not done" jq -e '.children[] | select(.name == "m" and .model == "sonnet" and .state == "active")' "$(roster s9)"
  expect "the argv has --model sonnet --resume <id>" argv_has "$(launch_of "$sid")" --model sonnet --resume "$sid"
  note "S9's memory check (the child recalls a word from before) is tier 3"
fi

# ---------------------------------------------------------------------------------------------------------------
if want S10; then
  echo "S10 stop and close (0018, 0020, 0023; Q6)"
  start_flock s10 || bad "the s10 flock did not start"
  for c in clean dirty open; do spawn s10 $c --repo "$REPO" --model haiku || bad "spawn $c"; done
  echo x >"$REPO/.worktrees/s10-dirty/scratch.txt"
  crook stop clean --flock s10 >/dev/null 2>&1
  expect "a stopped child is done" test "$(child_field s10 clean state)" = "done"
  expect "its terminal is gone" not_live s10.clean
  out="$(crook close s10 2>&1)"
  expect "crook close killed the coordinator" not_live s10
  expect "the roster says the flock is done" jq -e '.coordinator.state == "done"' "$(roster s10)"
  expect "close removed the clean worktree of the child that was still active" test ! -d "$REPO/.worktrees/s10-open"
  expect "close kept the dirty worktree" test -d "$REPO/.worktrees/s10-dirty"
  n="$(printf '%s\n' "$out" | grep -c '^warning: kept worktree')"
  expect "close printed one 'warning: kept worktree' line, for the dirty worktree" test "$n" = 1
  case "$out" in *"s10-dirty"*) ok "the warning names the dirty worktree" ;; *) bad "the warning does not name s10-dirty" ;; esac
fi

# ---------------------------------------------------------------------------------------------------------------
if want S13; then
  echo "S13 mod install and upgrade (0019, Q22; mod plan M5.2)"
  mod="$APP/mods/crook"
  expected="$(mod_files "$INSTALL/$VERSION/mod" | LC_ALL=C sort)"
  expected="$(printf '%s\n.crook-version\n' "$expected" | LC_ALL=C sort)"
  if crook setup >"$RUN_TMP/setup1.out" 2>&1; then ok "step 1: a fresh setup succeeds"; else bad "step 1: setup failed: $(tail -2 "$RUN_TMP/setup1.out")"; fi
  got="$(cd "$mod" 2>/dev/null && find . -type f | sed 's|^\./||' | LC_ALL=C sort)"
  expect "step 1: mods/crook holds the mod's files and the marker, nothing else" test "$got" = "$expected"
  for f in mod.json main.js lib.js .crook-version; do expect "step 1: mods/crook/$f exists" test -f "$mod/$f"; done
  expect "step 1: the marker says $VERSION" test "$(cat "$mod/.crook-version")" = "$VERSION"
  # 2: a folder crook did not install.
  mv "$mod/.crook-version" "$mod/.hold"
  before="$(tree_sum "$mod")"
  refuse "step 2: an unmarked mods/crook makes setup stop" crook setup
  crook setup >"$RUN_TMP/setup2.out" 2>&1
  expect "step 2: setup says why ($(head -1 "$RUN_TMP/setup2.out" | cut -c1-90))" test -s "$RUN_TMP/setup2.out"
  expect "step 2: the folder is as it was" test "$(tree_sum "$mod")" = "$before"
  mv "$mod/.hold" "$mod/.crook-version"
  # 3: an older marker.
  echo "1.1.0" >"$mod/.crook-version"
  echo stray >"$mod/old-file.js"
  if crook setup >/dev/null 2>&1; then ok "step 3: setup over an older marker succeeds"; else bad "step 3: setup failed"; fi
  expect "step 3: the file the new version lacks is gone" test ! -e "$mod/old-file.js"
  expect "step 3: the marker is the new version" test "$(cat "$mod/.crook-version")" = "$VERSION"
  got="$(cd "$mod" && find . -type f | sed 's|^\./||' | LC_ALL=C sort)"
  expect "step 3: the folder was replaced whole" test "$got" = "$expected"
  # 4: a newer marker.
  echo "9.9.9" >"$mod/.crook-version"
  before="$(tree_sum "$mod")"
  refuse "step 4: setup over a newer marker refuses" crook setup
  crook setup >"$RUN_TMP/setup4.out" 2>&1
  expect "step 4: it says the mod is newer" grep -qi newer "$RUN_TMP/setup4.out"
  expect "step 4: the folder is as it was" test "$(tree_sum "$mod")" = "$before"
  # 5: other commands leave the mod alone.
  echo "1.1.0" >"$mod/.crook-version"
  before="$(tree_sum "$mod")"
  crook status >/dev/null 2>&1
  expect "step 5: crook status leaves mods/crook untouched, even under an older marker" test "$(tree_sum "$mod")" = "$before"
  crook setup >/dev/null 2>&1
  # 6: herdr-app check, on a copy in a throwaway home (D61), with the real herdr-app found on the caller's PATH.
  real="$(PATH="$REAL_PATH" command -v herdr-app || true)"
  if [ -z "$real" ]; then
    note "step 6: herdr-app is not installed here, skipped"
  else
    chk="$RUN/check-home"
    rm -rf "$chk" "$RUN_TMP/modcheck"
    mkdir -p "$chk/data" "$chk/config" "$chk/cache" "$chk/app"
    cp -R "$mod" "$RUN_TMP/modcheck"
    if run_timeout 30 env -i PATH="$REAL_PATH" HOME="$chk" XDG_DATA_HOME="$chk/data" XDG_CONFIG_HOME="$chk/config" \
      XDG_CACHE_HOME="$chk/cache" XDG_STATE_HOME="$chk/state" XDG_RUNTIME_DIR="$RT" HERDR_APP_HOME="$chk/app" \
      "$real" check "$RUN_TMP/modcheck" >"$RUN_TMP/check.out" 2>&1; then
      ok "step 6: herdr-app check passes on the installed mod"
    else
      bad "step 6: herdr-app check failed: $(tail -3 "$RUN_TMP/check.out")"
    fi
  fi
fi

# Every launched session: its SessionStart hook exited 0 with nothing on stderr, no prompt was swallowed by a
# variadic flag, and its environment names hold none of ADR 0020's.
echo "all launches"
for f in "$calls"/*.hook-session-start.exit; do
  [ -e "$f" ] || continue
  base="${f%.hook-session-start.exit}"
  expect "hook exit 0 and no stderr: $(basename "$base")" test "$(cat "$f")" = 0 -a ! -s "$base.hook-session-start.err"
  expect "no prompt lost to a variadic flag: $(basename "$base")" test ! -e "$base.noprompt"
  expect "no ADR 0020 name in the session's environment: $(basename "$base")" env_names_clean "$base.env"
done

echo
echo "tier 2: $PASS passed, $FAIL failed, $NOTE note(s)"
[ "$FAIL" = 0 ]
