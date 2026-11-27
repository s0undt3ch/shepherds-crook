//! The shipped result template must be readable by the binary's own parser: a child copies it, and
//! crook then reads its `status` (ADR 0024). `tools/ci.py` checks the template's headings.
#![allow(clippy::expect_used)]

use std::path::Path;

use crook::files::result::{Status, parse_status, read};

fn template() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("plugin/skills/crook-child/result-template.md")
}

#[test]
fn the_shipped_result_template_parses_as_working() {
    let text = std::fs::read_to_string(template()).expect("read the result template");
    assert_eq!(parse_status(&text), Status::Working);
}

#[test]
fn reading_the_shipped_result_template_gives_working() {
    let info = read(&template()).expect("read");
    assert_eq!(info.status, Status::Working);
}
