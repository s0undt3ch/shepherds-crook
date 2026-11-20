use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid slug {0:?}: use lowercase letters, digits and `-`")]
    InvalidSlug(String),
    #[error("refused: {0}")]
    Refused(String),
    #[error("run crook open {slug}")]
    Schema { slug: String },
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
