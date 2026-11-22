use serde_json::{Value, json};

use super::*;

fn parse(s: &str) -> Value {
    serde_json::from_str(s).unwrap()
}

#[test]
fn pre_tool_use_shape_is_pinned() {
    assert_eq!(
        parse(&ask("why")),
        json!({"hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "ask",
            "permissionDecisionReason": "why"}})
    );
    assert_eq!(
        parse(&deny("no"))["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );
}

#[test]
fn session_start_shape_is_pinned() {
    assert_eq!(
        parse(&session_context("card")),
        json!({"hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": "card"}})
    );
}
