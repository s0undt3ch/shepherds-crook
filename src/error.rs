use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid slug {0:?}: use lowercase letters, digits and `-`")]
    InvalidSlug(String),
    #[error("refused: {0}")]
    Refused(String),
    #[error("the project card changed since you read it: read it again and retry")]
    Stale,
    #[error("run crook open {slug}")]
    Schema { slug: String },
    #[error("config {path}: {msg}")]
    Config { path: String, msg: String },
    #[error("private entry is not absolute: {0}")]
    RelativePrivate(String),
    #[error("claude agents: {0}")]
    Agents(String),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
