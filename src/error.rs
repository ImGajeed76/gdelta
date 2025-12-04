//! Error types for GDelta operations.

use std::fmt;

/// Result type for GDelta operations.
pub type Result<T> = std::result::Result<T, GDeltaError>;

/// Errors that can occur during delta encoding or decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GDeltaError {
    /// The delta data is corrupted or invalid.
    InvalidDelta(String),

    /// An unexpected end of data was encountered.
    UnexpectedEndOfData,

    /// The decoded data does not match expected size.
    SizeMismatch {
        /// Expected size
        expected: usize,
        /// Actual size
        actual: usize,
    },

    /// Buffer operation failed.
    BufferError(String),
}

impl fmt::Display for GDeltaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GDeltaError::InvalidDelta(msg) => write!(f, "Invalid delta: {}", msg),
            GDeltaError::UnexpectedEndOfData => write!(f, "Unexpected end of data"),
            GDeltaError::SizeMismatch { expected, actual } => {
                write!(
                    f,
                    "Size mismatch: expected {} bytes, got {} bytes",
                    expected, actual
                )
            }
            GDeltaError::BufferError(msg) => write!(f, "Buffer error: {}", msg),
        }
    }
}

impl std::error::Error for GDeltaError {}
