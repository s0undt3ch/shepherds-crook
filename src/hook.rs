use std::io::Read;

use crate::Env;

/// Stub until Task 9: drains stdin and answers nothing. The hook always exits 0.
pub fn main(_args: &[String], _env: Env) -> i32 {
    let mut sink = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut sink);
    0
}
