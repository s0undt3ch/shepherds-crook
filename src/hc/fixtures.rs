//! Real `hc` output captured from a scratch core (`tests/fixtures/hc/<version>/README.md`), for
//! the tests that parse it.

use std::path::PathBuf;

use crate::run::Output;

pub const HC_VERSION: &str = "0.1.22";

pub fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hc")
        .join(HC_VERSION)
}

/// What the captured call printed and exited with; an absent stderr is empty.
#[allow(clippy::expect_used)]
pub fn fixture(name: &str) -> Output {
    let read = |ext: &str| std::fs::read_to_string(dir().join(format!("{name}.{ext}"))).ok();
    let code = read("exit")
        .unwrap_or_else(|| panic!("no fixture {name}"))
        .trim()
        .parse()
        .expect("exit code");
    Output {
        code: Some(code),
        stdout: read("stdout").unwrap_or_default(),
        stderr: read("stderr").unwrap_or_default(),
    }
}
