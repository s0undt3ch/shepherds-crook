use super::*;

fn vars(names: &[&str]) -> Vec<(OsString, OsString)> {
    names
        .iter()
        .map(|n| ((*n).into(), format!("value of {n}").into()))
        .collect()
}

fn names(vars: &[(OsString, OsString)]) -> Vec<&str> {
    vars.iter().filter_map(|(k, _)| k.to_str()).collect()
}

#[test]
fn every_variable_in_adr_0020_is_removed() {
    let all = [
        "CLAUDECODE",
        "CLAUDE_PID",
        "CLAUDE_EFFORT",
        "CLAUDE_CODE_SESSION_ID",
        "CLAUDE_CODE_SESSION_ACCESS_TOKEN",
        "CLAUDE_CODE_CHILD_SESSION",
        "CLAUDE_CODE_MESSAGING_SOCKET",
        "CLAUDE_CODE_ENTRYPOINT",
        "CLAUDE_CODE_EXECPATH",
        "AI_AGENT",
        "HC_TERMINAL_ID",
        "HC_TERMINAL_SOCKET",
        "HC_SCOPE",
    ];
    assert!(scrub(vars(&all)).is_empty());
}

#[test]
fn everything_else_is_kept_in_order_with_its_value() {
    let pair = |k: &str, v: &str| (OsString::from(k), OsString::from(v));
    let input = vec![
        pair("PATH", "/usr/bin"),
        pair("CLAUDECODE", "1"),
        pair("CLAUDE_CONFIG_DIR", "/c"),
        pair("HC_SESSION", "scratch"),
    ];
    assert_eq!(
        scrub(input),
        vec![
            pair("PATH", "/usr/bin"),
            pair("CLAUDE_CONFIG_DIR", "/c"),
            pair("HC_SESSION", "scratch"),
        ]
    );
}

#[test]
fn user_settings_and_scratch_core_variables_are_kept() {
    let kept = [
        "CLAUDE_CONFIG_DIR",
        "CLAUDE_CODE_ENABLE_TELEMETRY",
        "CLAUDE_CODE_SUBAGENT_MODEL",
        "CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY",
        "HC_SESSION",
        "PATH",
        "HOME",
        "XDG_STATE_HOME",
        "XDG_RUNTIME_DIR",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "CROOK_BIN",
    ];
    assert_eq!(names(&scrub(vars(&kept))), kept);
}

#[test]
fn names_only_match_whole_or_by_the_listed_prefix() {
    let kept = [
        "CLAUDE_PIDS",
        "MY_CLAUDECODE",
        "CLAUDE_CODE_SESSION",
        "CLAUDE_CODE_CHILD",
        "CLAUDE_CODE_ENTRYPOINT_X",
        "HC_TERMINAL_IDS",
        "HC_SCOPES",
        "claudecode",
        "AI_AGENTS",
        "MY_AI_AGENT",
    ];
    assert_eq!(names(&scrub(vars(&kept))), kept);
}

#[cfg(unix)]
#[test]
fn variables_that_are_not_text_pass_through_unchanged() {
    use std::os::unix::ffi::OsStringExt;
    let odd_name = OsString::from_vec(vec![b'X', 0xff, b'Y']);
    let odd_value = OsString::from_vec(vec![0xfe, 0xfd]);
    let input = vec![
        (odd_name.clone(), "text".into()),
        ("PLAIN".into(), odd_value.clone()),
        ("CLAUDECODE".into(), odd_value.clone()),
    ];
    assert_eq!(
        scrub(input),
        vec![
            (odd_name, OsString::from("text")),
            (OsString::from("PLAIN"), odd_value)
        ]
    );
}
