use std::time::{Duration, SystemTime};

use super::*;
use crate::hc::fake::{Call, FakeHc};
use crate::testutil::{Buffers, Fixture, SESSION, make_flock, start_coordinator};

struct Rig {
    fx: Fixture,
    hc: FakeHc,
    who: Who,
}

fn rig() -> Rig {
    let fx = Fixture::new();
    fx.set_plugin_version("1.0.0");
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    let address = start_coordinator(&hc, &flock);
    let who = Who {
        address,
        flock: "demo".into(),
        child: None,
        parent: None,
    };
    Rig { fx, hc, who }
}

fn sends(hc: &FakeHc) -> Vec<String> {
    hc.calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Send { to, text } => Some(format!("{to}: {text}")),
            _ => None,
        })
        .collect()
}

fn reload(r: &Rig) -> Buffers {
    let mut bufs = Buffers::default();
    self_reload(&r.fx.ctx(), &r.hc, &mut bufs.io(), SESSION, &r.who);
    bufs
}

fn record(r: &Rig) -> Option<Record> {
    load(&r.fx.ctx(), SESSION).expect("loads")
}

#[test]
fn the_version_comes_from_the_plugin_root_else_the_current_link() {
    let r = rig();
    assert_eq!(current(&r.fx.ctx()).expect("version"), "1.0.0");
    r.fx.set_env("CLAUDE_PLUGIN_ROOT", "");
    assert!(current(&r.fx.ctx()).is_err(), "no current link yet");
    let plugin = r.fx.root().join("data/crook/current/plugin/.claude-plugin");
    std::fs::create_dir_all(&plugin).expect("mkdir");
    std::fs::write(plugin.join("plugin.json"), r#"{"version": "2.5.0"}"#).expect("write");
    assert_eq!(current(&r.fx.ctx()).expect("version"), "2.5.0");
}

#[test]
fn start_records_the_version_and_overwrites_an_old_record() {
    let r = rig();
    record_start(&r.fx.ctx(), SESSION, true).expect("records");
    assert_eq!(
        record(&r),
        Some(Record {
            version: "1.0.0".into(),
            reloaded: false
        })
    );
    r.fx.set_plugin_version("1.1.0");
    save(
        &r.fx.ctx(),
        SESSION,
        &Record {
            version: "0.9.0".into(),
            reloaded: true,
        },
    )
    .expect("save");
    record_start(&r.fx.ctx(), SESSION, true).expect("records");
    assert_eq!(
        record(&r),
        Some(Record {
            version: "1.1.0".into(),
            reloaded: false
        })
    );
}

#[test]
fn a_bad_session_id_is_refused_before_it_is_a_file_name() {
    let r = rig();
    assert!(record_start(&r.fx.ctx(), "../x", true).is_err());
    assert!(load(&r.fx.ctx(), "").is_err());
}

#[test]
fn the_same_version_sends_nothing() {
    let r = rig();
    record_start(&r.fx.ctx(), SESSION, true).expect("records");
    let bufs = reload(&r);
    assert_eq!(sends(&r.hc), Vec::<String>::new());
    assert_eq!(bufs.err.text(), "");
}

#[test]
fn a_new_version_sends_once_updates_the_record_and_sets_reloaded() {
    let r = rig();
    record_start(&r.fx.ctx(), SESSION, true).expect("records");
    r.fx.set_plugin_version("1.1.0");
    reload(&r);
    assert_eq!(
        sends(&r.hc),
        vec![format!("{}: /reload-plugins", r.who.address)]
    );
    assert_eq!(
        record(&r),
        Some(Record {
            version: "1.1.0".into(),
            reloaded: true
        })
    );
    // The next Stop sees the same version and sends nothing more.
    reload(&r);
    assert_eq!(sends(&r.hc).len(), 1);
}

#[test]
fn a_failed_send_keeps_the_old_record_so_the_next_stop_tries_again() {
    let r = rig();
    record_start(&r.fx.ctx(), SESSION, true).expect("records");
    r.fx.set_plugin_version("1.1.0");
    for outcome in [
        SendOutcome::Blocked,
        SendOutcome::Finished,
        SendOutcome::NotFound,
        SendOutcome::Failed("x".into()),
    ] {
        r.hc.force_send(outcome);
        let bufs = reload(&r);
        assert_eq!(
            record(&r),
            Some(Record {
                version: "1.0.0".into(),
                reloaded: false
            })
        );
        assert!(
            bufs.err.text().contains("will try again"),
            "{}",
            bufs.err.text()
        );
    }
    r.hc.fail("send", 1);
    let bufs = reload(&r);
    assert_eq!(record(&r).expect("record").version, "1.0.0");
    assert!(
        bufs.err.text().contains("could not reload"),
        "{}",
        bufs.err.text()
    );
    // Once hc answers, it goes through.
    r.hc.force_send(SendOutcome::Sent);
    reload(&r);
    assert_eq!(
        record(&r),
        Some(Record {
            version: "1.1.0".into(),
            reloaded: true
        })
    );
}

#[test]
fn a_missing_or_unreadable_record_is_written_and_sends_nothing() {
    let r = rig();
    reload(&r);
    assert_eq!(
        record(&r),
        Some(Record {
            version: "1.0.0".into(),
            reloaded: false
        })
    );
    std::fs::write(
        paths::session_record(r.fx.ctx().env, SESSION).expect("path"),
        "garbage",
    )
    .expect("write");
    r.fx.set_plugin_version("1.2.0");
    reload(&r);
    assert_eq!(
        record(&r),
        Some(Record {
            version: "1.2.0".into(),
            reloaded: false
        })
    );
    assert_eq!(sends(&r.hc), Vec::<String>::new());
}

#[test]
fn an_unreadable_plugin_version_changes_nothing() {
    let r = rig();
    record_start(&r.fx.ctx(), SESSION, true).expect("records");
    r.fx.set_env("CLAUDE_PLUGIN_ROOT", "/does/not/exist");
    let bufs = reload(&r);
    assert!(bufs.err.text().contains("version check failed"));
    assert_eq!(record(&r).expect("record").version, "1.0.0");
    assert_eq!(sends(&r.hc), Vec::<String>::new());
}

#[test]
fn the_nudge_is_taken_once() {
    let r = rig();
    assert!(
        !take_nudge(&r.fx.ctx(), SESSION).expect("none"),
        "no record"
    );
    save(
        &r.fx.ctx(),
        SESSION,
        &Record {
            version: "1.0.0".into(),
            reloaded: true,
        },
    )
    .expect("save");
    assert!(take_nudge(&r.fx.ctx(), SESSION).expect("first"));
    assert!(!take_nudge(&r.fx.ctx(), SESSION).expect("second"));
    assert_eq!(record(&r).expect("kept").version, "1.0.0");
}

#[test]
fn the_nudge_names_the_skill_of_the_role() {
    let mut who = Who {
        address: "t1".into(),
        flock: "demo".into(),
        child: None,
        parent: None,
    };
    assert!(nudge_line(&who).contains("`crook-coordinator`"));
    who.child = Some("a".into());
    assert!(nudge_line(&who).contains("`crook-child`"));
}

#[test]
fn records_older_than_thirty_days_are_pruned_and_newer_ones_kept() {
    let r = rig();
    // Nothing to prune before the folder exists.
    assert_eq!(prune(&r.fx.ctx()).expect("prunes"), 0);
    let dir = paths::sessions_dir(r.fx.ctx().env).expect("dir");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let now = r.fx.ctx().clock.now();
    let age = |name: &str, days: u64| {
        let path = dir.join(name);
        std::fs::write(&path, "{}").expect("write");
        let file = std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("open");
        file.set_modified(now - Duration::from_secs(days * 86_400))
            .expect("mtime");
    };
    age("old", 31);
    age("edge", 29);
    age("fresh", 0);
    let future: SystemTime = now + Duration::from_secs(3600);
    std::fs::write(dir.join("future"), "{}").expect("write");
    std::fs::File::options()
        .write(true)
        .open(dir.join("future"))
        .expect("open")
        .set_modified(future)
        .expect("mtime");
    assert_eq!(prune(&r.fx.ctx()).expect("prunes"), 1);
    let mut left: Vec<String> = std::fs::read_dir(&dir)
        .expect("dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["edge", "fresh", "future"]);
}

#[test]
fn the_new_record_is_down_before_the_send_so_a_cut_off_hook_cannot_loop() {
    use crate::testutil::HookedHc;
    let r = rig();
    record_start(&r.fx.ctx(), SESSION, true).expect("records");
    r.fx.set_plugin_version("1.1.0");
    let ctx = r.fx.ctx();
    let seen = std::cell::RefCell::new(None);
    let mut hooked = HookedHc::new(&r.hc);
    hooked.on_send = Some(Box::new(|_, text| {
        assert_eq!(text, "/reload-plugins");
        *seen.borrow_mut() = load(&ctx, SESSION).expect("loads");
    }));
    let mut bufs = Buffers::default();
    self_reload(&ctx, &hooked, &mut bufs.io(), SESSION, &r.who);
    assert_eq!(
        seen.borrow().clone(),
        Some(Record {
            version: "1.1.0".into(),
            reloaded: true
        }),
        "at the moment of the send"
    );
}

#[test]
fn clear_and_compact_keep_an_existing_record_and_create_a_missing_one() {
    let r = rig();
    // Missing: created, whatever the source.
    record_start(&r.fx.ctx(), SESSION, false).expect("creates");
    assert_eq!(
        record(&r),
        Some(Record {
            version: "1.0.0".into(),
            reloaded: false
        })
    );
    // Present: kept, even though the plugin on disk has moved on.
    r.fx.set_plugin_version("2.0.0");
    record_start(&r.fx.ctx(), SESSION, false).expect("keeps");
    assert_eq!(record(&r).expect("record").version, "1.0.0");
    record_start(&r.fx.ctx(), SESSION, true).expect("startup overwrites");
    assert_eq!(record(&r).expect("record").version, "2.0.0");
}
