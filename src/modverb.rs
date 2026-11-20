use std::io::Write;

use crate::Env;

/// Stub until Task 3: every mod verb answers with a not-implemented error.
pub fn main(_args: &[String], _env: Env) -> i32 {
    let _ = writeln!(std::io::stdout(), r#"{{"error":"not implemented"}}"#);
    1
}
