#!/bin/bash
# Stand-in for the crook shim in eval runs. It logs every call's arguments and prints
# output shaped like the real command. It never starts a session and never calls hc.
# __WORKSPACE__ is filled in by install.sh, which allows only plain path characters.
workspace='__WORKSPACE__'
printf '%s\n' "$*" >>"$workspace/.crook-calls.log"
case "$1" in
status)
    # A case with a child scaffolds its `children` array here.
    children='[]'
    [ -f "$workspace/.stub-children.json" ] && children=$(cat "$workspace/.stub-children.json")
    cat <<JSON
{"version":1,"crook_version":"0.0.0-dev","flocks":[{"slug":"demo","space":"demo","folder":"$workspace","notes_folder":"$workspace","state":"active","coordinator":{"session_id":"11111111-1111-4111-8111-111111111111","terminal":{"id":"t1","state":"working","app":"claude-code"}},"children":$children}]}
JSON
    ;;
spawn)
    # spawn child <name> <brief> ...: the brief must be a file under the flock's briefs/, as in the real command.
    case "$4" in
    "$workspace"/briefs/?*.md) ;;
    *)
        echo "error: the brief must be a file under $workspace/briefs/" >&2
        exit 1
        ;;
    esac
    if [ ! -f "$4" ]; then
        echo "error: the brief $4 does not exist" >&2
        exit 1
    fi
    printf 'spawned demo.%s (terminal t2, session 22222222-2222-4222-8222-222222222222)\n' "$3"
    ;;
stop)
    printf 'stopped demo.%s\n' "$2"
    ;;
resume)
    printf 'resumed demo.%s (terminal t3)\n' "$2"
    ;;
hook)
    # The coordinator's SessionStart context names the literal path to call, as the real hook does
    # (ADR 0027). The text must stay launch::call_rule's; a Rust test compares them.
    if [ "$2" = session-start ] && [ -f "$workspace/.stub-coordinator" ]; then
        # shellcheck disable=SC2016 # the backticks are text
        rule='Run every crook command as \"%s\" <subcommand> ..., with that exact quoted path, as its own Bash command: nothing before it and no pipe, `&&` or `;` after it. Only that form runs without asking the maintainer.'
        context=${rule//'%s'/$0}
        printf '{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"crook: %s"}}\n' "$context"
    fi
    exit 0
    ;;
esac
exit 0
