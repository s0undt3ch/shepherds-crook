//! The completion scripts, run in the shells that are installed, against a scratch flocks root.
#![allow(clippy::expect_used)]
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Rig {
    dir: tempfile::TempDir,
}

impl Rig {
    fn new() -> Rig {
        let dir = tempfile::tempdir().expect("tempdir");
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).expect("mkdir");
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_crook"), bin.join("crook")).expect("link");
        // Two flocks, as `crook new` leaves them.
        for (slug, child) in [("alpha", "auth"), ("beta", "billing")] {
            let folder = dir
                .path()
                .join("data/crook/flocks")
                .join(format!("2026-11-27-{slug}"));
            std::fs::create_dir_all(folder.join("briefs")).expect("mkdir");
            let roster = serde_json::json!({
                "version": 1,
                "flock": slug,
                "coordinator": {"session_id": "00000000-0000-4000-8000-0000000000aa",
                    "folder": folder, "state": "active", "started": true},
                "children": [{"name": child, "role": "child", "model": "sonnet",
                    "session_id": "00000000-0000-4000-8000-0000000000bb", "folder": "/w",
                    "repo": null, "branch": null, "brief": format!("briefs/{child}.md"),
                    "state": "active", "started": true}],
            });
            std::fs::write(folder.join("roster.json"), roster.to_string()).expect("roster");
        }
        Rig { dir }
    }

    fn path(&self) -> PathBuf {
        self.dir.path().to_path_buf()
    }

    fn cmd(&self, program: &str) -> Command {
        let mut cmd = Command::new(program);
        cmd.env_clear()
            .env(
                "PATH",
                format!(
                    "{}:/usr/bin:/bin:/opt/homebrew/bin",
                    self.path().join("bin").display()
                ),
            )
            .env("HOME", self.path().join("home"))
            .env("XDG_DATA_HOME", self.path().join("data"))
            .env("XDG_CONFIG_HOME", self.path().join("config"))
            .env("XDG_STATE_HOME", self.path().join("state"));
        cmd
    }

    fn script(&self, shell: &str) -> String {
        let out = self
            .cmd(env!("CARGO_BIN_EXE_crook"))
            .args(["completion", "print", shell])
            .output()
            .expect("runs");
        assert!(out.status.success());
        String::from_utf8(out.stdout).expect("utf8")
    }
}

fn have(tool: &str) -> bool {
    ["/usr/bin", "/bin", "/opt/homebrew/bin", "/usr/local/bin"]
        .iter()
        .any(|d| Path::new(d).join(tool).exists())
}

fn lines(out: &Output) -> Vec<String> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

/// What bash would offer for `words`, the last being the one being typed.
fn bash_offers(rig: &Rig, words: &[&str]) -> Vec<String> {
    let script_path = rig.path().join("crook.bash");
    std::fs::write(&script_path, rig.script("bash")).expect("write");
    let quoted: Vec<String> = words.iter().map(|w| format!("'{w}'")).collect();
    let program = format!(
        "source '{}'; COMP_WORDS=(crook {}); COMP_CWORD={}; _crook; printf '%s\\n' \"${{COMPREPLY[@]}}\"",
        script_path.display(),
        quoted.join(" "),
        words.len()
    );
    let out = rig
        .cmd("bash")
        .args(["-c", &program])
        .output()
        .expect("bash runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    lines(&out)
}

#[test]
fn bash_completes_subcommands_slugs_and_children() {
    if !have("bash") {
        return;
    }
    let rig = Rig::new();
    let subs = bash_offers(&rig, &["s"]);
    for want in ["spawn", "stop", "status", "setup"] {
        assert!(subs.contains(&want.to_string()), "{subs:?}");
    }
    assert!(!subs.contains(&"new".to_string()), "{subs:?}");
    assert_eq!(bash_offers(&rig, &["open", ""]), ["alpha", "beta"]);
    assert_eq!(bash_offers(&rig, &["status", "al"]), ["alpha"]);
    assert_eq!(bash_offers(&rig, &["stop", ""]), ["auth", "billing"]);
    assert_eq!(
        bash_offers(&rig, &["stop", "--flock", "beta", ""]),
        ["billing"]
    );
    assert_eq!(
        bash_offers(&rig, &["spawn", "--flock", ""]),
        ["alpha", "beta"]
    );
    assert_eq!(bash_offers(&rig, &["completion", ""]), ["print"]);
    assert_eq!(bash_offers(&rig, &["completion", "print", "f"]), ["fish"]);
}

#[test]
fn zsh_and_fish_scripts_are_valid_syntax() {
    let rig = Rig::new();
    if have("zsh") {
        let path = rig.path().join("_crook");
        std::fs::write(&path, rig.script("zsh")).expect("write");
        let out = rig.cmd("zsh").arg("-n").arg(&path).output().expect("zsh");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    if have("fish") {
        let path = rig.path().join("crook.fish");
        std::fs::write(&path, rig.script("fish")).expect("write");
        let out = rig
            .cmd("fish")
            .arg("--no-execute")
            .arg(&path)
            .output()
            .expect("fish");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn the_hidden_helper_is_not_in_help() {
    let rig = Rig::new();
    let out = rig
        .cmd(env!("CARGO_BIN_EXE_crook"))
        .args(["completion", "--help"])
        .output()
        .expect("runs");
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(help.contains("print"), "{help}");
    assert!(!help.contains("list"), "{help}");
}
