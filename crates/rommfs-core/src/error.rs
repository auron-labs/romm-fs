//! Error taxonomy. The variants distinguish login failure, permission failure,
//! server/network failure, and local I/O so status/logs stay truthful (PRD R1).

/// All fallible core operations return this.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Authentication was rejected (bad credentials or expired token).
    /// The UI must show "sign-in required".
    #[error("authentication failed: {0}")]
    Auth(String),

    /// Authenticated but the token lacks the required scope (HTTP 403).
    #[error("permission denied: {0}")]
    Forbidden(String),

    /// Server returned an unexpected HTTP status.
    #[error("server error {status}: {message}")]
    Http { status: u16, message: String },

    /// Connection, DNS, or stream-level failure, including stalls that were
    /// aborted by the no-progress timeout.
    #[error("transport error: {0}")]
    Transport(String),

    /// A download ended before the declared content length.
    #[error("truncated body: expected {expected} bytes, got {received}")]
    Truncated { expected: u64, received: u64 },

    /// Response body could not be parsed as the documented contract.
    #[error("invalid catalogue payload: {0}")]
    InvalidCatalogue(String),

    /// Local filesystem / SQLite failure.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Caller asked for something the PoC does not support.
    #[error("unsupported: {0}")]
    Unsupported(String),

    /// Operation was cancelled by stop/quit.
    #[error("cancelled")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// True when the failure means the user must sign in again.
    pub fn needs_sign_in(&self) -> bool {
        matches!(self, Error::Auth(_))
    }
}
