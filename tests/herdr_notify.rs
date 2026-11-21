//! `need` and `done` raise a herdr notification after their write commits, and
//! a failed notification never fails the verb.

use std::io::Cursor;
use std::os::unix::fs::PermissionsExt;

use crook::crew::{self, Child};
use crook::modverb;
use crook::store::{Mode, Store, Touch};
use serde_json::{Value, json};

struct Fx {
    tmp: tempfile::TempDir,
}

impl Fx {
    /// Project `p` with one child `c1` (session `s1`), and a `herdr` on `PATH`
    /// that logs its argv and exits `herdr_exit`.
    fn new(herdr_exit: i32) -> Fx {
        let fx = Fx {
            tmp: tempfile::tempdir().unwrap(),
        };
        let env = fx.env(false);
        crook::project::init(&env, "p").unwrap();
        Store::open(&env, "p", Mode::ReadWrite)
            .unwrap()
            .write(Touch::CHILDREN, |tx| {
                crew::add_child(
                    tx,
                    &Child {
                        child: "c1".into(),
                        repo: "/r".into(),
                        worktree: "/r/.worktrees/c1".into(),
                        branch: "b".into(),
                        brief: "x".into(),
                        session_id: "s1".into(),
                    },
                )
            })
            .unwrap();
        let bin = fx.tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let herdr = bin.join("herdr");
        std::fs::write(
            &herdr,
            format!(
                "#!/bin/sh\necho \"$@\" >> {}\necho '{{\"noise\":true}}'\necho 'herdr says no' >&2\nexit {herdr_exit}\n",
                fx.log().display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&herdr, std::fs::Permissions::from_mode(0o755)).unwrap();
        fx
    }

    fn log(&self) -> std::path::PathBuf {
        self.tmp.path().join("herdr.log")
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.log())
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn env(&self, in_herdr: bool) -> impl Fn(&str) -> Option<String> + use<> {
        let state = self.tmp.path().display().to_string();
        let bin = self.tmp.path().join("bin").display().to_string();
        move |k: &str| match k {
            "XDG_STATE_HOME" => Some(state.clone()),
            "PATH" => Some(bin.clone()),
            "HERDR_ENV" if in_herdr => Some("1".to_string()),
            "CROOK_ROLE" => Some("child".to_string()),
            "CROOK_PROJECT" => Some("p".to_string()),
            "CROOK_CHILD" => Some("c1".to_string()),
            _ => None,
        }
    }

    fn call(&self, in_herdr: bool, verb: &str, rest: Value) -> (i32, String, Value) {
        let env = self.env(in_herdr);
        let mut body = json!({"session_id": "s1", "user_turn": false});
        if let (Some(b), Some(r)) = (body.as_object_mut(), rest.as_object()) {
            b.extend(r.clone());
        }
        let mut out = Vec::new();
        let code = modverb::main(
            &[verb.to_string()],
            &env,
            &mut Cursor::new(body.to_string().into_bytes()),
            &mut out,
        );
        let text = String::from_utf8(out).unwrap();
        let v = serde_json::from_str(text.trim()).expect("stdout is one JSON object");
        (code, text, v)
    }

    fn start_task(&self) {
        let env = self.env(false);
        Store::open(&env, "p", Mode::ReadWrite)
            .unwrap()
            .write(Touch::INBOX | Touch::STATUS, |tx| {
                crew::task(tx, "c1", "t")?;
                crew::take_task(tx, "c1")?;
                Ok(())
            })
            .unwrap();
    }
}

#[test]
fn need_and_done_notify_with_the_child_as_the_body() {
    let fx = Fx::new(0);
    let (code, text, v) = fx.call(true, "need", json!({"question": "which one?"}));
    assert_eq!((code, v["ok"].as_bool()), (0, Some(true)), "{text}");
    assert_eq!(
        fx.calls(),
        ["notification show crook: p needs you --body c1 --sound request"]
    );
    // herdr's own output never reaches the verb's stdout.
    assert!(!text.contains("noise"), "{text}");

    let env = fx.env(false);
    Store::open(&env, "p", Mode::ReadWrite)
        .unwrap()
        .write(Touch::STATUS, |tx| crew::lift(tx, "c1").map(drop))
        .unwrap();
    let (code, text, _) = fx.call(true, "done", json!({"outcome": "ok"}));
    assert_eq!(code, 0, "{text}");
    assert_eq!(
        fx.calls().last().map(String::as_str),
        Some("notification show crook: p is done --body c1 --sound request")
    );
}

#[test]
fn a_failed_notification_does_not_fail_the_verb() {
    let fx = Fx::new(1);
    let (code, text, v) = fx.call(true, "need", json!({"question": "which one?"}));
    assert_eq!((code, v["ok"].as_bool()), (0, Some(true)), "{text}");
    assert_eq!(fx.calls().len(), 1, "herdr was asked once");
    // The write stands.
    let env = fx.env(false);
    let state = Store::open(&env, "p", Mode::ReadOnly)
        .unwrap()
        .read(|c| {
            Ok(
                c.query_row("SELECT state FROM status WHERE child = 'c1'", [], |r| {
                    r.get::<_, String>(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(state, "parked");
}

#[test]
fn no_notification_outside_herdr_or_for_a_refused_verb() {
    let fx = Fx::new(0);
    let (code, _, _) = fx.call(false, "need", json!({"question": "q"}));
    assert_eq!(code, 0);
    assert!(fx.calls().is_empty(), "{:?}", fx.calls());

    // A refused `done` (bad outcome) wrote nothing, so it says nothing.
    let fx = Fx::new(0);
    fx.start_task();
    let (code, _, _) = fx.call(true, "done", json!({"outcome": "great"}));
    assert_eq!(code, 1);
    assert!(fx.calls().is_empty(), "{:?}", fx.calls());
}
