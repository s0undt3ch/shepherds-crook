use super::*;
use crate::files::result::sha256_hex;
use crate::hc::fake::FakeHc;
use crate::hook::version::{Record, save};
use crate::testutil::{
    Buffers, Fixture, SESSION, hook_input, make_flock, run_hook, start_coordinator,
};

struct Rig {
    fx: Fixture,
    hc: FakeHc,
    flock: Flock,
    coordinator: String,
}

fn rig() -> Rig {
    let fx = Fixture::new();
    fx.set_plugin_version("1.0.0");
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    let coordinator = start_coordinator(&hc, &flock);
    Rig {
        fx,
        hc,
        flock,
        coordinator,
    }
}

fn child_terminal(r: &Rig, name: &str) -> String {
    use crate::hc::{Hc, Parent, RunSpec};
    r.hc.run(&RunSpec {
        name: format!("demo.{name}"),
        cwd: r.flock.folder.clone(),
        labels: vec![
            ("crook.flock".into(), "demo".into()),
            ("crook.child".into(), name.into()),
        ],
        parent: Parent::Terminal(r.coordinator.clone()),
        argv: vec!["claude".into()],
    })
    .expect("run")
    .address
}

fn prompt(r: &Rig, terminal: &str) -> Option<String> {
    let ran = run_hook(
        &r.fx,
        &r.hc,
        "user-prompt-submit",
        Some(terminal),
        &hook_input(SESSION),
    );
    assert_eq!(ran.stderr, "");
    ran.context()
}

fn write(r: &Rig, name: &str, text: &str) {
    std::fs::write(r.flock.result_path(name), text).expect("result");
}

fn path(r: &Rig, name: &str) -> String {
    r.flock.result_path(name).display().to_string()
}

#[test]
fn two_changed_results_give_two_lines_and_two_marks() {
    let r = rig();
    write(&r, "a", "---\nstatus: done\n---\none\n");
    write(&r, "b", "---\nstatus: working\n---\ntwo\n");
    let context = prompt(&r, &r.coordinator).expect("context");
    assert_eq!(
        context,
        format!(
            "Undelivered child result: {}\nUndelivered child result: {}",
            path(&r, "a"),
            path(&r, "b")
        )
    );
    let marks = marks::load(&r.flock.folder).expect("marks");
    assert_eq!(
        marks.get("a"),
        Some(&sha256_hex(b"---\nstatus: done\n---\none\n"))
    );
    assert_eq!(
        marks.get("b"),
        Some(&sha256_hex(b"---\nstatus: working\n---\ntwo\n"))
    );
}

#[test]
fn no_change_gives_no_output_and_a_change_gives_only_that_line() {
    let r = rig();
    assert_eq!(prompt(&r, &r.coordinator), None, "no results at all");
    write(&r, "a", "---\nstatus: done\n---\n");
    prompt(&r, &r.coordinator).expect("first");
    assert_eq!(prompt(&r, &r.coordinator), None);
    write(&r, "a", "---\nstatus: done\n---\nmore\n");
    assert_eq!(
        prompt(&r, &r.coordinator).expect("changed"),
        format!("Undelivered child result: {}", path(&r, "a"))
    );
}

#[test]
fn dot_files_and_other_files_are_skipped() {
    let r = rig();
    for name in [".hidden.md", "notes.txt"] {
        std::fs::write(r.flock.folder.join("results").join(name), "x").expect("write");
    }
    std::fs::create_dir(r.flock.folder.join("results/dir.md")).expect("dir");
    assert_eq!(prompt(&r, &r.coordinator), None);
}

#[test]
fn an_unreadable_result_is_still_reported() {
    let r = rig();
    write(&r, "a", "no frontmatter");
    assert_eq!(
        prompt(&r, &r.coordinator).expect("context"),
        format!("Undelivered child result: {}", path(&r, "a"))
    );
    // Not even text: reported by its hash.
    std::fs::write(r.flock.result_path("b"), [0xff, 0xfe, 0x00]).expect("write");
    assert!(
        prompt(&r, &r.coordinator)
            .expect("context")
            .ends_with(&path(&r, "b"))
    );
}

#[test]
fn a_child_session_gets_no_result_lines() {
    let r = rig();
    write(&r, "a", "---\nstatus: done\n---\n");
    let child = child_terminal(&r, "a");
    assert_eq!(prompt(&r, &child), None);
    assert!(
        marks::load(&r.flock.folder).expect("marks").is_empty(),
        "no mark either"
    );
}

#[test]
fn after_a_reload_the_first_prompt_gets_the_nudge_and_the_second_does_not() {
    let r = rig();
    let child = child_terminal(&r, "a");
    for (terminal, skill) in [
        (r.coordinator.clone(), "crook-coordinator"),
        (child, "crook-child"),
    ] {
        let session = format!("{SESSION}-{skill}");
        save(
            &r.fx.ctx(),
            &session,
            &Record {
                version: "1.0.0".into(),
                reloaded: true,
            },
        )
        .expect("save");
        let ran = run_hook(
            &r.fx,
            &r.hc,
            "user-prompt-submit",
            Some(&terminal),
            &hook_input(&session),
        );
        let context = ran.context().expect("nudge");
        assert!(context.contains(&format!("`{skill}`")), "{context}");
        let ran = run_hook(
            &r.fx,
            &r.hc,
            "user-prompt-submit",
            Some(&terminal),
            &hook_input(&session),
        );
        assert_eq!(ran.stdout, "", "second prompt");
    }
}

#[test]
fn the_nudge_comes_before_the_result_lines() {
    let r = rig();
    write(&r, "a", "---\nstatus: done\n---\n");
    save(
        &r.fx.ctx(),
        SESSION,
        &Record {
            version: "1.0.0".into(),
            reloaded: true,
        },
    )
    .expect("save");
    let context = prompt(&r, &r.coordinator).expect("context");
    let lines: Vec<&str> = context.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("crook: the plugin was reloaded"));
    assert!(lines[1].starts_with("Undelivered child result: "));
}

#[test]
fn an_unwritable_mark_is_warned_about_and_the_result_still_reported() {
    let r = rig();
    write(&r, "a", "---\nstatus: done\n---\n");
    // A folder where the marks file should be makes the write fail.
    std::fs::create_dir(r.flock.folder.join("results/.delivered.json")).expect("block");
    let mut bufs = Buffers::default();
    let found = scan(&r.flock, &mut bufs.io());
    assert_eq!(found.len(), 1);
    commit(&found.iter().collect::<Vec<_>>(), &mut bufs.io());
    assert!(
        bufs.err.text().contains("crook hook:"),
        "{}",
        bufs.err.text()
    );
}

#[test]
fn a_corrupt_marks_file_heals_after_one_rescan_with_one_warning() {
    let r = rig();
    write(&r, "a", "---\nstatus: done\n---\n");
    write(&r, "b", "---\nstatus: done\n---\n");
    std::fs::write(r.flock.folder.join("results/.delivered.json"), "garbage").expect("write");
    let ran = run_hook(
        &r.fx,
        &r.hc,
        "user-prompt-submit",
        Some(&r.coordinator),
        &hook_input(SESSION),
    );
    assert_eq!(
        ran.stderr.matches("crook hook:").count(),
        1,
        "{}",
        ran.stderr
    );
    assert_eq!(ran.context().expect("both reported").lines().count(), 2);
    assert_eq!(marks::load(&r.flock.folder).expect("valid again").len(), 2);
    let again = run_hook(
        &r.fx,
        &r.hc,
        "user-prompt-submit",
        Some(&r.coordinator),
        &hook_input(SESSION),
    );
    assert_eq!((again.stdout.as_str(), again.stderr.as_str()), ("", ""));
}
