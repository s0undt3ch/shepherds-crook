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
}
