use super::*;

const VALID: &[&str] = &["a", "a1", "1", "abc-def", "a-b-c", "0-0", "x9-y"];

fn invalid() -> Vec<String> {
    let mut v: Vec<String> = [
        "", "A", "aB", "a/b", "..", "a..b", "-a", "a-", "-", "a--b", "a_b", "a.b", "a b", "é",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();
    v.push("a".repeat(17));
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
fn the_limits_keep_the_hc_name_within_32_characters() {
    assert!(validate_slug(&"a".repeat(16)).is_ok());
    assert!(validate_slug(&"a".repeat(17)).is_err());
    assert!(validate_child(&"a".repeat(15)).is_ok());
    assert!(validate_child(&"a".repeat(16)).is_err());
    let longest = hc_child(&"a".repeat(MAX_SLUG_LEN), &"b".repeat(MAX_CHILD_LEN));
    assert_eq!(longest.len(), 32);
}

#[test]
fn a_slug_shaped_like_an_hc_address_is_refused_but_a_child_name_may_be() {
    for bad in ["t0", "t7", "t123", "t1x", "t1-x", "t7a"] {
        let msg = validate_slug(bad).expect_err(bad).to_string();
        assert!(msg.contains("address"), "{msg}");
        assert!(validate_child(bad).is_ok(), "{bad}");
    }
    for ok in ["t", "tt7", "t-7", "ta7", "a7", "7t"] {
        validate_slug(ok).unwrap_or_else(|e| panic!("{ok}: {e}"));
    }
}

#[test]
fn the_length_error_says_the_limit_and_why() {
    let msg = validate_slug(&"a".repeat(17))
        .expect_err("fails")
        .to_string();
    assert!(msg.contains("16") && msg.contains("32"), "{msg}");
    let msg = validate_child(&"a".repeat(16))
        .expect_err("fails")
        .to_string();
    assert!(msg.contains("15") && msg.contains("32"), "{msg}");
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
