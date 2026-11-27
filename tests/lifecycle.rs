//! The lifecycle commands (`new`, `open`, `close`, `spawn`, `stop`, `resume`) as the binary runs
//! them: a fake `hc` and a fake `herdr-app` on a cleared `PATH`, real `git` in temp repositories,
//! and temp XDG folders. No real hc core, claude or herdr-app is ever started.
#![allow(clippy::expect_used)]
#![cfg(unix)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hc/0.1.22")
}

fn script(path: &Path, text: &str) {
    std::fs::write(path, text).expect("write");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

/// The first of these that exists, so the wrapper runs the real tool by absolute path and `PATH`
/// can hold nothing else.
fn real(tool: &str) -> Option<PathBuf> {
    ["/usr/bin", "/bin", "/opt/homebrew/bin", "/usr/local/bin"]
        .iter()
        .map(|d| Path::new(d).join(tool))
        .find(|p| p.exists())
}

/// A fake `hc` that answers from the captured fixtures, keeps a counter for new addresses, and logs
/// every call as `--- <subcommand>` followed by one argument per line. `$STATE/ls-<flock>.json` (what `ls --label crook.flock=<flock>` lists) and
/// `$STATE/get-<id>` stand in for what the core knows, and `$STATE/run-fails` makes `run` fail.
const FAKE_HC: &str = r#"#!/bin/sh
fx='@FIXTURES@'
{ echo "--- $1"; for a in "$@"; do printf '%s\n' "$a"; done; } >> "$FAKE_HC_STATE/log"
case "$1" in
  --version) /bin/cat "$fx/version.stdout" ;;
  server) /bin/cat "$fx/server_start_warm.stdout" ;;
  schema) /bin/cat "$fx/schema.stdout" ;;
  ls)
    file="$FAKE_HC_STATE/ls.json"
    for a in "$@"; do
      case "$a" in crook.flock=*) file="$FAKE_HC_STATE/ls-${a#crook.flock=}.json" ;; esac
    done
    if [ -e "$file" ]; then /bin/cat "$file"; else /bin/cat "$fx/ls_empty.stdout"; fi ;;
  get)
    id=""
    for a in "$@"; do id="$a"; done
    if [ -e "$FAKE_HC_STATE/get-$id" ]; then /bin/cat "$FAKE_HC_STATE/get-$id"; else /bin/cat "$fx/get_missing.stderr" >&2; exit 3; fi ;;
  run)
    if [ -e "$FAKE_HC_STATE/run-fails" ]; then /bin/cat "$fx/run_name_taken.stderr" >&2; exit 1; fi
    n=$(/bin/cat "$FAKE_HC_STATE/counter" 2>/dev/null || echo 0)
    n=$((n + 1))
    echo "$n" > "$FAKE_HC_STATE/counter"
    name=""
    prev=""
    for a in "$@"; do
      if [ "$prev" = "--name" ]; then name="$a"; fi
      prev="$a"
    done
    printf '{"address":"t%s","agent":"claude","cwd":"/x","name":"%s","state":"live","turn":0}\n' "$n" "$name" ;;
  kill) /bin/cat "$fx/kill.stdout" ;;
  *) exit 2 ;;
esac
"#;

struct Sandbox {
    dir: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Sandbox {
        let dir = tempfile::tempdir().expect("tempdir");
        let s = Sandbox { dir };
        for d in ["bin", "state", "home", "data", "config", "xdg-state"] {
            std::fs::create_dir_all(s.root().join(d)).expect("mkdir");
        }
        script(
            &s.bin().join("hc"),
            &FAKE_HC.replace("@FIXTURES@", &fixtures().display().to_string()),
        );
        script(
            &s.bin().join("herdr-app"),
            &format!(
                "#!/bin/sh\necho \"$@\" >> '{}'\necho '{{\"ok\": true}}'\n",
                s.state().join("herdr-log").display()
            ),
        );
        for tool in ["git", "date"] {
            let path = real(tool).expect("tool exists");
            script(
                &s.bin().join(tool),
                &format!("#!/bin/sh\nexec '{}' \"$@\"\n", path.display()),
            );
        }
        s
    }

    fn root(&self) -> PathBuf {
        self.dir.path().canonicalize().expect("canonical")
    }

    fn bin(&self) -> PathBuf {
        self.root().join("bin")
    }

    fn state(&self) -> PathBuf {
        self.root().join("state")
    }

    fn flocks(&self) -> PathBuf {
        self.root().join("data/crook/flocks")
    }

    fn crook(&self, args: &[&str], terminal: Option<&str>) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_crook"));
        cmd.args(args)
            .env_clear()
            .env("PATH", self.bin())
            .env("HOME", self.root().join("home"))
            .env("XDG_DATA_HOME", self.root().join("data"))
            .env("XDG_CONFIG_HOME", self.root().join("config"))
            .env("XDG_STATE_HOME", self.root().join("xdg-state"))
            .env("FAKE_HC_STATE", self.state());
        if let Some(id) = terminal {
            cmd.env("HC_TERMINAL_ID", id);
        }
        cmd.output().expect("crook runs")
    }

    /// The one flock folder, whatever today's date made of its name.
    fn flock_folder(&self) -> PathBuf {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(self.flocks())
            .expect("flocks root")
            .map(|e| e.expect("entry").path())
            .collect();
        assert_eq!(entries.len(), 1, "{entries:?}");
        entries.remove(0)
    }

    fn roster(&self) -> Value {
        let text =
            std::fs::read_to_string(self.flock_folder().join("roster.json")).expect("roster");
        serde_json::from_str(&text).expect("json")
    }

    /// The hc calls so far, each as its list of arguments after the subcommand marker.
    fn hc_calls(&self) -> Vec<Vec<String>> {
        let text = std::fs::read_to_string(self.state().join("log")).unwrap_or_default();
        let mut calls: Vec<Vec<String>> = Vec::new();
        for line in text.lines() {
            if line.starts_with("--- ") {
                calls.push(Vec::new());
            } else if let Some(call) = calls.last_mut() {
                call.push(line.to_string());
            }
        }
        calls
    }

    fn hc_runs(&self) -> Vec<Vec<String>> {
        self.hc_calls()
            .into_iter()
            .filter(|c| c.first().map(String::as_str) == Some("run"))
            .collect()
    }

    /// Tells the fake core that `id` is a live terminal with these labels.
    fn know_terminal(&self, id: &str, name: &str, labels: &Value) {
        let record = json!({
            "address": id, "name": name, "state": "live", "labels": labels,
            "agent": "claude", "cwd": "/x", "parent": null, "status": null, "turn": 0
        });
        std::fs::write(self.state().join(format!("get-{id}")), record.to_string()).expect("write");
    }

    fn set_live(&self, terminals: &Value) {
        std::fs::write(
            self.state().join("ls-demo.json"),
            json!({"hidden_finished": 0, "terminals": terminals}).to_string(),
        )
        .expect("write");
    }

    fn new_flock(&self) -> PathBuf {
        let out = self.crook(&["new", "demo"], None);
        assert!(out.status.success(), "{}", text(&out.stderr));
        self.flock_folder()
    }

    fn write_brief(&self, name: &str) -> String {
        let path = self
            .flock_folder()
            .join("briefs")
            .join(format!("{name}.md"));
        std::fs::write(&path, "# Goal\n\nDo it.\n").expect("brief");
        path.display().to_string()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn flag_value(argv: &[String], flag: &str) -> String {
    let at = argv.iter().position(|a| a == flag).expect(flag);
    argv[at + 1].clone()
}

fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git");
    assert!(out.status.success(), "git {args:?}: {}", text(&out.stderr));
}

fn repo(sandbox: &Sandbox) -> PathBuf {
    let root = sandbox.root().join("repo");
    std::fs::create_dir_all(&root).expect("mkdir");
    git(&root, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "Crook Test"),
        ("user.email", "crook@example.invalid"),
    ] {
        git(&root, &["config", k, v]);
    }
    std::fs::write(root.join("a.txt"), "one\n").expect("write");
    git(&root, &["add", "a.txt"]);
    git(&root, &["commit", "-q", "-m", "one"]);
    root
}

#[test]
fn new_creates_the_flock_and_launches_the_coordinator() {
    let s = Sandbox::new();
    let out = s.crook(&["new", "demo", "--task", "ship it"], None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "opened flock demo (terminal t1)\n");
    assert_eq!(text(&out.stderr), "");

    let folder = s.flock_folder();
    let name = folder
        .file_name()
        .and_then(|n| n.to_str())
        .expect("name")
        .to_string();
    assert!(name.ends_with("-demo"), "{name}");
    let date = &name[..10];
    assert!(
        date.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 {
            b == b'-'
        } else {
            b.is_ascii_digit()
        }),
        "{name}"
    );
    common::assert_roster_file_valid(&folder);
    let roster = s.roster();
    assert_eq!(roster["coordinator"]["started"], json!(true));
    assert_eq!(roster["children"], json!([]));

    let runs = s.hc_runs();
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    assert_eq!(flag_value(run, "--name"), "demo");
    assert!(run.contains(&"--no-parent".to_string()));
    assert!(run.contains(&"crook.flock=demo".to_string()));
    assert!(run.contains(&"herdr.space=crook-demo".to_string()));
    assert_eq!(
        flag_value(run, "--session-id"),
        roster["coordinator"]["session_id"].as_str().expect("sid")
    );
    assert!(run.last().expect("prompt").ends_with("ship it"));

    let herdr = std::fs::read_to_string(s.state().join("herdr-log")).expect("herdr log");
    assert_eq!(herdr.lines().count(), 2);
    assert!(
        herdr.contains("do space.rename {\"id\":\"crook-demo\",\"name\":\"demo\"}"),
        "{herdr}"
    );
    assert!(herdr.contains("do tab.rename {\"id\":\"crook-demo-coord\",\"name\":\"coordinator\"}"));
}

#[test]
fn new_with_a_used_slug_is_refused_and_writes_nothing() {
    let s = Sandbox::new();
    s.new_flock();
    let before = s.hc_calls().len();
    let out = s.crook(&["new", "demo"], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("already used"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(s.hc_calls().len(), before);
    assert_eq!(std::fs::read_dir(s.flocks()).expect("dir").count(), 1);
}

#[test]
fn open_resumes_a_known_flock_and_leaves_a_live_coordinator_alone() {
    let s = Sandbox::new();
    s.new_flock();
    // The core says the coordinator is gone, so open starts it again, with --resume.
    let out = s.crook(&["open", "demo"], None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let runs = s.hc_runs();
    assert_eq!(runs.len(), 2);
    assert!(runs[1].contains(&"--resume".to_string()));
    assert!(!runs[1].contains(&"--session-id".to_string()));

    // Now it is live: nothing more starts.
    s.set_live(
        &json!([{"address": "t1", "name": "demo", "state": "live", "status": null,
        "agent": "claude", "cwd": "/x", "parent": null, "turn": 0}]),
    );
    let out = s.crook(&["open", "demo"], None);
    assert!(out.status.success());
    assert!(
        text(&out.stdout).contains("already open"),
        "{}",
        text(&out.stdout)
    );
    assert_eq!(s.hc_runs().len(), 2);
}

#[test]
fn open_of_an_unknown_slug_lists_the_known_ones() {
    let s = Sandbox::new();
    s.new_flock();
    let out = s.crook(&["open", "nope"], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("demo"), "{}", text(&out.stderr));
}

#[test]
fn a_missing_herdr_app_is_only_a_warning() {
    let s = Sandbox::new();
    std::fs::remove_file(s.bin().join("herdr-app")).expect("remove");
    let out = s.crook(&["new", "demo"], None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).starts_with("warning: "),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(text(&out.stdout), "opened flock demo (terminal t1)\n");
}

#[test]
fn a_missing_hc_fails_the_new_but_keeps_the_flock_for_a_retry() {
    let s = Sandbox::new();
    std::fs::remove_file(s.bin().join("hc")).expect("remove");
    let out = s.crook(&["new", "demo"], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("hc"), "{}", text(&out.stderr));
    assert!(text(&out.stderr).contains("crook open demo"));
    assert!(s.flock_folder().join("roster.json").is_file());
}

#[test]
fn spawn_writes_the_roster_makes_the_worktree_and_launches_under_the_coordinator() {
    let s = Sandbox::new();
    s.new_flock();
    s.know_terminal("t1", "demo", &json!({"crook.flock": "demo"}));
    let brief = s.write_brief("auth");
    let repo = repo(&s);
    let out = s.crook(
        &[
            "spawn",
            "child",
            "auth",
            &brief,
            "--repo",
            repo.to_str().expect("utf8"),
            "--model",
            "haiku",
        ],
        Some("t1"),
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).starts_with("spawned demo.auth (terminal t2, session "));

    common::assert_roster_file_valid(&s.flock_folder());
    let roster = s.roster();
    let child = &roster["children"][0];
    assert_eq!(child["model"], "haiku");
    assert_eq!(child["branch"], "crook/demo/auth");
    assert_eq!(child["brief"], "briefs/auth.md");
    assert!(repo.join(".worktrees/demo-auth").is_dir());

    let run = &s.hc_runs()[1];
    assert_eq!(flag_value(run, "--parent"), "t1");
    assert_eq!(flag_value(run, "--name"), "demo.auth");
    assert_eq!(flag_value(run, "--agent"), "crook:child");
    assert_eq!(flag_value(run, "--model"), "haiku");
    assert_eq!(
        flag_value(run, "--session-id"),
        child["session_id"].as_str().expect("sid")
    );
    assert!(run.last().expect("prompt").contains("Write your result to"));
}

#[test]
fn spawn_outside_a_crook_terminal_needs_flock() {
    let s = Sandbox::new();
    s.new_flock();
    let brief = s.write_brief("auth");
    let calls = s.hc_calls().len();
    let out = s.crook(&["spawn", "child", "auth", &brief], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("--flock"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(s.hc_calls().len(), calls, "refused before any hc call");
}

#[test]
fn two_spawns_at_once_both_land_in_the_roster() {
    let s = Sandbox::new();
    s.new_flock();
    s.know_terminal("t1", "demo", &json!({"crook.flock": "demo"}));
    let a = s.write_brief("one");
    let b = s.write_brief("two");
    std::thread::scope(|scope| {
        let first = scope.spawn(|| s.crook(&["spawn", "child", "one", &a], Some("t1")));
        let second = scope.spawn(|| s.crook(&["spawn", "child", "two", &b], Some("t1")));
        for out in [
            first.join().expect("thread"),
            second.join().expect("thread"),
        ] {
            assert!(out.status.success(), "{}", text(&out.stderr));
        }
    });
    common::assert_roster_file_valid(&s.flock_folder());
    let mut names: Vec<String> = s.roster()["children"]
        .as_array()
        .expect("children")
        .iter()
        .map(|c| c["name"].as_str().expect("name").to_string())
        .collect();
    names.sort();
    assert_eq!(names, ["one", "two"]);
}

#[test]
fn a_failed_launch_leaves_no_entry_and_the_name_works_again() {
    let s = Sandbox::new();
    s.new_flock();
    s.know_terminal("t1", "demo", &json!({"crook.flock": "demo"}));
    let brief = s.write_brief("auth");
    std::fs::write(s.state().join("run-fails"), "").expect("flag");
    let out = s.crook(&["spawn", "child", "auth", &brief], Some("t1"));
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(s.roster()["children"], json!([]));
    std::fs::remove_file(s.state().join("run-fails")).expect("flag");
    let out = s.crook(&["spawn", "child", "auth", &brief], Some("t1"));
    assert!(out.status.success(), "{}", text(&out.stderr));
}

#[test]
fn stop_marks_done_and_removes_the_clean_worktree() {
    let s = Sandbox::new();
    s.new_flock();
    s.know_terminal("t1", "demo", &json!({"crook.flock": "demo"}));
    let brief = s.write_brief("auth");
    let repo = repo(&s);
    let out = s.crook(
        &[
            "spawn",
            "child",
            "auth",
            &brief,
            "--repo",
            repo.to_str().expect("utf8"),
        ],
        Some("t1"),
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    s.set_live(
        &json!([{"address": "t2", "name": "demo.auth", "state": "live", "status": "idle",
        "agent": "claude", "cwd": "/x", "parent": "t1", "turn": 0}]),
    );

    let out = s.crook(&["stop", "auth"], Some("t1"));
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "stopped demo.auth\n");
    assert_eq!(s.roster()["children"][0]["state"], "done");
    common::assert_roster_file_valid(&s.flock_folder());
    assert!(!repo.join(".worktrees/demo-auth").exists());
    let kills: Vec<_> = s
        .hc_calls()
        .into_iter()
        .filter(|c| c.first().map(String::as_str) == Some("kill"))
        .collect();
    assert_eq!(kills.len(), 1);
    assert_eq!(kills[0].last().map(String::as_str), Some("t2"));
}

#[test]
fn resume_starts_the_lost_children_with_resume() {
    let s = Sandbox::new();
    s.new_flock();
    s.know_terminal("t1", "demo", &json!({"crook.flock": "demo"}));
    let brief = s.write_brief("auth");
    let out = s.crook(&["spawn", "child", "auth", &brief], Some("t1"));
    assert!(out.status.success(), "{}", text(&out.stderr));
    // Only the coordinator is live, so the child is lost.
    s.set_live(
        &json!([{"address": "t1", "name": "demo", "state": "live", "status": null,
        "agent": "claude", "cwd": "/x", "parent": null, "turn": 0}]),
    );

    let out = s.crook(&["resume"], Some("t1"));
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        text(&out.stdout).starts_with("resumed demo.auth"),
        "{}",
        text(&out.stdout)
    );
    let run = s.hc_runs().pop().expect("run");
    assert_eq!(
        flag_value(&run, "--resume"),
        s.roster()["children"][0]["session_id"]
            .as_str()
            .expect("sid")
    );
    assert_eq!(flag_value(&run, "--parent"), "t1");
}

#[test]
fn close_kills_the_child_then_the_coordinator_and_later_closes_only_retry_the_worktree() {
    let s = Sandbox::new();
    s.new_flock();
    s.know_terminal("t1", "demo", &json!({"crook.flock": "demo"}));
    let brief = s.write_brief("auth");
    let repo = repo(&s);
    let out = s.crook(
        &[
            "spawn",
            "child",
            "auth",
            &brief,
            "--repo",
            repo.to_str().expect("utf8"),
        ],
        Some("t1"),
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    s.set_live(&json!([
        {"address": "t1", "name": "demo", "state": "live", "status": null, "agent": "claude", "cwd": "/x", "parent": null, "turn": 0},
        {"address": "t2", "name": "demo.auth", "state": "live", "status": "idle", "agent": "claude", "cwd": "/x", "parent": "t1", "turn": 0}
    ]));
    std::fs::write(repo.join(".worktrees/demo-auth/scratch.txt"), "wip").expect("write");

    let out = s.crook(&["close", "demo"], Some("t1"));
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "closed flock demo\n");
    assert!(
        text(&out.stderr).starts_with("warning: kept worktree "),
        "{}",
        text(&out.stderr)
    );
    let roster = s.roster();
    assert_eq!(roster["coordinator"]["state"], "done");
    assert_eq!(roster["children"][0]["state"], "done");
    common::assert_roster_file_valid(&s.flock_folder());

    let kills: Vec<String> = s
        .hc_calls()
        .into_iter()
        .filter(|c| c.first().map(String::as_str) == Some("kill"))
        .filter_map(|c| c.last().cloned())
        .collect();
    assert_eq!(kills, ["t2", "t1"], "the child first, the coordinator last");

    s.set_live(&json!([]));
    // The worktree is still dirty: the same warning, nothing else, exit 0.
    let out = s.crook(&["close", "demo"], Some("t1"));
    assert!(out.status.success());
    assert_eq!(text(&out.stdout), "");
    assert!(text(&out.stderr).contains("untracked files"));
    // Clean now: removed, and silence.
    std::fs::remove_file(repo.join(".worktrees/demo-auth/scratch.txt")).expect("clean");
    let out = s.crook(&["close", "demo"], Some("t1"));
    assert!(out.status.success());
    assert_eq!(
        (text(&out.stdout), text(&out.stderr)),
        (String::new(), String::new())
    );
    assert!(!repo.join(".worktrees/demo-auth").exists());
}
