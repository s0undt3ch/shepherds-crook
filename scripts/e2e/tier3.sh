#!/usr/bin/env bash
# tier3.sh [--keep] [--only S1,S4,...] [--binary <crook>] [--permission-mode auto|default]: the tier 3 scenarios (release plan, "E2E harness"): a
# real scratch hc core, the REAL `claude` CLI on Haiku, a stub `herdr-app`. LOCAL ONLY (Q34), and driven by Claude
# with the crook-e2e skill: it spends model tokens and needs Claude's login.
#
# The run has its own HOME and CLAUDE_CONFIG_DIR, as in tier 2. login.sh copies Claude's login into them without its
# refresh token (D122), and teardown.sh deletes the copy. The sessions load none of the maintainer's Claude settings,
# plugins or hooks: the run's settings.json sets only the permission mode, `auto` by default as the maintainer's
# sessions run, `default` for ADR 0027's check that the coordinator's crook calls need no approval. Names only are
# ever printed from the environment, never values.
#
# Every hc and crook call goes through hc.sh and crook.sh. Run this script with bash, never type a raw `hc` or
# `crook` in a shell that sits inside a real hc core.
#
# Scenarios (T3 parts): S1 core start and no "Transcript saving is off" line, S4 stop-hook delivery (and what /clear
# does), S5 blocked coordinator, S6 core stop and resume, S7 upgrade self-reload, S9 model change and memory, S11
# blocked child, S12 step-up question. Each uses its own flock, so --only picks any of them. The pass checks read files, hc and the
# transcripts, never what a model says it did. A step the model would not do is NOT EXERCISED, not a failure.
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
source "$here/lib.sh"

SCENARIOS="S1 S4 S5 S6 S7 S9 S11 S12"
keep=0
only=''
only_given=0
binary="$here/../../target/release/crook"
mode=auto
while [ "$#" -gt 0 ]; do
  case "$1" in
    --keep) keep=1 ;;
    --only) only="${2:-}"; only_given=1; shift ;;
    --binary) binary="${2:-}"; shift ;;
    --permission-mode) mode="${2:-}"; shift ;;
    *) die "usage: tier3.sh [--keep] [--only S1,S4,...] [--binary <crook>] [--permission-mode auto|default]" ;;
  esac
  shift
done
case "$mode" in auto | default) ;; *) die "--permission-mode takes auto or default (got '$mode')" ;; esac
# An unknown or lower-case name would run nothing and pass.
if [ "$only_given" = 1 ]; then
  valid_only "$only" "$SCENARIOS" || die "--only takes names from: $SCENARIOS (got '$only')"
fi
for tool in hc jq git lsof toolr claude shasum; do command -v "$tool" >/dev/null || die "$tool is needed and not on PATH"; done
[ -x "$binary" ] || die "no crook binary at $binary; run cargo build --release, or pass --binary"
binary="$(cd "$(dirname "$binary")" && pwd -P)/$(basename "$binary")"

VERSION_A=1.2.0
VERSION_B=1.3.0
ENVF="$("$BASH" "$here/setup.sh" --tier 3)" || die "setup failed"

# The trap goes up before anything else can fail: a run made by setup.sh is always torn down, or kept on purpose.
cleanup() {
  trap - EXIT INT TERM
  local rc=0
  if [ "$keep" = 1 ]; then "$BASH" "$here/teardown.sh" "$ENVF" --keep || rc=$?; else "$BASH" "$here/teardown.sh" "$ENVF" || rc=$?; fi
  if [ "$rc" != 0 ]; then
    echo "tier3: teardown left the run in place; see above. To clean up: bash scripts/e2e/teardown.sh $ENVF" >&2
    exit 1
  fi
}
trap cleanup EXIT
trap 'exit 130' INT TERM
load_run "$ENVF"
[ "$TIER" = 3 ] || die "setup made a tier $TIER run"
echo "run: $RUN (permission mode $mode)"
"$BASH" "$here/login.sh" "$ENVF" || die "no Claude login for the run"
# `tui` as the maintainer's sessions run, and so a fresh config does not stop at the "try the fullscreen renderer" dialog.
# An `autoMode.environment` line, so auto mode does not stop to offer to scan the environment (shell history included).
jq -n --arg m "$mode" '{permissions: {defaultMode: $m}, skipAutoPermissionPrompt: true, tui: "fullscreen",
  autoMode: {environment: ["A throwaway crook end-to-end test run: every folder it uses is under one temporary folder."]}}' \
  >"$CLAUDE_DIR/settings.json"
REAL_CFG="${REAL_CLAUDE:-$REAL_HOME/.claude}"

# The real places a run must not change, as names and mtimes: what the maintainer's app and crook installs hold.
REGISTRY="$REAL_HOME/.local/state/herdr-core/default/terminals.json"
snapshot() {
  local p
  for p in "$REAL_HOME/.config/herdr-app/mods" "$REAL_HOME/.local/share/crook" "$REAL_HOME/.local/state/crook" \
    "$REAL_HOME/.config/crook" "$REAL_HOME/.config/herdr-core" "$REAL_HOME/.local/state/herdr-core"; do
    echo "== $p"
    if [ -e "$p" ]; then mtime_list "$p"; else echo absent; fi
  done
}
SNAP_BEFORE="$RUN_TMP/real-before.txt"
snapshot >"$SNAP_BEFORE"
"$BASH" "$here/install.sh" "$ENVF" "$VERSION_A" "$binary" >/dev/null || die "install failed"

hc() { "$BASH" "$here/hc.sh" "$ENVF" "$@"; }
crook() { "$BASH" "$here/crook.sh" "$ENVF" "$@"; }
trust() { "$BASH" "$here/trust.sh" "$ENVF" "$@"; }
PASS=0
FAIL=0
NOTE=0
SKIP=0

ok() { PASS=$((PASS + 1)); echo "  PASS $*"; }
bad() { FAIL=$((FAIL + 1)); echo "  FAIL $*"; }
note() { NOTE=$((NOTE + 1)); echo "  note $*"; }
nx() { SKIP=$((SKIP + 1)); echo "  NOT EXERCISED $*"; }
expect() {
  local desc="$1"
  shift
  if "$@" >/dev/null 2>&1; then ok "$desc"; else bad "$desc"; fi
}
want() { [ -z "$only" ] || case ",$only," in *",$1,"*) return 0 ;; *) return 1 ;; esac; }
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

CFG="$CLAUDE_DIR"
flock_dir() { find "$DATA/crook/flocks" -maxdepth 1 -type d -name "*-$1" 2>/dev/null | head -1; }
roster() { echo "$(flock_dir "$1")/roster.json"; }
res() { echo "$(flock_dir "$1")/results/$2.md"; }
child_field() { jq -r --arg c "$2" --arg f "$3" '.children[] | select(.name == $c) | .[$f]' "$(roster "$1")"; }
coord_sid() { jq -r '.coordinator.session_id' "$(roster "$1")"; }
# Claude names a transcript folder after the cwd with every non-alphanumeric character turned into -; it is found by
# the session id instead of computed.
transcript() { case "$1" in '' | null) return 1 ;; esac; find "$CFG/projects" -name "$1.jsonl" 2>/dev/null | head -1; }
# The text of the last assistant message.
last_said() { jq -rs '[.[] | select(.type == "assistant") | [.message.content[]? | select(.type == "text") | .text] | select(length > 0) | join("\n")] | last // ""' "$1"; }
sha_of() { shasum -a 256 "$1" | cut -c1-64; }
mark_of() { jq -r --arg c "$2" '.[$c] // empty' "$(flock_dir "$1")/results/.delivered.json" 2>/dev/null; }
result_status() { sed -n 's/^status: *//p' "$(res "$1" "$2")" 2>/dev/null | head -1; }
result_is() { [ "$(result_status "$1" "$2")" = "$3" ]; }
mark_is_file() { [ -f "$(res "$1" "$2")" ] && [ "$(mark_of "$1" "$2")" = "$(sha_of "$(res "$1" "$2")")" ]; }
state_of() { hc get "$1" --json | jq -r '.status.state // empty'; }
state_is() { [ "$(state_of "$1")" = "$2" ]; }
kind_of() { hc get "$1" --json | jq -r '.status.kind // empty'; }
address_of() { hc get "$1" --json | jq -r '.address // empty'; }
rec_is() { [ "$(jq -r .version "$STATE/crook/sessions/$1" 2>/dev/null)" = "$2" ]; }
# A line the maintainer would type, into the session's own pane: hc types it untagged (no <hc from=…>), so a
# session reads it as the maintainer. Returns non-zero when the send failed or started no turn; waits for the turn
# up to $3 seconds.
say() {
  hc send "$1" "$2" --wait --timeout "${3:-120}" >"$RUN_TMP/say.out" 2>&1 || return 1
  jq -e '.started == true' "$RUN_TMP/say.out" >/dev/null 2>&1
}
# /clear starts no model turn, so say's check that a turn started does not fit it: send it, then wait until the
# session is idle again.
clear_in() { # clear_in <terminal>
  # hc exits non-zero when the send starts no turn, which is what /clear does; its JSON reply says it was typed.
  hc send "$1" "/clear" --wait --timeout 60 >"$RUN_TMP/clear.out" 2>&1
  jq -e '.turn != null' "$RUN_TMP/clear.out" >/dev/null 2>&1 || return 1
  # A cleared session with no turn yet reports `idle`, not `done`.
  wait_for 30 settled "$1"
}
settled() { case "$(state_of "$1")" in done | idle) return 0 ;; *) return 1 ;; esac; }
brief() { # brief <flock> <child> <goal>
  local dir
  dir="$(flock_dir "$1")"
  mkdir -p "$dir/briefs"
  printf '## Goal\n\n%s\n\n## Why\n\nAn end-to-end test of crook. Do nothing beyond the goal.\n\n## Scope\n\nIn scope:\n\n- The result file only.\n\nOut of scope:\n\n- Everything else.\n\n## References\n\nNone.\n\n## Done when\n\n- The goal is met.\n' "$3" >"$dir/briefs/$2.md"
}
# trust.sh refuses a dialog that names a path outside the run. That is a stop, not a step to skip: the run is kept
# for inspection and the runner exits non-zero.
trust_in() { # trust_in <terminal>
  local out="$RUN_TMP/trust-$1.out"
  trust "$1" 120 >"$out" 2>&1
  local rc=$?
  if grep -q '^REFUSED' "$out" 2>/dev/null; then
    keep=1
    echo "REFUSED: trust.sh would not answer a dialog in $1: $(grep '^REFUSED' "$out" | head -1)" >&2
    exit 1
  fi
  return "$rc"
}
# A fresh Claude config offers, after the first turn under `default` mode, to make auto mode the default. The answer
# is "No, keep manual mode" (the second option); nothing else is ever answered here.
no_auto_offer() { # no_auto_offer <terminal>
  if state_is "$1" blocked && screen_has "$1" 'Make auto mode your default permission mode'; then
    note "answered Claude's auto-mode offer in $1 with: No, keep manual mode"
    hc keys "$1" down enter >/dev/null 2>&1 || return 1
    wait_for 30 state_is "$1" "done"
  fi
}
new_flock() { # new_flock <slug> <task>
  crook new "$1" --task "$2" >"$RUN_TMP/new-$1.out" 2>&1 || return 1
  wait_for 30 is_live "$1" || return 1
  wait_for 90 state_is "$1" blocked || true
  trust_in "$1" || return 1
  wait_for 180 state_is "$1" "done" || { no_auto_offer "$1" && state_is "$1" "done"; }
}
spawn_child() { # spawn_child <flock> <child> <goal> [crook spawn options...]
  local f="$1" c="$2"
  brief "$f" "$c" "$3"
  shift 3
  crook spawn child "$c" "$(flock_dir "$f")/briefs/$c.md" --flock "$f" --model haiku "$@" >"$RUN_TMP/spawn-$f-$c.out" 2>&1 || return 1
  wait_for 30 is_live "$f.$c" || return 1
  # The add-dir dialog takes a while to draw; trust.sh gives up after a quiet screen, so wait for the session first.
  wait_for 90 state_is "$f.$c" blocked || true
  trust_in "$f.$c" || return 1
  return 0
}
# A terminal that is live; its agent shows only once the session has started past any dialog.
is_live() {
  local out
  out="$(hc ls --json)" || return 1
  printf '%s' "$out" | jq -e --arg n "$1" '.terminals[] | select(.name == $n and .state == "live")' >/dev/null
}
# A failing hc is a failing check: the screen is read first, and only then searched.
screen_has() {
  local out
  out="$(hc read "$1" --lines 400)" || return 1
  printf '%s' "$out" | grep -qiF -- "$2"
}
screen_lacks() {
  local out
  out="$(hc read "$1" --lines 400)" || return 1
  ! printf '%s' "$out" | grep -qiF -- "$2"
}
# A screen line that holds both texts.
screen_line_has() {
  local out
  out="$(hc read "$1" --lines 400)" || return 1
  printf '%s' "$out" | grep -F -- "$2" | grep -qF -- "$3"
}
not_in_tree() { ! grep -rq -- "$1" "$2"; }
names_in() { find "$1" -maxdepth 1 -type f -exec basename {} \; 2>/dev/null | LC_ALL=C sort | tr '\n' ' '; }
new_terminal() { local a; a="$(address_of "$1")" && [ -n "$a" ] && [ "$a" != "$2" ]; }
roster_model_is() { [ "$(child_field "$1" "$2" model)" = "$3" ]; }
no_core_sockets() { [ -z "$(find "$RT/herdr-core" -type s 2>/dev/null)" ]; }
no_resumed_names() {
  local out
  out="$(hc ls --json)" || return 1
  [ -z "$(printf '%s' "$out" | jq -r '.terminals[].name | select(test("-r$"))')" ]
}
not_live() {
  local out
  out="$(hc ls --json)" || return 1
  ! printf '%s' "$out" | jq -e --arg n "$1" '.terminals[] | select(.name == $n)' >/dev/null
}
# hc's core sockets are in herdr-core/; Claude keeps its own sockets (cc-socks/) in the same runtime folder.
no_child_socket_second() { [ "$(find "$RT/herdr-core" -type s -name '*.sock' ! -name '*.holder.*' | wc -l | tr -d ' ')" = 1 ]; }
shot() { hc read "$1" --lines 400 | grep -v '^[[:space:]]*$' | tail -n "${2:-25}" | sed 's/^/      | /'; }
# Every scenario is a function: a step it cannot get past reports one FAIL and the scenario returns.
stop_scenario() { bad "$*; the rest of the scenario is skipped"; }

# ---------------------------------------------------------------------------------------------------------------
s1() {
  echo "S1 core start with the scrub, T3 part (0016, 0020; spike 23)"
  expect "no core runs for the run's HC_SESSION before crook starts one" test -z "$(find "$RT" -type s)"
  if "$BASH" "$here/core_env.sh" "$ENVF" | sed 's/^/    /'; then
    ok "crook starts the core, and a probe terminal's env has none of 0020's variables"
  else
    bad "core_env.sh failed"
  fi
  new_flock s1 "Reply with the single word ready and do nothing else." || { stop_scenario "the s1 coordinator did not start"; shot s1; return; }
  ok "a real coordinator started on the scratch core and finished its first turn"
  local sid tr
  sid="$(coord_sid s1)"
  expect "hc reports the coordinator as app claude-code" test "$(hc get s1 --json | jq -r '.status.app')" = claude-code
  expect "the coordinator shows no 'Transcript saving is off' line" screen_lacks s1 'transcript saving is off'
  tr="$(transcript "$sid")"
  expect "the coordinator's transcript exists under the Claude config folder" tr_ok "$tr"
  expect "the coordinator runs on --model haiku (the run's coordinator_model)" proc_with_id --session-id "$sid" "--model haiku"
  note "permission mode of the real session: $(jq -r 'select(.permissionMode != null) | .permissionMode' "$tr" 2>/dev/null | sort | uniq -c | tr '\n' ' ')"
  # ADR 0027: the coordinator calls crook by the literal path its allow rule names, so the call needs no approval.
  # Only `--permission-mode default` proves that; under `auto` a classifier may allow any form.
  if say s1 "Run crook status for the flock s1 with --json, then reply with only its state." 180; then
    wait_for 120 state_is s1 "done" || true
    if state_is s1 blocked; then
      bad "the coordinator's crook call waits for approval (0027):"
      shot s1 12
      hc keys s1 escape >/dev/null 2>&1 || true
    else
      expect "the coordinator ran crook as the literal run path, with no denial (0027, $mode mode)" crook_call_ran "$tr"
      expect "the coordinator's crook calls never use a variable (0027)" no_variable_crook_call "$tr"
    fi
  else
    nx "the coordinator took no turn for the crook status request: $(head -c 300 "$RUN_TMP/say.out" | tr '\n' ' ')"
    shot s1 10
  fi
}

# A Bash call that starts with the quoted run shim path, and whose result is not an error.
crook_call_ran() {
  jq -s -e --arg p "\"$SHIM\" " '
    [.[] | select(.type == "assistant") | .message.content[]? | select(.type == "tool_use" and .name == "Bash")
      | select(.input.command | startswith($p)) | .id] as $ids
    | [.[] | select(.type == "user") | .message.content[]? | select(.type == "tool_result")
      | select(.tool_use_id as $i | $ids | index($i)) | select(.is_error != true)] | length > 0' "$1"
}
no_variable_crook_call() {
  ! jq -r 'select(.type == "assistant") | .message.content[]? | select(.type == "tool_use" and .name == "Bash")
    | .input.command' "$1" | grep -q -e 'CROOK_BIN' -e 'MISE_DATA_DIR' -e 'shims/crook}'
}

# ---------------------------------------------------------------------------------------------------------------
s4() {
  echo "S4 stop-hook delivery (0008, 0009; spike 14)"
  new_flock s4 "Reply with the single word ready. Then wait for the maintainer." || { stop_scenario "the s4 coordinator did not start"; return; }
  spawn_child s4 one "Set the result file's status to done and its Summary to the single word ok." || { stop_scenario "spawn one"; return; }
  wait_for 240 result_is s4 one "done" || { stop_scenario "the child never wrote status done"; return; }
  expect ".delivered.json holds the SHA-256 of the whole result file (Q27)" wait_for 60 mark_is_file s4 one
  local p ctr n0 m0 sid proj before after newfiles newid
  p="$(res s4 one)"
  ctr="$(transcript "$(coord_sid s4)")"
  expect "the coordinator's screen shows an <hc from=…> line with 'Result updated: <path>'" \
    screen_line_has s4 '<hc from=' "Result updated: $p"
  expect "the coordinator was told 'Result updated: <path>' by the child's hook (an <hc> line in its transcript)" told_has hc "$ctr" "Result updated: $p"
  expect "the child shows no 'Transcript saving is off' line" screen_lacks s4.one 'transcript saving is off'
  # A turn that changes nothing sends nothing.
  n0="$(count_in "Result updated: $p" "$ctr")"
  m0="$(mark_of s4 one)"
  if say s4.one "Reply with the single word ok. Do not touch any file." 120; then
    sleep 5
    expect "a turn that changes nothing leaves the mark as it was" test "$(mark_of s4 one)" = "$m0"
    expect "and the coordinator was told nothing more" test "$(count_in "Result updated: $p" "$ctr")" = "$n0"
  else
    nx "the no-change turn: the send to the child failed"
  fi
  # A real change is sent again.
  if say s4.one "Append the line 'second update' under Summary in $p, keep status done, then end your turn." 120; then
    expect "a changed result is sent again: the mark moves to the new hash" wait_for 60 mark_is_file s4 one
    expect "the mark differs from the first one" test "$(mark_of s4 one)" != "$m0"
  else
    nx "the changed-result turn: the send to the child failed"
  fi
  # What /clear does to a child: the session id changes (plugin review question).
  sid="$(child_field s4 one session_id)"
  proj="$(dirname "$(transcript "$sid")")"
  before="$(names_in "$proj")"
  if clear_in s4.one && sleep 3 && say s4.one "Append the line 'after clear' under Summary in $p, keep status done, then end your turn." 120; then
    after="$(names_in "$proj")"
    newfiles="$(comm -13 <(printf '%s\n' "$before" | tr ' ' '\n' | sort) <(printf '%s\n' "$after" | tr ' ' '\n' | sort) | tr '\n' ' ')"
    note "/clear: transcripts new in the child's project folder: ${newfiles:-none}; the original session id was $sid"
    note "/clear: SessionStart records now: $(names_in "$STATE/crook/sessions")"
    newid="${newfiles%%.jsonl*}"
    newid="${newid// /}"
    if [ -n "$newid" ] && [ "$newid" != "$sid" ] && [ -z "${newfiles#*.jsonl }" ]; then
      expect "/clear (D114): roster.json's session_id for the child is the new session id, not the original" test "$(child_field s4 one session_id)" = "$newid"
      expect "/clear (D115): a version record exists for the new id" test -s "$STATE/crook/sessions/$newid"
      expect "/clear (D115): the new id's record holds the version the old id's did" test "$(jq -r .version "$STATE/crook/sessions/$newid")" = "$(jq -r .version "$STATE/crook/sessions/$sid" 2>/dev/null)"
      hc kill s4.one >/dev/null 2>&1
      wait_for 30 not_live s4.one || note "the cleared child's terminal did not end"
      if crook resume one --flock s4 >"$RUN_TMP/resume-s4.out" 2>&1; then
        expect "/clear: crook resume launches with --resume <the new id>" wait_for 30 proc_with_id --resume "$newid" "--agent crook:child"
        wait_for 90 state_is s4.one blocked || true
        trust_in s4.one || true
      else
        bad "/clear: crook resume one failed: $(head -c 200 "$RUN_TMP/resume-s4.out")"
      fi
    else
      nx "/clear: not exactly one new transcript (${newfiles:-none}), so the new session id is unknown"
    fi
    if grep -q 'after clear' "$p"; then
      expect "/clear: a result changed after it is still delivered to the coordinator" wait_for 60 mark_is_file s4 one
    else
      nx "/clear: the child did not write the line after the clear"
    fi
  else
    nx "/clear: the sends to the child failed: clear: $(head -c 200 "$RUN_TMP/clear.out" | tr '\n' ' '); say: $(head -c 200 "$RUN_TMP/say.out" | tr '\n' ' ')"
    shot s4.one 10
  fi
}

# ---------------------------------------------------------------------------------------------------------------
s5() {
  echo "S5 blocked coordinator, then rescan (0008, 0010; spike 14; Q28)"
  new_flock s5 "Reply with the single word ready. Then wait for the maintainer." || { stop_scenario "the s5 coordinator did not start"; return; }
  # The coordinator's own skill forbids AskUserQuestion while a child is live; none is live yet.
  hc send s5 'Test instruction from the maintainer: call the AskUserQuestion tool right now with the question "Which colour?" and options red and blue. Do nothing else first.' >/dev/null 2>&1
  if ! wait_for 90 state_is s5 blocked || [ "$(kind_of s5)" != question ]; then
    nx "the coordinator is not blocked on a question (hc: state '$(state_of s5)', kind '$(kind_of s5)'); no key is sent to it"
    shot s5
    return
  fi
  ok "the coordinator is blocked on a question (hc kind: $(kind_of s5))"
  spawn_child s5 two "Set the result file's status to done and its Summary to the single word ok." || { stop_scenario "spawn two"; return; }
  wait_for 240 result_is s5 two "done" || { stop_scenario "the child never wrote status done"; return; }
  sleep 10
  local p ctr held_send
  p="$(res s5 two)"
  ctr="$(transcript "$(coord_sid s5)")"
  tr_ok "$ctr" || { stop_scenario "the coordinator's transcript is missing"; return; }
  expect "the coordinator is still blocked while the child's update arrives" state_is s5 blocked
  if [ -z "$(mark_of s5 two)" ]; then
    ok "the send to the blocked coordinator did not go through: the mark is still empty"
  else
    note "the mark moved while the coordinator was blocked (hc took the send)"
  fi
  expect "the coordinator was told no 'Result updated: <path>' while it was blocked" test "$(count_in "Result updated: $p" "$ctr")" = 0
  # An hc note about the child, sent while the coordinator cannot take it (Q28).
  hc send s5.two 'Use the AskUserQuestion tool now to ask "Which size?" with options S and M. Do not guess; wait.' >/dev/null 2>&1
  wait_for 90 state_is s5.two blocked || nx "the child did not ask with AskUserQuestion"
  # Answer only a question: Enter on a permission dialog would approve it.
  if [ "$(kind_of s5)" != question ]; then
    nx "the coordinator's dialog is not a question any more (kind '$(kind_of s5)'); no key is sent"
    return
  fi
  hc keys s5 enter >/dev/null 2>&1
  wait_for 90 state_is s5 "done" || stop_scenario "the coordinator never finished after the answer"
  sleep 15
  held_send=0
  if [ "$(count_in "Result updated: $p" "$ctr")" -ge 1 ]; then
    held_send=1
    note "Q28: the child's 'Result updated:' send was HELD or retried: the coordinator was told it after the answer"
    expect "the mark moved on that delivery" wait_for 20 mark_is_file s5 two
  else
    note "Q28: the child's 'Result updated:' send was DROPPED while the coordinator was blocked (nothing typed after the answer)"
  fi
  if told_has hc "$ctr" 's5.two is blocked'; then
    note "Q28: hc's note about the blocked child was HELD and typed after the answer: $(hc_lines "$ctr" | grep 's5.two is blocked' | head -1 | cut -c1-120)"
  else
    note "Q28: hc's note about the blocked child was DROPPED: no <hc> line about s5.two in the coordinator's transcript"
  fi
  say s5 "Carry on." 120 || nx "the 'Carry on.' prompt could not be sent"
  # The rescan runs on that prompt: it names a result that was not delivered, and nothing when it was.
  if [ "$held_send" = 0 ]; then
    expect "after the answer, the next prompt's hook output names the undelivered result (rescan)" told_has hook "$ctr" "Undelivered child result: $p"
  else
    expect "the send was held, so the rescan names nothing" told_lacks hook "$ctr" "Undelivered child result: $p"
  fi
  expect "the mark matches the file once the rescan has run" wait_for 30 mark_is_file s5 two
  note "S5's 'asks about a model step-up in plain text' is S12's check: this coordinator is told to use AskUserQuestion here on purpose, and S12 has the failed result that raises the question"
  crook stop two --flock s5 >/dev/null 2>&1
}

# ---------------------------------------------------------------------------------------------------------------
s6() {
  echo "S6 core stop and resume (0017, 0018, 0020; spike 15)"
  new_flock s6 "Reply with the single word ready. Then wait for the maintainer." || { stop_scenario "the s6 coordinator did not start"; return; }
  spawn_child s6 a "Set the result file's status to done and its Summary to the single word ok." || { stop_scenario "spawn a"; return; }
  spawn_child s6 d "Set the result file's status to done and its Summary to the single word ok." || { stop_scenario "spawn d"; return; }
  wait_for 240 result_is s6 a "done" || { stop_scenario "child a never wrote status done"; return; }
  wait_for 240 result_is s6 d "done" || { stop_scenario "child d never wrote status done"; return; }
  crook stop d --flock s6 >/dev/null 2>&1
  local csid asid before out
  csid="$(coord_sid s6)"
  asid="$(child_field s6 a session_id)"
  before="$(jq -S '.children | map({name,session_id,model,state})' "$(roster s6)")"
  hc server stop >/dev/null 2>&1
  expect "hc server stop ended the core: no core socket is left" wait_for 15 no_core_sockets
  out="$(crook open s6 2>&1)"
  case "$out" in *"opened flock s6"*) ok "crook open s6 after the core stop starts the coordinator" ;; *) stop_scenario "crook open s6: $out"; return ;; esac
  # Every resumed session shows the add-dir dialog again.
  wait_for 60 is_live s6 || true
  wait_for 90 state_is s6 blocked || true
  trust_in s6 || true
  wait_for 90 is_live s6.a || stop_scenario "the SessionStart hook did not resume child a"
  wait_for 90 state_is s6.a blocked || true
  trust_in s6.a || true
  expect "the coordinator was started again with --resume <its id>" proc_with_id --resume "$csid" "--agent crook:coordinator"
  expect "child a was resumed under its own name with --resume <its id>" proc_with_id --resume "$asid" "--agent crook:child"
  expect "no resumed terminal got a new name (-r)" no_resumed_names
  expect "the done child s6.d was not resumed" not_live s6.d
  expect "the roster's children are as they were (no new entries)" test "$(jq -S '.children | map({name,session_id,model,state})' "$(roster s6)")" = "$before"
  note "sockets in the run's folder: $(find "$RT" -type s | sed "s|^$RT/||" | tr '\n' ' ')"
  expect "the hook started no second core: one core socket" no_child_socket_second
  expect "every socket in the run's folder belongs to the run's session" test -z "$(find "$RT/herdr-core" -type s ! -name "$SESSION.*")"
  nx "the child's checklist continuing: the brief's work is one step, so there is nothing to continue"
}

# ---------------------------------------------------------------------------------------------------------------
s7() {
  echo "S7 upgrade self-reload (0019; spikes 18 to 20; Q16, Q26)"
  "$BASH" "$here/install.sh" "$ENVF" "$VERSION_B" "$binary" --no-activate >/dev/null || { stop_scenario "install of $VERSION_B"; return; }
  local v sid ctr n_before n_after
  # A line only a reloaded skill can give: the run's own staged copies carry a marker each.
  for v in "$VERSION_A" "$VERSION_B"; do
    printf '\nThe crook marker: when the maintainer asks "what is the crook marker", answer with exactly MARKER-%s and nothing else.\n' "$v" \
      >>"$INSTALL/$v/plugin/skills/crook-coordinator/SKILL.md"
  done
  current_is() { [ "$(cd -P "$DATA/crook/current" 2>/dev/null && pwd)" = "$INSTALL/$1" ]; }
  new_flock s7 "Reply with the single word ready. Then wait for the maintainer." || { stop_scenario "the s7 coordinator did not start on $VERSION_A"; return; }
  sid="$(coord_sid s7)"
  expect "a session starts on $VERSION_A: current points at $VERSION_A" current_is "$VERSION_A"
  expect "its SessionStart record says $VERSION_A" rec_is "$sid" "$VERSION_A"
  printf '%s\n' "$VERSION_B" >"$RUN/active"
  expect "$VERSION_B runs once (crook status exits 0)" crook status
  expect "after $VERSION_B runs once, current points at $VERSION_B" current_is "$VERSION_B"
  say s7 "Reply with the single word ok." 120 || { stop_scenario "the send to the coordinator failed"; return; }
  expect "at the next turn end the session's version record moves to $VERSION_B" wait_for 60 rec_is "$sid" "$VERSION_B"
  ctr="$(transcript "$sid")"
  expect "the session was sent /reload-plugins (a user entry in its transcript)" wait_for 60 told_has user "$ctr" '/reload-plugins'
  sleep 5
  note "the screen after the reload: $(hc read s7 --lines 60 | grep -i 'reload' | head -2 | tr -s ' ' | cut -c1-160 | tr '\n' '|')"
  n_before="$(tool_count "$ctr" '^Skill')"
  say s7 "What is the crook marker?" 120 || { stop_scenario "the marker prompt could not be sent"; return; }
  expect "the first prompt after the reload got the one-line nudge (hook output)" told_has hook "$ctr" 'the plugin was reloaded to a newer version'
  n_after="$(tool_count "$ctr" '^Skill')"
  if [ -n "$n_before" ] && [ -n "$n_after" ] && [ "$n_after" -gt "$n_before" ]; then
    case "$(last_said "$ctr")" in
      *MARKER-"$VERSION_B"*) ok "after the nudge the skill was loaded again and answers $VERSION_B" ;;
      *MARKER-"$VERSION_A"*) bad "after the reload the skill still answers $VERSION_A" ;;
      *) nx "the model did not answer with a marker: $(last_said "$ctr" | head -c 100)" ;;
    esac
  else
    nx "the model did not load the skill again after the nudge, so the marker check has nothing to read"
  fi
  printf '%s\n' "$VERSION_A" >"$RUN/active"
  expect "$VERSION_A runs again (crook status exits 0)" crook status
  expect "running $VERSION_A again leaves current on $VERSION_B" current_is "$VERSION_B"
  printf '%s\n' "$VERSION_B" >"$RUN/active"
  note "a debug build leaving current alone is not run here: it is covered by the binary's own tests"
}

# ---------------------------------------------------------------------------------------------------------------
s9() {
  echo "S9 model change on resume, memory (0025; spike 26; Q10)"
  new_flock s9 "Reply with the single word ready. Then wait for the maintainer." || { stop_scenario "the s9 coordinator did not start"; return; }
  spawn_child s9 m "Set the result file's status to done and its Summary to the single word ok." || { stop_scenario "spawn m"; return; }
  wait_for 240 result_is s9 m "done" || { stop_scenario "the child never wrote status done"; return; }
  local word p sid old dir
  word="plum$RANDOM$RANDOM"
  p="$(res s9 m)"
  dir="$(flock_dir s9)"
  say s9.m "Remember this word, and write it nowhere: $word. Reply with the single word ok." 120 || { stop_scenario "the send of the word failed"; return; }
  [ -n "$dir" ] || { stop_scenario "the s9 flock folder is missing"; return; }
  expect "the word is in no file of the flock before the resume" not_in_tree "$word" "$dir"
  sid="$(child_field s9 m session_id)"
  old="$(address_of s9.m)"
  [ -n "$old" ] || { stop_scenario "hc get s9.m failed"; return; }
  if crook resume m --model sonnet --flock s9 >"$RUN_TMP/resume-s9.out" 2>&1; then
    ok "crook resume m --model sonnet works on an idle child"
  else
    bad "crook resume: $(head -c 200 "$RUN_TMP/resume-s9.out")"
  fi
  wait_for 30 new_terminal s9.m "$old"
  expect "the child is on a new terminal under the same name" new_terminal s9.m "$old"
  expect "the roster says sonnet and the child is not done" jq -e '.children[] | select(.name == "m" and .model == "sonnet" and .state == "active")' "$(roster s9)"
  expect "ps argv has --model sonnet and --resume <id>" proc_with_id --resume "$sid" "--model sonnet"
  wait_for 90 state_is s9.m blocked || true
  trust_in s9.m || true
  wait_for 180 state_is s9.m "done"
  say s9.m "What was the word I asked you to remember? Append the line 'Codeword: <the word>' under Summary in $p, then end your turn." 180 || nx "the recall prompt could not be sent"
  expect "after the resume on sonnet, the child recalled the word (it is in the result file)" grep -q -F "Codeword: $word" "$p"
}

# ---------------------------------------------------------------------------------------------------------------
s11() {
  echo "S11 blocked child (0010; spike 3)"
  new_flock s11 "Reply with the single word ready. Then wait for the maintainer." || { stop_scenario "the s11 coordinator did not start"; return; }
  local ctr said
  ctr="$(transcript "$(coord_sid s11)")"
  tr_ok "$ctr" || { stop_scenario "the coordinator's transcript is missing"; return; }
  spawn_child s11 ask 'Use the AskUserQuestion tool to ask the maintainer: "Which size?" with the options S and M. Do not guess the answer. Wait for it.' || { stop_scenario "spawn ask"; return; }
  if ! wait_for 180 state_is s11.ask blocked; then
    nx "the child never asked with AskUserQuestion"
    shot s11.ask
    return
  fi
  ok "hc shows the child as blocked ($(kind_of s11.ask))"
  wait_for 120 told_has hc "$ctr" 's11.ask is blocked' || note "no hc note reached the coordinator within 120 s"
  wait_for 120 state_is s11 "done"
  sleep 5
  expect "the coordinator was told hc's blocked note about the child (an <hc> line in its transcript)" told_has hc "$ctr" 's11.ask is blocked'
  said="$(last_said "$ctr")"
  # The model may write `ask` for the child instead of the hc name, but not a bare word: the name must be quoted or bold.
  # shellcheck disable=SC2016 # backticks are literal characters in the pattern
  if printf '%s' "$said" | grep -qE 's11\.ask|`ask`|\*\*ask\*\*|"ask"'; then ok "the coordinator's last message names the child"; else bad "the coordinator's last message does not name the child (s11.ask or ask in quotes)"; fi
  case "$said" in *[Ss]ize*) ok "and the question" ;; *) bad "and does not carry the question" ;; esac
  note "the coordinator said: $(printf '%s' "$said" | tr '\n' ' ' | cut -c1-240)"
  expect "the coordinator made no hc send to the child (it did not answer for the maintainer)" no_send_in "$ctr"
  expect "and did not use AskUserQuestion itself" no_tool "$ctr" '^AskUserQuestion'
  expect "the child is still blocked: nobody answered it" state_is s11.ask blocked
  crook stop ask --flock s11 >/dev/null 2>&1
}

# ---------------------------------------------------------------------------------------------------------------
s12() {
  echo "S12 step-up question in plain text (0025; Q28)"
  new_flock s12 "Reply with the single word ready. Then wait for the maintainer." || { stop_scenario "the s12 coordinator did not start"; return; }
  local sid ctr said old csid
  sid="$(coord_sid s12)"
  ctr="$(transcript "$sid")"
  tr_ok "$ctr" || { stop_scenario "the coordinator's transcript is missing"; return; }
  spawn_child s12 hard 'You cannot do this task. Set the result file to status failed, and write "too hard for this model" as its Summary.' || { stop_scenario "spawn hard"; return; }
  wait_for 240 result_is s12 hard failed || { stop_scenario "the child never wrote status failed"; return; }
  wait_for 60 mark_is_file s12 hard || note "the failed result was not marked delivered"
  wait_for 180 state_is s12 "done"
  sleep 20
  wait_for 120 state_is s12 "done"
  expect "the coordinator did not use AskUserQuestion for the step-up" no_tool "$ctr" '^AskUserQuestion'
  expect "nothing was resumed on a new model before the maintainer said yes" no_tool "$ctr" ' resume '
  expect "the roster still says haiku" roster_model_is s12 hard haiku
  said="$(last_said "$ctr")"
  note "the coordinator said: $(printf '%s' "$said" | tr '\n' ' ' | cut -c1-300)"
  case "$said" in
    *\?*) if printf '%s' "$said" | grep -qiE 'sonnet|opus|stronger|step.?up|model'; then
        ok "the coordinator's last message is a question about the model, in plain text"
      else
        nx "the coordinator asked a question, but not about the model"
      fi ;;
    *) nx "the coordinator asked no step-up question (it judged the failed brief met)" ;;
  esac
  old="$(address_of s12.hard)"
  [ -n "$old" ] || { stop_scenario "hc get s12.hard failed"; return; }
  # The maintainer types the answer in the coordinator's own pane.
  hc send s12 "Yes. Move the child hard to sonnet with crook resume --model sonnet, then tell it what is missing." >/dev/null 2>&1
  if wait_for 180 roster_model_is s12 hard sonnet; then
    ok "after a yes, the roster's model for the child is sonnet"
    expect "the child is on a new terminal under the same name" wait_for 90 new_terminal s12.hard "$old"
    csid="$(child_field s12 hard session_id)"
    expect "ps argv has --model sonnet and --resume <id>" wait_for 30 proc_with_id --resume "$csid" "--model sonnet"
    wait_for 90 state_is s12.hard blocked || true
    trust_in s12.hard || true
  else
    nx "the coordinator did not resume the child after the yes: $(last_said "$ctr" | tr '\n' ' ' | cut -c1-200)"
  fi
}

# ---------------------------------------------------------------------------------------------------------------
want S1 && s1
want S4 && s4
want S5 && s5
want S6 && s6
want S7 && s7
want S9 && s9
want S11 && s11
want S12 && s12

# ---------------------------------------------------------------------------------------------------------------
SNAP_AFTER="$RUN_TMP/real-after.txt"
snapshot >"$SNAP_AFTER"
echo "real places (mtimes before and after the run):"
# The core's registry is the maintainer's own running core's: it moves with their own terminals, so those two paths
# (the file and its folder) are a note, and the check that counts is that no entry of this run is in it. Everything
# else, `herdr-core` config and the rest of its state included, must not change.
quiet() { grep -v -x -e "[0-9]* $REAL_HOME/.local/state/herdr-core/default" -e "[0-9]* $REGISTRY" "$1"; }
if diff <(quiet "$SNAP_BEFORE") <(quiet "$SNAP_AFTER") >"$RUN_TMP/real.diff"; then
  ok "the real mods folder, crook data, state and config folders and herdr-core config are as they were"
else
  bad "a real place changed during the run:"
  sed 's/^/      /' "$RUN_TMP/real.diff"
fi
diff "$SNAP_BEFORE" "$SNAP_AFTER" >/dev/null || note "the maintainer's own core's registry changed its mtime during the run (it moves with their own terminals)"
if [ ! -f "$REGISTRY" ]; then
  bad "the real core's registry is missing, so it cannot be checked for this run: $REGISTRY"
elif grep -q -e "$RUN" -e "$SESSION" "$REGISTRY"; then
  bad "the real core's registry names this run"
else
  ok "the real core's registry holds no entry of this run"
fi

# D122: the run's sessions used the run's Claude folder, so the maintainer's has no project folder for any of them.
leaked="$(find "$REAL_CFG/projects" -maxdepth 1 -name "*crook-e2e-run*" -newer "$SNAP_BEFORE" 2>/dev/null | head -3)"
if [ -z "$leaked" ]; then
  ok "the maintainer's Claude folder got no transcript or project entry from this run"
else
  bad "the maintainer's Claude folder got project folders from this run: $leaked"
fi
if [ -n "$REAL_CLAUDE" ]; then real_json="$REAL_CLAUDE/.claude.json"; else real_json="$REAL_HOME/.claude.json"; fi
expect "the maintainer's .claude.json names no folder of this run" not_in_tree "$RUN" "$real_json"

echo
echo "tier 3: $PASS passed, $FAIL failed, $SKIP not exercised, $NOTE note(s)"
[ "$FAIL" = 0 ]
