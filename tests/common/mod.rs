use std::process::Command;

/// The built binary with a cleared environment. `PATH` and the coverage
/// variables go back in so binary-driven tests still count under `cargo llvm-cov`.
pub fn crook() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_crook"));
    cmd.env_clear();
    for (k, v) in std::env::vars() {
        if k == "PATH" || k == "LLVM_PROFILE_FILE" || k.starts_with("CARGO_LLVM_COV") {
            cmd.env(k, v);
        }
    }
    cmd
}
