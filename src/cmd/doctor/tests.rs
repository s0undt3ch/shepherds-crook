use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use super::*;
use crate::files::roster::{self, Child, State};
use crate::hc::fake::FakeHc;
use crate::testutil::{Buffers, Fixture, fake_install, init_repo, make_flock};

fn executable(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, text).expect("write");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

/// Everything in place: hc answers, `herdr-app` on `PATH`, a login shell that finds `crook`, the
/// mise shim, the mod installed at the install's version, the link pointing here, no config.
struct Rig {
    fx: Fixture,
    hc: FakeHc,
    install: Install,
    app_home: PathBuf,
}

fn healthy() -> Rig {
    let fx = Fixture::new();
    let app_home = fx.root().join("app");
    fx.set_env("HERDR_APP_HOME", &app_home.display().to_string());
    let bin = fx.root().join("pathbin");
    executable(&bin.join("herdr-app"), "#!/bin/sh\n");
    fx.set_env("PATH", &bin.display().to_string());
    let shell = fx.root().join("fakeshell");
    executable(
        &shell,
        "#!/bin/sh\necho 'noise'\necho /somewhere/shims/crook\n",
    );
    fx.set_env("SHELL", &shell.display().to_string());
    executable(&fx.root().join("data/mise/shims/crook"), "#!/bin/sh\n");
    let install = fake_install(&fx, "0.2.0");
    crate::cmd::setup::install_mod(&fx.ctx(), &install, "0.2.0").expect("mod");
    install::point_current(fx.ctx().env, &install, false).expect("link");
    Rig {
        fx,
        hc: FakeHc::new(),
        install,
        app_home,
    }
}

impl Rig {
    fn checks(&self) -> Vec<Check> {
        collect(&self.fx.ctx(), &self.hc, Ok(&self.install), false)
    }

    fn check(&self, id: &str) -> Check {
        self.checks()
            .into_iter()
            .find(|c| c.id == id)
            .unwrap_or_else(|| panic!("no check {id}"))
    }

    fn doctor(&self, fix: bool, json: bool) -> (Result<(), Error>, Buffers) {
        let mut bufs = Buffers::default();
        let out = perform(
            &self.fx.ctx(),
            &self.hc,
            &mut bufs.io(),
            &self.install,
            false,
            fix,
            json,
        );
        (out, bufs)
    }
}

/// Every file under `dir` with its text.
fn files_under(dir: &Path) -> std::collections::BTreeMap<String, String> {
    fn walk(base: &Path, dir: &Path, out: &mut std::collections::BTreeMap<String, String>) {
        for entry in std::fs::read_dir(dir).expect("read_dir").flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(base)
                        .expect("under")
                        .display()
                        .to_string(),
                    std::fs::read_to_string(&path).expect("read"),
                );
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn add_repo_child(rig: &Rig, slug: &str, repo: &Path, state: State) {
    let flock = make_flock(&rig.fx, slug);
    roster::add_child(
        &flock.folder,
        Child {
            name: "auth".into(),
            role: "child".into(),
            model: "sonnet".into(),
            session_id: "00000000-0000-4000-8000-000000000001".into(),
            folder: repo.join(".worktrees").join(format!("{slug}-auth")),
            repo: Some(repo.to_path_buf()),
            branch: Some(format!("crook/{slug}/auth")),
            brief: "briefs/auth.md".into(),
            state,
            started: true,
        },
    )
    .expect("child");
}

#[test]
fn a_healthy_install_passes_every_check() {
    let rig = healthy();
    let checks = rig.checks();
    let ids: Vec<&str> = checks.iter().map(|c| c.id).collect();
    assert_eq!(
        ids,
        [
            "hc",
            "herdr-app",
            "login-shell",
            "shim",
            "mod",
            "current-link",
            "config",
            "worktrees-ignored"
        ]
    );
    for c in &checks {
        assert_eq!(c.status, Status::Ok, "{}: {}", c.id, c.detail);
        assert!(c.fix.is_none(), "{}", c.id);
    }
    let (out, bufs) = rig.doctor(false, false);
    out.expect("exit 0");
    assert!(bufs.out.text().contains("ok"), "{}", bufs.out.text());
}

#[test]
fn hc_fails_when_too_old_without_labels_or_unreachable() {
    let mut rig = healthy();
    rig.hc = FakeHc::new().with_version("0.1.21");
    let c = rig.check("hc");
    assert_eq!(c.status, Status::Fail);
    assert!(c.detail.contains("too old"), "{}", c.detail);

    rig.hc = FakeHc::new().without_run_labels();
    assert_eq!(rig.check("hc").status, Status::Fail);

    rig.hc = FakeHc::new();
    rig.hc.fail("version", 5);
    assert_eq!(rig.check("hc").status, Status::Fail);
}

#[test]
fn herdr_app_must_be_an_executable_on_path() {
    let rig = healthy();
    rig.fx.set_env("PATH", "/nonexistent");
    assert_eq!(rig.check("herdr-app").status, Status::Fail);
    // A file that is not executable does not count.
    let dir = rig.fx.root().join("plain");
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join("herdr-app"), "").expect("write");
    rig.fx
        .set_env("PATH", &format!("/nonexistent:{}", dir.display()));
    assert_eq!(rig.check("herdr-app").status, Status::Fail);
}

#[test]
fn the_login_shell_is_asked_for_crook() {
    let rig = healthy();
    let c = rig.check("login-shell");
    assert!(c.detail.contains("/somewhere/shims/crook"), "{}", c.detail);

    executable(&rig.fx.root().join("fakeshell"), "#!/bin/sh\nexit 1\n");
    let c = rig.check("login-shell");
    assert_eq!(c.status, Status::Warn);
    assert!(c.fix.is_some());

    rig.fx.set_env("SHELL", "");
    assert_eq!(rig.check("login-shell").status, Status::Warn);
}

#[test]
fn a_missing_shim_warns_and_names_crook_bin() {
    let rig = healthy();
    std::fs::remove_file(rig.fx.root().join("data/mise/shims/crook")).expect("rm");
    let c = rig.check("shim");
    assert_eq!(c.status, Status::Warn);
    assert!(c.fix.as_deref().is_some_and(|f| f.contains("CROOK_BIN")));

    // CROOK_BIN pointing at a real file stands in for the shim.
    let bin = rig.fx.root().join("mycrook");
    executable(&bin, "#!/bin/sh\n");
    rig.fx.set_env("CROOK_BIN", &bin.display().to_string());
    assert_eq!(rig.check("shim").status, Status::Ok);
    rig.fx.set_env("CROOK_BIN", "relative/crook");
    assert_eq!(rig.check("shim").status, Status::Warn);
}

#[test]
fn the_mod_check_follows_the_marker() {
    let rig = healthy();
    let dest = rig.app_home.join("mods/crook");
    let marker = dest.join(".crook-version");

    std::fs::write(&marker, "0.1.0\n").expect("write");
    let c = rig.check("mod");
    assert_eq!(c.status, Status::Fail);
    assert!(c.fix.as_deref().is_some_and(|f| f.contains("crook setup")));

    std::fs::write(&marker, "0.3.0\n").expect("write");
    assert_eq!(rig.check("mod").status, Status::Fail);

    std::fs::remove_file(&marker).expect("rm");
    let c = rig.check("mod");
    assert_eq!(c.status, Status::Fail);
    assert!(c.detail.contains("someone else"), "{}", c.detail);

    std::fs::remove_dir_all(&dest).expect("rm");
    let c = rig.check("mod");
    assert_eq!(c.status, Status::Fail);
    assert!(c.detail.contains("not installed"), "{}", c.detail);
}

#[test]
fn the_link_check_tells_missing_dangling_older_and_elsewhere_apart() {
    let rig = healthy();
    let link = rig.fx.root().join("data/crook/current");

    // Pointing at a newer install is fine: the link only moves forward.
    let newer = rig.fx.root().join("newer");
    std::fs::create_dir_all(newer.join("plugin/.claude-plugin")).expect("mkdir");
    std::fs::write(
        newer.join("plugin/.claude-plugin/plugin.json"),
        r#"{"version": "0.9.0"}"#,
    )
    .expect("write");
    std::fs::remove_file(&link).expect("rm");
    std::os::unix::fs::symlink(&newer, &link).expect("link");
    let c = rig.check("current-link");
    assert_eq!(c.status, Status::Warn, "{}", c.detail);

    // An older install is what `setup` moves, so it fails.
    let older = rig.fx.root().join("older");
    std::fs::create_dir_all(older.join("plugin/.claude-plugin")).expect("mkdir");
    std::fs::write(
        older.join("plugin/.claude-plugin/plugin.json"),
        r#"{"version": "0.1.0"}"#,
    )
    .expect("write");
    std::fs::remove_file(&link).expect("rm");
    std::os::unix::fs::symlink(&older, &link).expect("link");
    let c = rig.check("current-link");
    assert_eq!(c.status, Status::Fail);
    assert!(c.fix.is_some());

    std::fs::remove_file(&link).expect("rm");
    std::os::unix::fs::symlink(rig.fx.root().join("gone"), &link).expect("link");
    assert_eq!(rig.check("current-link").status, Status::Fail);

    std::fs::remove_file(&link).expect("rm");
    assert_eq!(rig.check("current-link").status, Status::Fail);

    std::fs::write(&link, "a plain file").expect("write");
    assert_eq!(rig.check("current-link").status, Status::Fail);
}

#[test]
fn a_debug_build_does_not_manage_the_link() {
    let rig = healthy();
    std::fs::remove_file(rig.fx.root().join("data/crook/current")).expect("rm");
    let checks = collect(&rig.fx.ctx(), &rig.hc, Ok(&rig.install), true);
    let c = checks
        .iter()
        .find(|c| c.id == "current-link")
        .expect("check");
    assert_eq!(c.status, Status::Ok);
}

#[test]
fn a_bad_config_fails_the_config_check() {
    let rig = healthy();
    rig.fx.write_config("nonsense = 1\n");
    let c = rig.check("config");
    assert_eq!(c.status, Status::Fail);
    assert!(c.detail.contains("config.toml"), "{}", c.detail);
}

#[test]
fn a_repository_of_an_active_flock_must_ignore_dot_worktrees() {
    let rig = healthy();
    let repo = init_repo(&rig.fx.root());
    add_repo_child(&rig, "demo", &repo, State::Active);
    let c = rig.check("worktrees-ignored");
    assert_eq!(c.status, Status::Warn);
    assert!(
        c.detail.contains(&repo.display().to_string()),
        "{}",
        c.detail
    );
    assert!(c.fix.as_deref().is_some_and(|f| f.contains(".worktrees/")));

    std::fs::write(repo.join(".gitignore"), ".worktrees/\n").expect("write");
    assert_eq!(rig.check("worktrees-ignored").status, Status::Ok);
}

#[test]
fn a_closed_flocks_repository_is_not_checked() {
    let rig = healthy();
    let repo = init_repo(&rig.fx.root());
    add_repo_child(&rig, "demo", &repo, State::Active);
    let flock = crate::flock::find(rig.fx.ctx().env, "demo").expect("flock");
    roster::mark_all_done(&flock.folder).expect("close");
    assert_eq!(rig.check("worktrees-ignored").status, Status::Ok);
}

#[test]
fn a_repository_that_is_gone_is_a_warning_not_a_crash() {
    let rig = healthy();
    add_repo_child(
        &rig,
        "demo",
        &rig.fx.root().join("no-such-repo"),
        State::Active,
    );
    assert_eq!(rig.check("worktrees-ignored").status, Status::Warn);
}

#[test]
fn json_carries_the_three_resolved_paths_and_every_check() {
    let rig = healthy();
    let (out, bufs) = rig.doctor(false, true);
    out.expect("healthy");
    let doc: Value = serde_json::from_str(bufs.out.text().trim()).expect("one JSON document");
    assert_eq!(
        doc["plugin_dir"],
        rig.install.plugin_dir().display().to_string()
    );
    assert_eq!(doc["mod_dir"], rig.install.mod_dir().display().to_string());
    assert_eq!(doc["herdr_app_home"], rig.app_home.display().to_string());
    assert_eq!(doc["ok"], true);
    assert_eq!(doc["checks"].as_array().expect("array").len(), 8);
    let first = &doc["checks"][0];
    assert_eq!(first["id"], "hc");
    assert_eq!(first["status"], "ok");
    assert!(first["detail"].is_string());
    assert!(first["fix"].is_null());
}

#[test]
fn a_missing_shim_in_json_names_crook_bin_as_the_fix() {
    let rig = healthy();
    std::fs::remove_file(rig.fx.root().join("data/mise/shims/crook")).expect("rm");
    let (out, bufs) = rig.doctor(false, true);
    out.expect("a warning is not a failure");
    let doc: Value = serde_json::from_str(bufs.out.text().trim()).expect("json");
    let shim = &doc["checks"][3];
    assert_eq!(shim["id"], "shim");
    assert_eq!(shim["status"], "warn");
    assert!(shim["fix"].as_str().expect("fix").contains("CROOK_BIN"));
}

#[test]
fn a_failing_check_exits_one_with_the_report_still_on_stdout() {
    let mut rig = healthy();
    rig.hc = FakeHc::new().with_version("0.1.1");
    let (out, bufs) = rig.doctor(false, true);
    let err = out.expect_err("fails");
    assert!(err.to_string().contains("1 check"), "{err}");
    let doc: Value = serde_json::from_str(bufs.out.text().trim()).expect("json");
    assert_eq!(doc["ok"], false);
    // Exactly one line on stdout (the report) and nothing mixed into it on stderr.
    assert_eq!(bufs.out.text().trim().lines().count(), 1);
    assert!(bufs.err.text().is_empty(), "{}", bufs.err.text());
    let failing: Vec<&str> = doc["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .filter(|c| c["status"] == "fail")
        .map(|c| c["id"].as_str().expect("id"))
        .collect();
    assert_eq!(failing, ["hc"]);
}

#[test]
fn text_output_marks_each_check_and_shows_the_fix_under_it() {
    let rig = healthy();
    std::fs::remove_file(rig.fx.root().join("data/mise/shims/crook")).expect("rm");
    let (_, bufs) = rig.doctor(false, false);
    let text = bufs.out.text();
    assert!(text.contains("warn  shim"), "{text}");
    assert!(text.contains("fix: "), "{text}");
    assert!(text.contains("ok    hc"), "{text}");
}

#[test]
fn fix_repairs_a_broken_link_and_a_missing_mod() {
    let rig = healthy();
    std::fs::remove_file(rig.fx.root().join("data/crook/current")).expect("rm");
    std::fs::remove_dir_all(rig.app_home.join("mods/crook")).expect("rm");
    assert_eq!(rig.check("mod").status, Status::Fail);
    assert_eq!(rig.check("current-link").status, Status::Fail);

    let (out, bufs) = rig.doctor(true, false);
    out.expect("fixed, so exit 0");
    assert_eq!(rig.check("mod").status, Status::Ok);
    assert_eq!(rig.check("current-link").status, Status::Ok);
    let text = bufs.out.text();
    assert!(text.contains("installed the mod"), "{text}");
    assert!(text.contains("pointed"), "{text}");
}

#[test]
fn fix_in_json_lists_what_it_did() {
    let rig = healthy();
    std::fs::remove_dir_all(rig.app_home.join("mods/crook")).expect("rm");
    let (out, bufs) = rig.doctor(true, true);
    out.expect("fixed");
    let doc: Value = serde_json::from_str(bufs.out.text().trim()).expect("json");
    assert!(
        doc["fixes"]
            .as_array()
            .expect("fixes")
            .iter()
            .any(|f| f.as_str().is_some_and(|f| f.contains("installed the mod")))
    );
}

#[test]
fn fix_leaves_a_healthy_mod_alone() {
    let rig = healthy();
    let sentinel = rig.app_home.join("mods/crook/sentinel.txt");
    std::fs::write(&sentinel, "x").expect("write");
    rig.doctor(true, false).0.expect("ok");
    assert!(sentinel.exists(), "a current mod is not reinstalled");
}

#[test]
fn fix_never_touches_a_mod_that_is_not_crooks_or_is_newer() {
    let rig = healthy();
    let marker = rig.app_home.join("mods/crook/.crook-version");
    std::fs::write(&marker, "0.9.0\n").expect("write");
    let (out, _) = rig.doctor(true, false);
    assert!(out.is_err(), "still failing");
    assert_eq!(std::fs::read_to_string(&marker).expect("marker"), "0.9.0\n");

    std::fs::remove_file(&marker).expect("rm");
    std::fs::write(rig.app_home.join("mods/crook/main.js"), "theirs").expect("write");
    assert!(rig.doctor(true, false).0.is_err());
    assert_eq!(
        std::fs::read_to_string(rig.app_home.join("mods/crook/main.js")).expect("read"),
        "theirs"
    );
}

#[test]
fn fix_prunes_old_session_records_and_changes_nothing_else() {
    let rig = healthy();
    let dir = rig.fx.root().join("state/crook/sessions");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let now = rig.fx.ctx().clock.now();
    for (name, days) in [("old", 31u64), ("recent", 29)] {
        let path = dir.join(name);
        std::fs::write(&path, "{}").expect("write");
        std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("open")
            .set_modified(now - Duration::from_secs(days * 86_400))
            .expect("mtime");
    }
    let mod_before = files_under(&rig.app_home.join("mods"));
    let link = rig.fx.root().join("data/crook/current");
    let target = std::fs::read_link(&link).expect("link");
    let (out, bufs) = rig.doctor(true, false);
    out.expect("ok");
    assert!(!dir.join("old").exists());
    assert!(dir.join("recent").exists());
    assert_eq!(
        std::fs::read_dir(&dir).expect("dir").count(),
        1,
        "only the recent record is left"
    );
    // A healthy mod and link are not rewritten, and only the prune is reported as done.
    assert_eq!(files_under(&rig.app_home.join("mods")), mod_before);
    assert_eq!(std::fs::read_link(&link).expect("link"), target);
    let fixed = bufs
        .out
        .text()
        .lines()
        .filter(|l| l.starts_with("fixed:"))
        .count();
    assert_eq!(fixed, 1, "{}", bufs.out.text());
    // `setup`'s other steps are not doctor's: no flocks root, no config.
    assert!(!rig.fx.flocks_root().exists());
    assert!(!rig.fx.root().join("config/crook").exists());
}

#[test]
fn without_fix_nothing_is_written() {
    let rig = healthy();
    std::fs::remove_dir_all(rig.app_home.join("mods/crook")).expect("rm");
    std::fs::remove_file(rig.fx.root().join("data/crook/current")).expect("rm");
    assert!(rig.doctor(false, false).0.is_err());
    assert!(!rig.app_home.join("mods/crook").exists());
    assert!(std::fs::symlink_metadata(rig.fx.root().join("data/crook/current")).is_err());
}

#[test]
fn the_login_shell_gets_a_minimal_environment_not_crooks() {
    let rig = healthy();
    executable(
        &rig.fx.root().join("fakeshell"),
        "#!/bin/sh\necho \"PATH=$PATH FOO=${FOO-unset} HOME=$HOME\"\n",
    );
    rig.fx.set_env("FOO", "leaked");
    rig.fx.set_env("MISE_SHELL", "leaked");
    let c = rig.check("login-shell");
    let home = rig.fx.root().join("home");
    assert!(
        c.detail.contains(&format!(
            "PATH=/usr/bin:/bin FOO=unset HOME={}",
            home.display()
        )),
        "{}",
        c.detail
    );
}

#[test]
fn a_broken_install_is_reported_not_raised() {
    let rig = healthy();
    for json in [true, false] {
        let mut bufs = Buffers::default();
        let out = report(
            &rig.fx.ctx(),
            &rig.hc,
            &mut bufs.io(),
            Err("there is no plugin/ folder".to_string()),
            false,
            false,
            json,
        );
        assert!(matches!(out, Err(Error::Checks(2))), "{out:?}");
        let text = bufs.out.text();
        assert!(text.contains("there is no plugin/ folder"), "{text}");
        if json {
            let doc: Value = serde_json::from_str(text.trim()).expect("json");
            assert!(doc["plugin_dir"].is_null() && doc["mod_dir"].is_null());
            assert_eq!(doc["ok"], false);
            let failing: Vec<&str> = doc["checks"]
                .as_array()
                .expect("checks")
                .iter()
                .filter(|c| c["status"] == "fail")
                .map(|c| c["id"].as_str().expect("id"))
                .collect();
            assert_eq!(failing, ["mod", "current-link"]);
        }
    }
}

#[test]
fn a_check_that_errors_fails_alone_and_the_rest_still_report() {
    let rig = healthy();
    // No usable HOME and no HERDR_APP_HOME: path-based checks cannot even name their folders.
    rig.fx.set_env("HERDR_APP_HOME", "");
    rig.fx.set_env("HOME", "");
    rig.fx.set_env("XDG_DATA_HOME", "");
    rig.fx.set_env("XDG_CONFIG_HOME", "");
    let (out, bufs) = rig.doctor(false, true);
    assert!(out.is_err());
    let doc: Value = serde_json::from_str(bufs.out.text().trim()).expect("json");
    assert_eq!(doc["checks"].as_array().expect("checks").len(), 8);
    assert_eq!(doc["herdr_app_home"], Value::Null);
}
