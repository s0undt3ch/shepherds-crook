//! Shared by the binary's tests that write a `roster.json`: they check it against the committed schema.
#![allow(dead_code, clippy::expect_used)]

use std::path::Path;

use serde_json::Value;

pub fn roster_schema() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("schema/roster.schema.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("schema file")).expect("schema json")
}

/// Every way `roster` breaks the schema, empty when it is valid.
pub fn roster_errors(roster: &Value) -> Vec<String> {
    let validator = jsonschema::validator_for(&roster_schema()).expect("schema compiles");
    validator
        .iter_errors(roster)
        .map(|e| format!("{}: {e}", e.instance_path()))
        .collect()
}

/// Panics, listing the errors, when the roster file in `folder` does not validate.
pub fn assert_roster_file_valid(folder: &Path) {
    let text = std::fs::read_to_string(folder.join("roster.json")).expect("roster file");
    let value: Value = serde_json::from_str(&text).expect("roster json");
    let errors = roster_errors(&value);
    assert!(errors.is_empty(), "{errors:#?}\n{text}");
}

pub fn status_schema() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("schema/status.schema.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("schema file")).expect("schema json")
}

/// Every way `status` (a `crook status --json` document) breaks the schema, empty when it is valid.
pub fn status_errors(status: &Value) -> Vec<String> {
    let validator = jsonschema::validator_for(&status_schema()).expect("schema compiles");
    validator
        .iter_errors(status)
        .map(|e| format!("{}: {e}", e.instance_path()))
        .collect()
}
