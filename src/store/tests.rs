use std::time::Duration;

use super::*;
use crate::paths;

fn env_for(dir: &Path) -> impl Fn(&str) -> Option<String> + use<> {
    let x = dir.to_str().unwrap().to_string();
    move |k: &str| (k == "XDG_STATE_HOME").then(|| x.clone())
}

const TABLES: &[&str] = &[
    "project",
    "project_item",
    "orchestrator",
    "children",
    "inbox",
    "status",
    "forks",
    "seen",
    "notified",
    "asks",
];

#[test]
fn migrate_creates_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let env = env_for(tmp.path());
    let s = Store::open(&env, "p", Mode::Migrate).unwrap();
    s.read(|c| {
        let v: i32 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        assert_eq!(v, SCHEMA_VERSION);
        let m: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
        assert_eq!(m, "wal");
        for t in TABLES {
            let n: i64 = c.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [t],
                |r| r.get(0),
            )?;
            assert_eq!(n, 1, "table {t}");
        }
        Ok(())
    })
    .unwrap();
}

#[test]
fn migrate_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let env = env_for(tmp.path());
    drop(Store::open(&env, "p", Mode::Migrate).unwrap());
    Store::open(&env, "p", Mode::Migrate).unwrap();
}

#[test]
fn open_refuses_newer_or_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let env = env_for(tmp.path());
    let err = Store::open(&env, "p", Mode::ReadWrite).err().unwrap();
    assert!(matches!(err, Error::Schema { .. }));
    assert!(err.to_string().contains("run crook open"));
    assert!(!tmp.path().join("crook").exists(), "nothing created");

    let dir = paths::project_dir(&env, "p").unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let c = rusqlite::Connection::open(dir.join("crook.db")).unwrap();
    c.pragma_update(None, "user_version", 99).unwrap();
    drop(c);
    let err = Store::open(&env, "p", Mode::ReadWrite).err().unwrap();
    assert!(matches!(err, Error::Schema { .. }));
}

#[test]
fn open_refuses_older_for_read_write() {
    let tmp = tempfile::tempdir().unwrap();
    let env = env_for(tmp.path());
    let dir = paths::project_dir(&env, "p").unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let c = rusqlite::Connection::open(dir.join("crook.db")).unwrap();
    c.execute_batch("CREATE TABLE project(id INTEGER)").unwrap();
    drop(c);
    let err = Store::open(&env, "p", Mode::ReadWrite).err().unwrap();
    assert!(matches!(err, Error::Schema { .. }));
}

#[test]
fn read_only_older_version() {
    let tmp = tempfile::tempdir().unwrap();
    let env = env_for(tmp.path());
    let dir = paths::project_dir(&env, "p").unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let c = rusqlite::Connection::open(dir.join("crook.db")).unwrap();
    c.execute_batch("CREATE TABLE project(id INTEGER)").unwrap();
    drop(c);
    let s = Store::open(&env, "p", Mode::ReadOnly).unwrap();
    let v: i32 = s
        .read(|c| Ok(c.query_row("PRAGMA user_version", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(v, 0, "not migrated");
}

#[test]
fn read_only_missing_creates_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let env = env_for(tmp.path());
    assert!(Store::open(&env, "p", Mode::ReadOnly).is_err());
    assert!(!tmp.path().join("crook").exists());
}

#[test]
fn write_rolls_back_on_error() {
    let tmp = tempfile::tempdir().unwrap();
    let env = env_for(tmp.path());
    let mut s = Store::open(&env, "p", Mode::Migrate).unwrap();
    let r: Result<(), Error> = s.write(Touch::INBOX, |tx| {
        tx.execute("INSERT INTO seen(child, seq) VALUES ('a', 1)", [])?;
        Err(Error::Refused("nope".into()))
    });
    assert!(r.is_err());
    let n: i64 = s
        .read(|c| Ok(c.query_row("SELECT count(*) FROM seen", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(n, 0);
    assert!(!s.gen_path().exists(), "failed write does not bump gen");
}

fn mtime(p: &Path) -> std::time::SystemTime {
    std::fs::metadata(p).unwrap().modified().unwrap()
}

#[test]
fn gen_bump() {
    let tmp = tempfile::tempdir().unwrap();
    let env = env_for(tmp.path());
    let mut s = Store::open(&env, "p", Mode::Migrate).unwrap();
    s.write(Touch::INBOX, |_| Ok(())).unwrap();
    let m1 = mtime(s.gen_path());
    std::thread::sleep(Duration::from_millis(30));
    s.write(Touch::empty(), |_| Ok(())).unwrap();
    assert_eq!(mtime(s.gen_path()), m1, "empty touch leaves gen alone");
    s.read(|_| Ok(())).unwrap();
    assert_eq!(mtime(s.gen_path()), m1, "read leaves gen alone");
    s.write(Touch::INBOX, |_| Ok(())).unwrap();
    assert!(mtime(s.gen_path()) > m1, "INBOX touch bumps gen");
}
