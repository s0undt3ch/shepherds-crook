use std::time::Duration;

use super::*;

fn env_for(dir: &Path) -> impl Fn(&str) -> Option<String> + use<> {
    let x = dir.to_str().unwrap().to_string();
    move |k: &str| (k == "XDG_STATE_HOME").then(|| x.clone())
}

fn fresh() -> (tempfile::TempDir, Store) {
    let tmp = tempfile::tempdir().unwrap();
    let env = env_for(tmp.path());
    init(&env, "p").unwrap();
    let s = Store::open(&env, "p", Mode::ReadWrite).unwrap();
    (tmp, s)
}

fn version(s: &Store) -> i64 {
    s.read(|c| Ok(get(c)?.version)).unwrap()
}

#[test]
fn init_refuses_existing() {
    let tmp = tempfile::tempdir().unwrap();
    let env = env_for(tmp.path());
    init(&env, "p").unwrap();
    let e = init(&env, "p").unwrap_err();
    assert!(e.to_string().contains("already exists"), "{e}");
    assert!(init(&env, "Bad Slug").is_err());
}

#[test]
fn set_version_check() {
    let (_t, mut s) = fresh();
    let v = version(&s);
    let n = s
        .write(Touch::PROJECT, |tx| set_status(tx, "a", v))
        .unwrap();
    assert_eq!(n, v + 1);
    let e = s
        .write(Touch::PROJECT, |tx| set_status(tx, "b", v))
        .unwrap_err();
    assert!(matches!(e, Error::Stale));
    let n2 = s
        .write(Touch::PROJECT, |tx| set_status(tx, "b", v + 1))
        .unwrap();
    assert_eq!(n2, v + 2);
    s.read(|c| {
        assert_eq!(get(c)?.status_text, "b");
        Ok(())
    })
    .unwrap();
}

#[test]
fn set_what_why_checks_version_too() {
    let (_t, mut s) = fresh();
    let v = version(&s);
    s.write(Touch::PROJECT, |tx| set_what_why(tx, "w", "y", v))
        .unwrap();
    let e = s
        .write(Touch::PROJECT, |tx| set_what_why(tx, "w2", "y2", v))
        .unwrap_err();
    assert!(matches!(e, Error::Stale));
    s.read(|c| {
        let r = get(c)?;
        assert_eq!((r.what.as_str(), r.why.as_str()), ("w", "y"));
        Ok(())
    })
    .unwrap();
}

#[test]
fn add_bumps_version() {
    let (_t, mut s) = fresh();
    let v = version(&s);
    let n = s
        .write(Touch::PROJECT, |tx| {
            add(tx, ItemKind::Link, "https://x.test/a")
        })
        .unwrap();
    assert_eq!(n, v + 1);
    assert_eq!(version(&s), v + 1);
    // A set made with the pre-add version is stale.
    let e = s
        .write(Touch::PROJECT, |tx| set_status(tx, "z", v))
        .unwrap_err();
    assert!(matches!(e, Error::Stale));
}

#[test]
fn set_resets_stale_counters() {
    let (_t, mut s) = fresh();
    s.write(Touch::PROJECT, |tx| {
        tx.execute(
            "UPDATE project SET dones_since_card = 4, card_updated_at = 1 WHERE id = 1",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    let before = now();
    let v = version(&s);
    std::thread::sleep(Duration::from_millis(5));
    s.write(Touch::PROJECT, |tx| set_what_why(tx, "w", "y", v))
        .unwrap();
    s.read(|c| {
        let r = get(c)?;
        assert_eq!(r.dones_since_card, 0);
        assert!(r.card_updated_at >= before);
        Ok(())
    })
    .unwrap();
}

#[test]
fn add_does_not_reset_stale_counters() {
    let (_t, mut s) = fresh();
    s.write(Touch::PROJECT, |tx| {
        tx.execute("UPDATE project SET dones_since_card = 2 WHERE id = 1", [])?;
        add(tx, ItemKind::Ticket, "T-1")
    })
    .unwrap();
    s.read(|c| {
        assert_eq!(get(c)?.dones_since_card, 2);
        Ok(())
    })
    .unwrap();
}

#[test]
fn add_notes_is_private() {
    let (_t, mut s) = fresh();
    s.write(Touch::PROJECT, |tx| {
        add(tx, ItemKind::Notes, "/n/./vault/../notes/")
    })
    .unwrap();
    s.read(|c| {
        let r = get(c)?;
        assert_eq!(r.notes, ["/n/notes"]);
        assert_eq!(r.private, ["/n/notes"]);
        Ok(())
    })
    .unwrap();
}

#[test]
fn items_keep_order_and_ignore_duplicates() {
    let (_t, mut s) = fresh();
    for v in ["b", "a", "b"] {
        s.write(Touch::PROJECT, |tx| add(tx, ItemKind::Ticket, v))
            .unwrap();
    }
    s.read(|c| {
        assert_eq!(get(c)?.tickets, ["b", "a"]);
        Ok(())
    })
    .unwrap();
}

#[test]
fn repo_paths_are_absolute_and_cleaned() {
    assert_eq!(clean_path("/a//b/./c/../d/").unwrap(), "/a/b/d");
    assert_eq!(clean_path("/../x").unwrap(), "/x");
    assert!(clean_path("rel/x").is_err());
    let (_t, mut s) = fresh();
    assert!(
        s.write(Touch::PROJECT, |tx| add(tx, ItemKind::Repo, "rel"))
            .is_err()
    );
    assert!(
        s.write(Touch::PROJECT, |tx| add(tx, ItemKind::Link, "  "))
            .is_err()
    );
}

#[test]
fn remove_deletes_and_bumps() {
    let (_t, mut s) = fresh();
    s.write(Touch::PROJECT, |tx| add(tx, ItemKind::Repo, "/r/./x"))
        .unwrap();
    let v = version(&s);
    let n = s
        .write(Touch::PROJECT, |tx| remove(tx, ItemKind::Repo, "/r/x/"))
        .unwrap();
    assert_eq!(n, v + 1);
    s.read(|c| {
        assert!(get(c)?.repos.is_empty());
        Ok(())
    })
    .unwrap();
    let e = s
        .write(Touch::PROJECT, |tx| remove(tx, ItemKind::Repo, "/r/x"))
        .unwrap_err();
    assert!(matches!(e, Error::Refused(_)));
    assert_eq!(version(&s), v + 1, "a refused remove changes nothing");
}

#[test]
fn add_refuses_a_relative_private_entry() {
    let (_t, mut s) = fresh();
    for rel in ["rel/x", "x", "./y/**"] {
        let e = s
            .write(Touch::PROJECT, |tx| add(tx, ItemKind::Private, rel))
            .unwrap_err();
        assert!(e.to_string().contains("not absolute"), "{e}");
    }
    for ok in ["/abs/x", "~/v", "$HOME/w", "${HOME}"] {
        s.write(Touch::PROJECT, |tx| add(tx, ItemKind::Private, ok))
            .unwrap();
    }
}

#[test]
fn private_notes_entry_cannot_be_removed_alone() {
    let (_t, mut s) = fresh();
    s.write(Touch::PROJECT, |tx| add(tx, ItemKind::Notes, "/n/x"))
        .unwrap();
    let v = version(&s);
    let e = s
        .write(Touch::PROJECT, |tx| remove(tx, ItemKind::Private, "/n/x"))
        .unwrap_err();
    assert!(e.to_string().contains("remove the notes folder"), "{e}");
    assert_eq!(version(&s), v);
    s.write(Touch::PROJECT, |tx| remove(tx, ItemKind::Notes, "/n/x"))
        .unwrap();
    s.read(|c| {
        let r = get(c)?;
        assert!(r.notes.is_empty() && r.private.is_empty());
        Ok(())
    })
    .unwrap();
}

#[test]
fn mod_addable_kinds() {
    for (k, want) in [
        ("repo", true),
        ("notes", true),
        ("link", true),
        ("ticket", true),
        ("private", false),
        ("allow", false),
    ] {
        assert_eq!(k.parse::<ItemKind>().unwrap().mod_addable(), want, "{k}");
    }
}

#[test]
fn kind_round_trips() {
    for k in ["repo", "notes", "private", "allow", "link", "ticket"] {
        assert_eq!(k.parse::<ItemKind>().unwrap().to_string(), k);
    }
    assert!("memory".parse::<ItemKind>().is_err());
}

#[test]
fn add_repo_stores_the_resolved_path() {
    let d = tempfile::tempdir().unwrap();
    let real = std::fs::canonicalize(d.path()).unwrap();
    std::fs::create_dir(real.join("repo")).unwrap();
    std::os::unix::fs::symlink(&real, real.join("link")).unwrap();
    let (_tmp, mut s) = fresh();
    let spelt = format!("{}//repo", real.join("link").display());
    let plain = real.join("repo").display().to_string();
    s.write(Touch::PROJECT, |tx| {
        add(tx, ItemKind::Repo, &spelt)?;
        add(tx, ItemKind::Repo, &plain).map(|_| ())
    })
    .unwrap();
    assert_eq!(s.read(|c| Ok(get(c)?.repos)).unwrap(), [plain]);
}

#[test]
fn remove_finds_a_repo_stored_before_paths_were_resolved() {
    let d = tempfile::tempdir().unwrap();
    let real = std::fs::canonicalize(d.path()).unwrap();
    std::fs::create_dir(real.join("repo")).unwrap();
    std::os::unix::fs::symlink(&real, real.join("link")).unwrap();
    let old = real.join("link/repo").display().to_string();
    let (_tmp, mut s) = fresh();
    s.write(Touch::PROJECT, |tx| {
        tx.execute(
            "INSERT INTO project_item(kind, value, pos) VALUES ('repo', ?1, 0)",
            [&old],
        )?;
        remove(tx, ItemKind::Repo, &old).map(|_| ())
    })
    .unwrap();
    assert!(s.read(|c| Ok(get(c)?.repos)).unwrap().is_empty());
}

#[test]
fn remove_finds_a_repo_gone_from_disk_by_its_spelling() {
    let d = tempfile::tempdir().unwrap();
    let real = std::fs::canonicalize(d.path()).unwrap();
    std::fs::create_dir(real.join("repo")).unwrap();
    std::os::unix::fs::symlink(&real, real.join("link")).unwrap();
    let spelt = real.join("link/repo").display().to_string();
    let (_tmp, mut s) = fresh();
    s.write(Touch::PROJECT, |tx| {
        add(tx, ItemKind::Repo, &spelt).map(|_| ())
    })
    .unwrap();
    std::fs::remove_dir(real.join("repo")).unwrap();
    s.write(Touch::PROJECT, |tx| {
        remove(tx, ItemKind::Repo, &spelt).map(|_| ())
    })
    .unwrap();
    assert!(s.read(|c| Ok(get(c)?.repos)).unwrap().is_empty());
}
