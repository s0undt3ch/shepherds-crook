use std::io::Cursor;

use super::*;

fn env_for(dir: &Path) -> impl Fn(&str) -> Option<String> + use<> {
    let x = dir.to_str().unwrap().to_string();
    move |k: &str| (k == "XDG_STATE_HOME").then(|| x.clone())
}

#[test]
fn cli_project_version_check() {
    let tmp = tempfile::tempdir().unwrap();
    let env = env_for(tmp.path());
    project::init(&env, "p").unwrap();
    let mut cli = Store::open(&env, "p", Mode::ReadWrite).unwrap();
    let mut tool = Store::open(&env, "p", Mode::ReadWrite).unwrap();

    // The CLI reads the version, then a tool write lands before its own.
    let seen = version(&cli).unwrap();
    tool.write(Touch::PROJECT, |tx| {
        project::set_status(tx, "from the tool", seen)
    })
    .unwrap();
    let e = set_status_at(&mut cli, "from the cli", seen).unwrap_err();
    assert!(matches!(e, Error::Stale), "{e}");
    cli.read(|c| {
        assert_eq!(project::get(c)?.status_text, "from the tool");
        Ok(())
    })
    .unwrap();
}

#[test]
fn set_commands_read_stdin_and_keep_the_other_field() {
    let tmp = tempfile::tempdir().unwrap();
    let env = env_for(tmp.path());
    project::init(&env, "p").unwrap();
    let cwd = Path::new("/");
    for (cmd, text) in [
        (ProjectCmd::SetWhat, "the what\n"),
        (ProjectCmd::SetWhy, "the why\n"),
        (ProjectCmd::SetStatus, "the status\n"),
    ] {
        run(&env, "p", cmd, cwd, &mut Cursor::new(text)).unwrap();
    }
    let s = Store::open(&env, "p", Mode::ReadWrite).unwrap();
    s.read(|c| {
        let r = project::get(c)?;
        assert_eq!(
            (r.what.as_str(), r.why.as_str(), r.status_text.as_str()),
            ("the what", "the why", "the status")
        );
        Ok(())
    })
    .unwrap();
    let e = run(
        &env,
        "p",
        ProjectCmd::SetStatus,
        cwd,
        &mut Cursor::new("  \n"),
    )
    .unwrap_err();
    assert!(e.to_string().contains("no text"), "{e}");
}

#[test]
fn relative_repo_paths_resolve_against_cwd() {
    let tmp = tempfile::tempdir().unwrap();
    let env = env_for(tmp.path());
    project::init(&env, "p").unwrap();
    run(
        &env,
        "p",
        ProjectCmd::AddRepo {
            path: "../other/./repo".into(),
        },
        Path::new("/work/here"),
        &mut Cursor::new(""),
    )
    .unwrap();
    let s = Store::open(&env, "p", Mode::ReadWrite).unwrap();
    s.read(|c| {
        assert_eq!(project::get(c)?.repos, ["/work/other/repo"]);
        Ok(())
    })
    .unwrap();
}

#[test]
fn relative_private_entries_resolve_against_cwd() {
    let tmp = tempfile::tempdir().unwrap();
    let env = env_for(tmp.path());
    project::init(&env, "p").unwrap();
    let cwd = Path::new("/work/here");
    for value in [
        "../notes/./secret/**",
        "plain",
        "/abs/../abs/x/",
        "~/vault",
        "$HOME/v",
        "${HOME}/w",
    ] {
        let cmd = ProjectCmd::AddPrivate {
            value: value.into(),
        };
        run(&env, "p", cmd, cwd, &mut Cursor::new("")).unwrap();
    }
    let s = Store::open(&env, "p", Mode::ReadWrite).unwrap();
    let private = s.read(|c| Ok(project::get(c)?.private)).unwrap();
    assert_eq!(
        private,
        [
            "/work/notes/secret/**",
            "/work/here/plain",
            "/abs/x",
            "~/vault",
            "$HOME/v",
            "${HOME}/w",
        ]
    );
    drop(s);
    let cmd = ProjectCmd::AddPrivate { value: " ".into() };
    let e = run(&env, "p", cmd, cwd, &mut Cursor::new("")).unwrap_err();
    assert!(e.to_string().contains("empty value"), "{e}");
    // `remove` resolves the same way.
    let cmd = ProjectCmd::Remove {
        kind: ItemKind::Private,
        value: "plain".into(),
    };
    run(&env, "p", cmd, cwd, &mut Cursor::new("")).unwrap();
    let s = Store::open(&env, "p", Mode::ReadWrite).unwrap();
    let private = s.read(|c| Ok(project::get(c)?.private)).unwrap();
    assert!(!private.iter().any(|p| p.ends_with("plain")), "{private:?}");
}

#[test]
fn legacy_relative_private_entry_can_be_removed() {
    let tmp = tempfile::tempdir().unwrap();
    let env = env_for(tmp.path());
    project::init(&env, "p").unwrap();
    let mut s = Store::open(&env, "p", Mode::ReadWrite).unwrap();
    s.write(Touch::PROJECT, |tx| {
        tx.execute(
            "INSERT INTO project_item(kind, value, pos) VALUES ('private', 'rel/x', 0)",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    drop(s);
    let cmd = ProjectCmd::Remove {
        kind: ItemKind::Private,
        value: "rel/x".into(),
    };
    run(&env, "p", cmd, Path::new("/work"), &mut Cursor::new("")).unwrap();
    let s = Store::open(&env, "p", Mode::ReadWrite).unwrap();
    assert!(s.read(|c| Ok(project::get(c)?.private)).unwrap().is_empty());
}
