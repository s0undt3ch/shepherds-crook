use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid slug {0:?}: use lowercase letters, digits and `-`")]
    InvalidSlug(String),
    #[error("refused: {0}")]
    Refused(String),
}
