use super::*;

const VALID: &[&str] = &["a", "a1", "1", "abc-def", "a-b-c", "0-0", "x9-y"];

fn invalid() -> Vec<String> {
    let mut v: Vec<String> = [
        "", "A", "aB", "a/b", "..", "a..b", "-a", "a-", "-", "a--b", "a_b", "a.b", "a b", "é",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();
    v.push("a".repeat(41));
    v
}

#[test]
fn valid_slugs_and_child_names() {
    for name in VALID {
        validate_slug(name).unwrap_or_else(|e| panic!("{name}: {e}"));
        validate_child(name).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn forty_characters_is_valid_and_forty_one_is_not() {
    assert!(validate_slug(&"a".repeat(40)).is_ok());
    assert!(validate_child(&"a".repeat(40)).is_ok());
    assert!(validate_slug(&"a".repeat(41)).is_err());
    assert!(validate_child(&"a".repeat(41)).is_err());
}

#[test]
fn invalid_slugs_and_child_names_are_refused() {
    for name in invalid() {
        assert!(validate_slug(&name).is_err(), "slug {name:?}");
        assert!(validate_child(&name).is_err(), "child {name:?}");
    }
}

#[test]
fn the_error_names_the_kind_and_the_value() {
    let msg = validate_slug("Bad").expect_err("fails").to_string();
    assert!(msg.contains("slug") && msg.contains("Bad"), "{msg}");
    let msg = validate_child("Bad").expect_err("fails").to_string();
    assert!(msg.contains("child name"), "{msg}");
}

#[test]
fn every_derived_name_for_one_example() {
    assert_eq!(folder_name("2026-11-27", "demo"), "2026-11-27-demo");
    assert_eq!(hc_coordinator("demo"), "demo");
    assert_eq!(hc_child("demo", "fix-bug"), "demo.fix-bug");
    assert_eq!(herdr_space("demo"), "crook-demo");
    assert_eq!(herdr_tab("demo"), "crook-demo-coord");
    assert_eq!(branch("demo", "fix-bug"), "crook/demo/fix-bug");
    assert_eq!(worktree_dir("demo", "fix-bug"), "demo-fix-bug");
}

#[test]
fn the_coordinator_gets_herdr_labels_and_a_child_does_not() {
    assert_eq!(
        coordinator_labels("demo", "0.1.0"),
        [
            "crook.flock=demo",
            "crook.version=0.1.0",
            "herdr.space=crook-demo",
            "herdr.tab=crook-demo-coord",
        ]
    );
    assert_eq!(
        child_labels("demo", "fix-bug", "0.1.0"),
        [
            "crook.flock=demo",
            "crook.version=0.1.0",
            "crook.child=fix-bug"
        ]
    );
}
