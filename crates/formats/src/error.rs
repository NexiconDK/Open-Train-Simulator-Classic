use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong while reading game files.
#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// The data is not in the expected format. `offset` is the byte position
    /// in the input where the problem was noticed.
    Malformed {
        offset: usize,
        message: String,
    },
    Xml(String),
    Archive(String),
    NotFound(String),
}

impl Error {
    pub(crate) fn malformed(offset: usize, message: impl Into<String>) -> Self {
        Error::Malformed {
            offset,
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::Malformed { offset, message } => {
                write!(f, "malformed data at offset {offset:#x}: {message}")
            }
            Error::Xml(m) => write!(f, "XML error: {m}"),
            Error::Archive(m) => write!(f, "archive error: {m}"),
            Error::NotFound(m) => write!(f, "not found: {m}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}
