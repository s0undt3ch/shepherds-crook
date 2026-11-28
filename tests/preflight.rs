//! Preflight against a fake `hc` script that replays the captured fixtures and logs what it was
//! run with (task 4.5). Library-level: no crook command calls preflight yet.
#![allow(clippy::expect_used)]
#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crook::hc::client::HcClient;
use crook::hc::preflight::{self, Options};
use crook::run::RealRunner;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hc/0.1.22")
}

/// A fake `hc` that behaves like the real one where it matters here:
///
/// - any call but `--version` starts a core when none runs, with the environment of that call, and
///   the core's environment is saved to `core-env`;
/// - the first `schema` answer finds the core gone (it stops it and fails), as a core that dies
///   during preflight would.
///
/// A wrapper in front of it adds `CLAUDECODE` and `HC_TERMINAL_ID` to every call but `server`,
/// standing in for crook's own environment, which `schema` inherits. `log` gets each call's
/// arguments.
fn fake_hc(dir: &Path, version: &str) -> PathBuf {
    let log = dir.join("log");
    let core = dir.join("core-up");
    let core_env = dir.join("core-env");
    let seen = dir.join("schema-failed-once");
    let script = format!(
        r#"#!/bin/sh
case "$1" in
  --version) echo 'hc {version}'; exit 0 ;;
esac
echo "ARGS $*" >> '{log}'
if [ ! -e '{core}' ]; then
  touch '{core}'
  /usr/bin/env > '{core_env}'
fi
case "$1" in
  server) cat '{fx}/server_start_cold.stdout' ;;
  schema)
    if [ ! -e '{seen}' ]; then
      touch '{seen}'
      rm -f '{core}'
      exit 1
    fi
    cat '{fx}/schema.stdout' ;;
  *) exit 2 ;;
esac
"#,
        log = log.display(),
        core = core.display(),
        core_env = core_env.display(),
        seen = seen.display(),
        fx = fixtures().display(),
    );
    let real = dir.join("hc-real");
    std::fs::write(&real, script).expect("write");
    let wrapper = format!(
        "#!/bin/sh\n[ \"$1\" = server ] || {{ CLAUDECODE=1; HC_TERMINAL_ID=t9; export CLAUDECODE HC_TERMINAL_ID; }}\nexec '{}' \"$@\"\n",
        real.display()
    );
    let path = dir.join("hc");
    std::fs::write(&path, wrapper).expect("write");
    for p in [&real, &path] {
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    path
}

fn vars() -> Vec<(OsString, OsString)> {
    [
        ("PATH", "/usr/bin:/bin"),
        ("CLAUDECODE", "1"),
        ("CLAUDE_PID", "123"),
        ("CLAUDE_EFFORT", "high"),
        ("CLAUDE_CODE_SESSION_ID", "abc"),
        ("CLAUDE_CODE_CHILD_SESSION", "1"),
        ("CLAUDE_CODE_MESSAGING_SOCKET", "/s"),
        ("CLAUDE_CODE_ENTRYPOINT", "cli"),
        ("CLAUDE_CODE_EXECPATH", "/x/claude"),
        ("AI_AGENT", "claude-code"),
        ("HC_TERMINAL_ID", "t9"),
        ("HC_TERMINAL_SOCKET", "/t.sock"),
        ("HC_SCOPE", "s"),
        ("HC_SESSION", "scratch-core"),
        ("CLAUDE_CONFIG_DIR", "/home/me/.claude-work"),
        ("CLAUDE_CODE_ENABLE_TELEMETRY", "0"),
        ("XDG_STATE_HOME", "/scratch/state"),
    ]
    .iter()
    .map(|(k, v)| ((*k).into(), (*v).into()))
    .collect()
}

fn quick() -> Options {
    Options {
        start_deadline: Duration::from_secs(5),
        poll_interval: Duration::from_millis(20),
    }
}

#[test]
fn the_core_only_ever_starts_from_a_scrubbed_server_start() {
    let dir = tempfile::tempdir().expect("tempdir");
    let script = fake_hc(dir.path(), "0.1.22");
    let client = HcClient::new(&RealRunner).with_program(script.display().to_string());
    preflight::run_with(&client, &vars(), &quick()).expect("passes");

    let log = std::fs::read_to_string(dir.path().join("log")).expect("log");
    let calls: Vec<&str> = log.lines().collect();
    // The first schema call lost the core, so `server start` came again before the retry; had the
    // retry been a bare `schema`, it would have started a core with the wrapper's CLAUDECODE.
    assert_eq!(
        calls,
        [
            "ARGS server start --json",
            "ARGS schema",
            "ARGS server start --json",
            "ARGS schema"
        ]
    );
    let env_text = std::fs::read_to_string(dir.path().join("core-env")).expect("core env");
    let env: Vec<&str> = env_text.lines().collect();
    let has = |name: &str| env.iter().any(|l| l.split('=').next() == Some(name));
    for gone in [
        "CLAUDECODE",
        "CLAUDE_PID",
        "CLAUDE_EFFORT",
        "CLAUDE_CODE_SESSION_ID",
        "CLAUDE_CODE_CHILD_SESSION",
        "CLAUDE_CODE_MESSAGING_SOCKET",
        "CLAUDE_CODE_ENTRYPOINT",
        "CLAUDE_CODE_EXECPATH",
        "AI_AGENT",
        "HC_TERMINAL_ID",
        "HC_TERMINAL_SOCKET",
        "HC_SCOPE",
    ] {
        assert!(!has(gone), "{gone} reached the core:\n{env:#?}");
    }
    for kept in [
        "PATH",
        "HC_SESSION",
        "CLAUDE_CONFIG_DIR",
        "CLAUDE_CODE_ENABLE_TELEMETRY",
        "XDG_STATE_HOME",
    ] {
        assert!(has(kept), "{kept} was dropped:\n{env:#?}");
    }
}

#[test]
fn an_old_hc_is_refused_before_the_core_is_touched() {
    let dir = tempfile::tempdir().expect("tempdir");
    let script = fake_hc(dir.path(), "0.1.9");
    let client = HcClient::new(&RealRunner).with_program(script.display().to_string());
    let msg = preflight::run_with(&client, &vars(), &quick())
        .expect_err("refused")
        .to_string();
    assert!(msg.contains("0.1.22") && msg.contains("0.1.9"), "{msg}");
    assert!(
        !dir.path().join("log").exists(),
        "an old hc must not reach the core"
    );
}
