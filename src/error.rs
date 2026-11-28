use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("crook {0}: not implemented")]
    NotImplemented(&'static str),
    #[error("{program}: could not start: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{program}: timed out after {after:?}")]
    Timeout { program: String, after: Duration },
    #[error("{}: working folder unusable: {source}", .path.display())]
    WorkDir {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{program}: {source}")]
    Io {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{}: {source}", .path.display())]
    File {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid {kind} {name:?}: {reason}")]
    InvalidName {
        kind: &'static str,
        name: String,
        reason: &'static str,
    },
    #[error("{}: {message}", .path.display())]
    Config {
        path: std::path::PathBuf,
        message: String,
    },
    #[error("{}: {message}", .path.display())]
    Roster {
        path: std::path::PathBuf,
        message: String,
    },
    #[error("{}: {message}", .path.display())]
    Marks {
        path: std::path::PathBuf,
        message: String,
    },
    #[error("{0}")]
    Flock(String),
    #[error("{0}")]
    Env(String),
    #[error("git {args}: {message}")]
    Git { args: String, message: String },
    #[error("{0}")]
    Install(String),
    /// `kind` is hc's own error kind (`not_found`, `name_taken` and so on) when it printed one.
    #[error("hc {args}: {message}")]
    Hc {
        args: String,
        kind: Option<String>,
        message: String,
    },
    #[error("{0}")]
    Preflight(String),
    #[error("doctor: {0} check(s) failed")]
    Checks(usize),
    /// `crook send` exits with hc's own code, so the coordinator's skill reads 9 and 11 as before (ADR 0028).
    #[error("{message}")]
    Send { code: u8, message: String },
}

impl Error {
    /// The process exit code: hc's own for a send that did not land, 1 for everything else.
    pub fn exit_code(&self) -> u8 {
        match self {
            Error::Send { code, .. } => *code,
            _ => 1,
        }
    }
}
