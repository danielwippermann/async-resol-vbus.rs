/// A common error type.
#[derive(Debug, PartialEq)]
pub struct Error {
    message: String,
}

/// A common result type.
pub type Result<T> = std::result::Result<T, Error>;

pub trait IntoError: std::fmt::Display {}

impl<T: IntoError> From<T> for Error {
    fn from(other: T) -> Error {
        let message = format!("{}", other);
        Error { message }
    }
}

impl IntoError for &str {}
impl IntoError for String {}
impl IntoError for std::io::Error {}
impl IntoError for std::net::AddrParseError {}
impl IntoError for std::str::Utf8Error {}
impl IntoError for async_std::future::TimeoutError {}
