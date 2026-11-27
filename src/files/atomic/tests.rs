use std::path::Path;

use serde_json::{Value, json};

use super::*;

fn read(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).expect("reads")).expect("json")
}

#[test]
fn write_creates_and_replaces_with_pretty_json_and_a_newline() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("a.json");
    write_json_atomic(&path, &json!({"n": 1})).expect("writes");
    write_json_atomic(&path, &json!({"n": 2})).expect("writes");
    let text = std::fs::read_to_string(&path).expect("reads");
    assert!(text.ends_with("}\n"), "{text:?}");
    assert_eq!(read(&path), json!({"n": 2}));
}

#[test]
fn a_successful_write_leaves_no_temporary_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_json_atomic(&dir.path().join("a.json"), &json!({})).expect("writes");
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .expect("lists")
        .map(|e| e.expect("entry").file_name())
        .collect();
    assert_eq!(names, ["a.json"]);
}

#[test]
fn a_crash_between_write_and_rename_leaves_the_target_untouched() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("a.json");
    write_json_atomic(&path, &json!({"old": true})).expect("writes");
    // The first half of a write, with the process dying before the rename.
    let tmp = write_temp(&path, b"{\"new\": true}\n").expect("temp");
    assert!(tmp.exists());
    assert_ne!(tmp, path);
    assert_eq!(tmp.parent(), path.parent());
    assert_eq!(read(&path), json!({"old": true}));
}

#[test]
fn a_failed_write_reports_the_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("missing-folder/a.json");
    let msg = write_json_atomic(&path, &json!({}))
        .expect_err("fails")
        .to_string();
    assert!(msg.contains("missing-folder"), "{msg}");
}

#[test]
fn the_lock_creates_its_file_and_returns_the_result() {
    let dir = tempfile::tempdir().expect("tempdir");
    let n = with_flock_lock(dir.path(), || Ok(7)).expect("locks");
    assert_eq!(n, 7);
    assert!(dir.path().join(".lock").exists());
}

#[test]
fn an_error_from_the_closure_passes_through_and_releases_the_lock() {
    let dir = tempfile::tempdir().expect("tempdir");
    let err =
        with_flock_lock::<()>(dir.path(), || Err(Error::Flock("boom".into()))).expect_err("fails");
    assert_eq!(err.to_string(), "boom");
    with_flock_lock(dir.path(), || Ok(())).expect("lock is free again");
}

#[test]
fn threads_adding_keys_under_the_lock_all_survive() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("data.json");
    write_json_atomic(&path, &json!({})).expect("writes");
    std::thread::scope(|scope| {
        for t in 0..4 {
            let (folder, path) = (dir.path(), &path);
            scope.spawn(move || {
                for i in 0..25 {
                    with_flock_lock(folder, || {
                        let mut value = read(path);
                        value[format!("t{t}-{i}")] = json!(true);
                        // Widen the window in which an unlocked writer would lose an update.
                        std::thread::yield_now();
                        write_json_atomic(path, &value)
                    })
                    .expect("cycle");
                }
            });
        }
    });
    assert_eq!(read(&path).as_object().expect("object").len(), 100);
}

#[test]
fn a_taken_temporary_name_is_skipped() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("a.json");
    // Stale files from other runs, even ones that look like ours, never block a write.
    for i in 0..3 {
        std::fs::write(
            dir.path()
                .join(format!(".a.json.tmp-{}-0-{i}", std::process::id())),
            "stale",
        )
        .expect("write");
    }
    let one = write_temp(&path, b"1").expect("temp");
    let two = write_temp(&path, b"2").expect("temp");
    assert_ne!(one, two);
    assert_eq!(std::fs::read_to_string(&one).expect("reads"), "1");
    assert_eq!(std::fs::read_to_string(&two).expect("reads"), "2");
}
