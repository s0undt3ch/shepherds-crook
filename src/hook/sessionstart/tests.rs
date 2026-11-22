use super::{claude_name, judge};

#[test]
fn ps_that_cannot_tell_is_the_child() {
    assert!(judge(None), "no ps ran");
    assert!(judge(Some((false, String::new()))), "ps failed");
    assert!(judge(Some((true, "  \n".into()))), "ps printed nothing");
    assert!(judge(Some((true, "/usr/bin/claude\n".into()))));
    assert!(!judge(Some((true, "sleep\n".into()))));
}

#[test]
fn claude_name_is_exact() {
    for yes in [
        "claude",
        "/usr/local/bin/claude",
        "claude-code",
        "claude.exe",
    ] {
        assert!(claude_name(yes), "{yes}");
    }
    for no in ["claudette", "notclaude", "zsh", "", "claude2"] {
        assert!(!claude_name(no), "{no}");
    }
}
