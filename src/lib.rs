//! # gdelta
//!
//! A fast delta compression algorithm for similar data chunks.
//!
//! `GDelta` efficiently encodes the differences between similar data chunks using
//! GEAR rolling hash for pattern matching and variable-length integer encoding
//! for space efficiency.
//!
//! ## Quick Start
//!
//! ```
//! use gdelta::{encode, decode};
//!
//! let base = b"Hello, World!";
//! let new = b"Hello, Rust!";
//!
//! // Encode the delta
//! let delta = encode(new, base).unwrap();
//!
//! // Decode to recover the new data
//! let recovered = decode(&delta, base).unwrap();
//! assert_eq!(recovered, new);
//! ```
//!
//! ## Algorithm Details
//!
//! The algorithm works by:
//! 1. Finding common prefix and suffix between base and new data
//! 2. Building a hash table of the base data using GEAR rolling hash
//! 3. Scanning the new data to find matches in the base
//! 4. Encoding the result as copy and literal instructions
//!
//! ## Performance
//!
//! `GDelta` is optimized for:
//! - Speed: Faster than Xdelta, Zdelta, Ddelta, and Edelta
//! - Similar chunks: Best for data chunks 4KB - 64KB in size
//! - Inter-chunk redundancy: Removes redundancy between similar chunks
//!
//! For maximum compression, combine `GDelta` with a general-purpose compressor
//! like ZSTD or LZ4.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::all)]

mod buffer;
mod delta;
mod error;
mod gear;
mod varint;

pub use error::{GDeltaError, Result};

/// Encodes the delta between new data and base data.
///
/// This function computes a compact representation of the differences between
/// `new_data` and `base_data`. The resulting delta can be later used with
/// [`decode`] to reconstruct the new data.
///
/// # Arguments
///
/// * `new_data` - The target data to encode
/// * `base_data` - The reference data to encode against
///
/// # Returns
///
/// A `Vec<u8>` containing the encoded delta, or a [`GDeltaError`] if encoding fails.
///
/// # Errors
///
/// Currently, encoding does not fail under normal circumstances. The `Result` type
/// is used for API consistency with `decode` and to allow for future validation
/// or error conditions without breaking the API.
///
/// # Examples
///
/// ```
/// use gdelta::encode;
///
/// let base = b"The quick brown fox jumps over the lazy dog";
/// let new = b"The quick brown cat jumps over the lazy dog";
///
/// let delta = encode(new, base).unwrap();
/// println!("Delta size: {} bytes", delta.len());
/// ```
///
/// # Performance
///
/// The encoding time is roughly proportional to the size of the new data,
/// with additional overhead for building the hash table of the base data.
/// Typical throughput is several hundred MB/s on modern hardware.
pub fn encode(new_data: &[u8], base_data: &[u8]) -> Result<Vec<u8>> {
    delta::encode(new_data, base_data)
}

/// Decodes delta data using the base data to reconstruct the original.
///
/// This function applies the delta (created by [`encode`]) to the base data
/// to reconstruct the new data.
///
/// # Arguments
///
/// * `delta` - The encoded delta data
/// * `base_data` - The same base data used during encoding
///
/// # Returns
///
/// A `Vec<u8>` containing the reconstructed data, or a [`GDeltaError`] if
/// decoding fails (e.g., corrupted delta data).
///
/// # Errors
///
/// Returns `GDeltaError::InvalidDelta` if:
/// - The delta data is corrupted or malformed
/// - The instruction length exceeds the delta size
/// - A copy instruction references data beyond the base data bounds
///
/// # Examples
///
/// ```
/// use gdelta::{encode, decode};
///
/// let base = b"Hello, World!";
/// let new = b"Hello, Rust!";
///
/// let delta = encode(new, base).unwrap();
/// let recovered = decode(&delta, base).unwrap();
///
/// assert_eq!(recovered, new);
/// ```
///
/// # Performance
///
/// Decoding is typically faster than encoding, as it only needs to follow
/// the instructions in the delta without performing hash table lookups.
pub fn decode(delta: &[u8], base_data: &[u8]) -> Result<Vec<u8>> {
    delta::decode(delta, base_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_identical() {
        let data = b"Hello, World!";
        let delta = encode(data, data).unwrap();
        let recovered = decode(&delta[..], data).unwrap();
        assert_eq!(recovered, data);
    }

    #[test]
    fn test_encode_decode_different() {
        let base = b"The quick brown fox jumps over the lazy dog";
        let new = b"The quick brown cat jumps over the lazy dog";

        let delta = encode(new, base).unwrap();
        let recovered = decode(&delta[..], base).unwrap();
        assert_eq!(recovered, new);
    }

    #[test]
    fn test_encode_decode_empty() {
        let base = b"Some data";
        let new = b"";

        let delta = encode(new, base).unwrap();
        let recovered = decode(&delta[..], base).unwrap();
        assert_eq!(recovered, new);
    }

    #[test]
    #[allow(clippy::cast_possible_truncation)]
    fn test_encode_decode_large() {
        let mut base = vec![0u8; 100_000];
        let mut new = vec![0u8; 100_000];

        // Fill with pattern
        for i in 0..base.len() {
            base[i] = (i % 256) as u8;
            new[i] = (i % 256) as u8;
        }

        // Make some changes
        for i in (0..new.len()).step_by(488) {
            if i < new.len() {
                new[i] = new[i].wrapping_add(1);
            }
        }

        let delta = encode(&new, &base).unwrap();
        let recovered = decode(&delta, &base).unwrap();
        assert_eq!(recovered, new);

        // Delta should be smaller than new data
        assert!(delta.len() < new.len());
    }
}
