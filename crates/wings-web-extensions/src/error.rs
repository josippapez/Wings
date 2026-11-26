use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Io(String),
    /// `manifest.json` is missing, not JSON, or lacks what an extension needs.
    Manifest(String),
    /// Themes and apps are not extensions the Browser can run.
    Unsupported(String),
    /// A ZIP, CRX or XPI that cannot be read.
    Archive(String),
    /// An entry that would write outside the target folder, or a symlink.
    Unsafe(String),
    /// More files or bytes than the limits allow.
    TooLarge(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(m) => write!(f, "io: {m}"),
            Error::Manifest(m) => write!(f, "manifest: {m}"),
            Error::Unsupported(m) => write!(f, "unsupported: {m}"),
            Error::Archive(m) => write!(f, "archive: {m}"),
            Error::Unsafe(m) => write!(f, "unsafe entry: {m}"),
            Error::TooLarge(m) => write!(f, "too large: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

impl From<zip::result::ZipError> for Error {
    fn from(e: zip::result::ZipError) -> Self {
        Error::Archive(e.to_string())
    }
}
