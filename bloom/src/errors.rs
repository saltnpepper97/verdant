use std::fmt;
use nix::Error as NixError;

#[derive(Debug)]
pub enum BloomError {
    Io(std::io::Error),
    Parse(String),
    InvalidCommand,
    NotFound,
    ServiceFailed,
    Nix(NixError),
    Custom(String),
}

impl fmt::Display for BloomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BloomError::Io(e) => write!(f, "IO error: {}", e),
            BloomError::Parse(msg) => write!(f, "Parse error: {}", msg),
            BloomError::InvalidCommand => write!(f, "Invalid command"),
            BloomError::NotFound => write!(f, "Not found"),
            BloomError::ServiceFailed => write!(f, "Service failed"),
            BloomError::Nix(e) => write!(f, "Nix error: {}", e),
            BloomError::Custom(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl std::error::Error for BloomError {}

impl From<std::io::Error> for BloomError {
    fn from(err: std::io::Error) -> BloomError {
        BloomError::Io(err)
    }
}

impl From<NixError> for BloomError {
    fn from(err: NixError) -> BloomError {
        BloomError::Nix(err)
    }
}

