use super::*;

const FIXTURE: &str = include_str!("../../tests/rust/fixtures/agents.json");

#[test]
fn agents_both_keys() {
    let a = parse(FIXTURE).unwrap();
    let by = |sid: &str| a.iter().find(|x| x.session_id == sid).unwrap();
    // Interactive rows carry `status`.
    assert_eq!(by("aaaa-interactive").state.as_deref(), Some("idle"));
    assert_eq!(by("aaaa-interactive").pid, Some(4242));
    assert_eq!(by("aaaa-interactive").kind, "interactive");
    // Background rows carry `state` and no pid.
    assert_eq!(by("bbbb-background").state.as_deref(), Some("blocked"));
    assert_eq!(by("bbbb-background").pid, None);
    assert_eq!(by("bbbb-background").cwd, "/work/repo/.worktrees/c2");
    assert_eq!(a.len(), 4);
}

#[test]
fn state_wins_over_status_and_neither_is_none() {
    let a = parse(
        r#"[{"sessionId":"s1","cwd":"/a","kind":"k","state":"waiting","status":"idle"},
            {"sessionId":"s2","cwd":"/a","kind":"k"}]"#,
    )
    .unwrap();
    assert_eq!(a[0].state.as_deref(), Some("waiting"));
    assert_eq!(a[1].state, None);
}

#[test]
fn rows_without_a_session_id_keep_their_cwd() {
    let a = parse(r#"[{"cwd":"/a"},{"sessionId":"s","cwd":"/b"}]"#).unwrap();
    assert_eq!(a.len(), 2);
    assert_eq!((a[0].session_id.as_str(), a[0].cwd.as_str()), ("", "/a"));
}

#[test]
fn a_row_that_does_not_read_fails_the_list() {
    for json in [
        r#"[{"sessionId":"s","pid":"4242","cwd":"/a"},{"sessionId":"t","cwd":"/b"}]"#,
        r#"[{"sessionId":7,"cwd":"/a"}]"#,
        r#"[{"sessionId":"s","cwd":"/a"},"nope"]"#,
    ] {
        let e = parse(json).unwrap_err();
        assert!(e.to_string().contains("unreadable row"), "{json}: {e}");
    }
}

#[test]
fn not_an_array_is_an_error() {
    assert!(matches!(parse("{}"), Err(Error::Agents(_))));
    assert!(matches!(parse("nope"), Err(Error::Agents(_))));
}

#[cfg(unix)]
#[test]
fn list_runs_claude_from_path_only() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("claude");
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/rust/fixtures/agents.json"
    );
    std::fs::write(
        &bin,
        format!(
            "#!/bin/sh\n[ \"$1 $2\" = \"agents --json\" ] || exit 3\nexec /bin/cat {fixture}\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let p = tmp.path().display().to_string();
    let env = move |k: &str| (k == "PATH").then(|| p.clone());
    assert_eq!(list(&env).unwrap().len(), 4);

    let empty = tempfile::tempdir().unwrap();
    let p = empty.path().display().to_string();
    let env = move |k: &str| (k == "PATH").then(|| p.clone());
    assert!(matches!(list(&env), Err(Error::Agents(_))));
}

#[cfg(unix)]
#[test]
fn list_gives_up_on_a_hung_claude() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("claude");
    std::fs::write(&bin, "#!/bin/sh\nexec /bin/sleep 30\n").unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let p = tmp.path().display().to_string();
    let env = move |k: &str| (k == "PATH").then(|| p.clone());
    let t = Instant::now();
    let e = list_within(&env, Duration::from_millis(300)).unwrap_err();
    assert!(e.to_string().contains("timed out"), "{e}");
    assert!(t.elapsed() < Duration::from_secs(5));
}

#[cfg(unix)]
#[test]
fn large_output_does_not_block_the_child() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("claude");
    // ~200 KB, past the 64 KB pipe buffer.
    std::fs::write(
        &bin,
        "#!/bin/sh\nprintf '['\ni=0; while [ $i -lt 4000 ]; do printf '{\"sessionId\":\"s%s\",\"cwd\":\"/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"},' $i; i=$((i+1)); done\nprintf '{\"sessionId\":\"last\"}]'\n",
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let p = tmp.path().display().to_string();
    let env = move |k: &str| (k == "PATH").then(|| p.clone());
    assert_eq!(list(&env).unwrap().len(), 4001);
}

#[cfg(unix)]
#[test]
fn claude_version_reads_the_first_word() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("claude");
    std::fs::write(&bin, "#!/bin/sh\necho '2.1.329 (Claude Code)'\n").unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let p = tmp.path().display().to_string();
    let env = move |k: &str| (k == "PATH").then(|| p.clone());
    assert_eq!(claude_version(&env).unwrap(), "2.1.329");
}

#[test]
fn short_id_is_the_first_part() {
    assert_eq!(short_id("40bf3692-9587-4e8e-9b24-85b53ecbedf1"), "40bf3692");
    assert_eq!(short_id("plain"), "plain");
}
