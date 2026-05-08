use std::fmt;
use std::path::PathBuf;
use std::process::ExitStatus;

use crate::vm_id::VmIdError;

/// Errors returned by the Firecracker SDK.
#[derive(Debug)]
pub enum Error {
    /// API error with error body from Firecracker.
    Api(Box<fc_api::Error<fc_api::types::Error>>),

    /// API error without error body (e.g., for endpoints with only default response).
    ApiNoBody(Box<fc_api::Error<()>>),

    /// HTTP/network error.
    Http(reqwest::Error),

    /// I/O error.
    Io(std::io::Error),

    /// Failed to spawn a process.
    SpawnFailed(std::io::Error),

    /// Timed out waiting for the API socket to become available.
    SocketTimeout(PathBuf),

    /// The process exited unexpectedly.
    ProcessExited(Option<ExitStatus>),

    /// Missing required configuration.
    MissingConfig(&'static str),

    /// Caller supplied an identifier that Firecracker would reject.
    InvalidVmId(VmIdError),

    /// Other error.
    Other(String),
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Api(e) => Some(e),
            Self::ApiNoBody(e) => Some(e),
            Self::Http(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::SpawnFailed(e) => Some(e),
            Self::InvalidVmId(e) => Some(e),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Api(e) => write!(f, "API error: {e}"),
            Self::ApiNoBody(e) => write!(f, "API error: {e}"),
            Self::Http(e) => write!(f, "HTTP error: {e}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::SpawnFailed(e) => write!(f, "failed to spawn process: {e}"),
            Self::SocketTimeout(path) => {
                write!(f, "timed out waiting for socket: {}", path.display())
            }
            Self::ProcessExited(Some(status)) => {
                write!(f, "process exited unexpectedly: {status}")
            }
            Self::ProcessExited(None) => write!(f, "process exited unexpectedly"),
            Self::MissingConfig(field) => write!(f, "missing required configuration: {field}"),
            Self::InvalidVmId(e) => write!(f, "invalid VM id: {e}"),
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl From<fc_api::Error<fc_api::types::Error>> for Error {
    fn from(err: fc_api::Error<fc_api::types::Error>) -> Self {
        Self::Api(Box::new(err))
    }
}

impl From<fc_api::Error<()>> for Error {
    fn from(err: fc_api::Error<()>) -> Self {
        Self::ApiNoBody(Box::new(err))
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Self::Http(err)
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<VmIdError> for Error {
    fn from(err: VmIdError) -> Self {
        Self::InvalidVmId(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
