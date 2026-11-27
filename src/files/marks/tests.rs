use super::*;

fn folder() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(dir.path().join("results")).expect("mkdir");
    dir
}

#[test]
fn a_missing_file_is_an_empty_map() {
    let dir = folder();
    assert!(load(dir.path()).expect("loads").is_empty());
}

#[test]
fn marks_round_trip_and_replace_per_child() {
    let dir = folder();
    mark(dir.path(), "a", "h1").expect("marks");
    mark(dir.path(), "b", "h2").expect("marks");
    mark(dir.path(), "a", "h3").expect("marks");
    let marks = load(dir.path()).expect("loads");
    assert_eq!(marks.get("a").map(String::as_str), Some("h3"));
    assert_eq!(marks.get("b").map(String::as_str), Some("h2"));
    assert_eq!(marks.len(), 2);
}

#[test]
fn the_file_is_a_plain_json_map() {
    let dir = folder();
    mark(dir.path(), "a", "h1").expect("marks");
    let text = std::fs::read_to_string(path(dir.path())).expect("reads");
    let value: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert_eq!(value, serde_json::json!({"a": "h1"}));
}

#[test]
fn a_broken_file_is_an_error_naming_it() {
    let dir = folder();
    std::fs::write(path(dir.path()), "[1]").expect("write");
    let msg = load(dir.path()).expect_err("fails").to_string();
    assert!(msg.contains(".delivered.json"), "{msg}");
}

#[test]
fn concurrent_marks_all_survive() {
    let dir = folder();
    std::thread::scope(|scope| {
        for t in 0..8 {
            let folder = dir.path();
            scope.spawn(move || mark(folder, &format!("c{t}"), "h").expect("marks"));
        }
    });
    assert_eq!(load(dir.path()).expect("loads").len(), 8);
}
