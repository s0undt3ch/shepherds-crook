//! Hook output JSON. Field names are from the Claude Code hooks reference
//! (https://code.claude.com/docs/en/hooks): `PreToolUse` returns its decision
//! inside `hookSpecificOutput` as `permissionDecision` and
//! `permissionDecisionReason`; `SessionStart` returns `additionalContext`
//! there. No output at all means "no decision".

use serde_json::json;

pub fn ask(reason: &str) -> String {
    pre_tool_use("ask", reason)
}

pub fn deny(reason: &str) -> String {
    pre_tool_use("deny", reason)
}

fn pre_tool_use(decision: &str, reason: &str) -> String {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": decision,
            "permissionDecisionReason": reason,
        }
    })
    .to_string()
}

pub fn session_context(text: &str) -> String {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": text,
        }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
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
}
