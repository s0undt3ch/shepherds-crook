use std::collections::HashMap;

use super::*;

fn run(entry: Entry, pairs: &[(&str, &str)]) -> Result<Identity, Error> {
    let m: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    check(entry, &move |k: &str| m.get(k).cloned())
}

fn msg(r: Result<Identity, Error>) -> String {
    r.unwrap_err().to_string()
}

#[test]
fn gate_user() {
    assert!(msg(run(Entry::User, &[("CLAUDECODE", "1")])).contains("CLAUDECODE"));
    assert!(run(Entry::User, &[("CLAUDECODE", "")]).is_ok());
    assert!(msg(run(Entry::User, &[("CROOK_ROLE", "child")])).contains("CROOK_ROLE"));
    assert!(run(Entry::User, &[]).is_ok());
}

#[test]
fn gate_mod() {
    let orch = [("CROOK_ROLE", "orchestrator"), ("CROOK_PROJECT", "p")];
    assert!(msg(run(Entry::Mod, &[("CROOK_PROJECT", "p")])).contains("CROOK_ROLE"));
    let blocked = [
        ("CLAUDECODE", "1"),
        ("CROOK_ROLE", "orchestrator"),
        ("CROOK_PROJECT", "p"),
    ];
    assert!(msg(run(Entry::Mod, &blocked)).contains("CLAUDECODE"));
    let empty_cc = [
        ("CLAUDECODE", ""),
        ("CROOK_ROLE", "orchestrator"),
        ("CROOK_PROJECT", "p"),
    ];
    assert!(run(Entry::Mod, &empty_cc).is_ok());

    let id = run(Entry::Mod, &orch).unwrap();
    assert_eq!(
        id,
        Identity {
            slug: "p".into(),
            role: Role::Orchestrator,
            child: None
        }
    );

    let child = [
        ("CROOK_ROLE", "child"),
        ("CROOK_PROJECT", "p"),
        ("CROOK_CHILD", "c1"),
    ];
    let id = run(Entry::Mod, &child).unwrap();
    assert_eq!(id.role, Role::Child);
    assert_eq!(id.child.as_deref(), Some("c1"));

    let no_child = [("CROOK_ROLE", "child"), ("CROOK_PROJECT", "p")];
    assert!(msg(run(Entry::Mod, &no_child)).contains("CROOK_CHILD"));
}

#[test]
fn gate_mod_validates_slug_and_role() {
    let bad_slug = [("CROOK_ROLE", "orchestrator"), ("CROOK_PROJECT", "../x")];
    assert!(run(Entry::Mod, &bad_slug).is_err());
    assert!(run(Entry::Mod, &[("CROOK_ROLE", "orchestrator")]).is_err());
    let bad_role = [("CROOK_ROLE", "root"), ("CROOK_PROJECT", "p")];
    assert!(run(Entry::Mod, &bad_role).is_err());
}

#[test]
fn gate_hook_never_refuses() {
    let none = run(Entry::Hook, &[]).unwrap();
    assert_eq!(none.role, Role::None);
    let cc = [
        ("CLAUDECODE", "1"),
        ("CROOK_ROLE", "orchestrator"),
        ("CROOK_PROJECT", "p"),
    ];
    let id = run(Entry::Hook, &cc).unwrap();
    assert_eq!((id.role, id.slug.as_str()), (Role::Orchestrator, "p"));
    let junk = [("CROOK_ROLE", "child"), ("CROOK_PROJECT", "../x")];
    assert_eq!(run(Entry::Hook, &junk).unwrap().role, Role::None);
}
