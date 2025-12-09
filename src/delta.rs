//! Core delta encoding and decoding implementation.

use crate::buffer::{BufferStream, INIT_BUFFER_SIZE};
use crate::error::{GDeltaError, Result};
use crate::gear::{WORD_SIZE, build_hash_table, compute_fingerprint, roll_fingerprint};
use crate::varint::{DeltaUnit, read_delta_unit, read_varint, write_delta_unit, write_varint};

/// Minimum length for prefix/suffix optimization.
const MIN_MATCH_LENGTH: usize = 16;

/// Chunk size for processing.
#[allow(dead_code)]
pub const CHUNK_SIZE: usize = 300 * 1024;

/// Encodes the delta between new data and base data.
#[allow(clippy::unnecessary_wraps)]
pub fn encode(new_data: &[u8], base_data: &[u8]) -> Result<Vec<u8>> {
    let new_size = new_data.len();
    let base_size = base_data.len();

    // Find common prefix
    let prefix_len = find_common_prefix(new_data, base_data);
    let has_prefix = prefix_len >= MIN_MATCH_LENGTH;
    let prefix_size = if has_prefix { prefix_len } else { 0 };

    // Find common suffix
    let suffix_len = find_common_suffix(new_data, base_data, prefix_size);
    let mut suffix_size = if suffix_len >= MIN_MATCH_LENGTH {
        suffix_len
    } else {
        0
    };

    // Ensure prefix and suffix don't overlap
    if prefix_size + suffix_size > new_size {
        suffix_size = new_size.saturating_sub(prefix_size);
    }

    // Initialize streams
    let mut instruction_stream = BufferStream::with_capacity(INIT_BUFFER_SIZE);
    let mut data_stream = BufferStream::with_capacity(INIT_BUFFER_SIZE);

    // Handle trivial case where prefix + suffix covers entire base
    if prefix_size + suffix_size >= base_size {
        encode_trivial_case(
            new_data,
            base_data,
            prefix_size,
            suffix_size,
            &mut instruction_stream,
            &mut data_stream,
        );

        return Ok(finalize_delta(&instruction_stream, &data_stream));
    }

    // Write prefix instruction if present
    if has_prefix {
        let unit = DeltaUnit::copy(0, prefix_size as u64);
        write_delta_unit(&mut instruction_stream, &unit);
    }

    // Build hash table for base data
    let work_base_size = base_size - prefix_size - suffix_size;
    let hash_bits = calculate_hash_bits(work_base_size);
    let hash_table = build_hash_table(base_data, prefix_size, base_size - suffix_size, hash_bits);
    let hash_shift = 64 - hash_bits;

    // Encode the middle section
    encode_middle_section(
        new_data,
        base_data,
        prefix_size,
        new_size - suffix_size,
        base_size - suffix_size,
        &hash_table[..],
        hash_shift,
        &mut instruction_stream,
        &mut data_stream,
    );

    // Write suffix instruction if present
    if suffix_size > 0 {
        let unit = DeltaUnit::copy((base_size - suffix_size) as u64, suffix_size as u64);
        write_delta_unit(&mut instruction_stream, &unit);
    }

    Ok(finalize_delta(&instruction_stream, &data_stream))
}

/// Finds the length of the common prefix between two byte slices.
fn find_common_prefix(a: &[u8], b: &[u8]) -> usize {
    let max_len = a.len().min(b.len());
    let mut len = 0;

    #[cfg(feature = "simd")]
    {
        use wide::u8x16;

        // Process 16 bytes at a time with SIMD
        while len + 16 <= max_len {
            let a_chunk = u8x16::new(a[len..len + 16].try_into().unwrap());
            let b_chunk = u8x16::new(b[len..len + 16].try_into().unwrap());

            if a_chunk != b_chunk {
                break;
            }
            len += 16;
        }
    }

    // Compare in 8-byte chunks for remaining data
    while len + 8 <= max_len {
        let a_chunk = u64::from_le_bytes(a[len..len + 8].try_into().unwrap());
        let b_chunk = u64::from_le_bytes(b[len..len + 8].try_into().unwrap());
        if a_chunk != b_chunk {
            break;
        }
        len += 8;
    }

    // Compare remaining bytes
    while len < max_len && a[len] == b[len] {
        len += 1;
    }

    len
}

/// Finds the length of the common suffix between two byte slices.
fn find_common_suffix(a: &[u8], b: &[u8], prefix_len: usize) -> usize {
    let max_len = (a.len() - prefix_len).min(b.len() - prefix_len);
    let mut len = 0;

    #[cfg(feature = "simd")]
    {
        use wide::u8x16;

        // Process 16 bytes at a time with SIMD (from the end)
        while len + 16 <= max_len {
            let a_start = a.len() - len - 16;
            let b_start = b.len() - len - 16;
            let a_chunk = u8x16::new(a[a_start..a_start + 16].try_into().unwrap());
            let b_chunk = u8x16::new(b[b_start..b_start + 16].try_into().unwrap());

            if a_chunk != b_chunk {
                break;
            }
            len += 16;
        }
    }

    // Compare in 8-byte chunks (from the end)
    while len + 8 <= max_len {
        let a_start = a.len() - len - 8;
        let b_start = b.len() - len - 8;
        let a_chunk = u64::from_le_bytes(a[a_start..a_start + 8].try_into().unwrap());
        let b_chunk = u64::from_le_bytes(b[b_start..b_start + 8].try_into().unwrap());
        if a_chunk != b_chunk {
            break;
        }
        len += 8;
    }

    // Compare remaining bytes
    while len < max_len {
        if a[a.len() - len - 1] != b[b.len() - len - 1] {
            break;
        }
        len += 1;
    }

    len
}

/// Calculates the number of hash bits based on data size.
fn calculate_hash_bits(size: usize) -> u32 {
    let mut bits = 0u32;
    let mut temp = size + 10;
    while temp > 0 {
        bits += 1;
        temp >>= 1;
    }
    bits
}

/// Encodes the trivial case where prefix + suffix cover the entire base.
fn encode_trivial_case(
    new_data: &[u8],
    _base_data: &[u8],
    prefix_size: usize,
    suffix_size: usize,
    instruction_stream: &mut BufferStream,
    data_stream: &mut BufferStream,
) {
    let new_size = new_data.len();

    // Write prefix
    if prefix_size > 0 {
        let unit = DeltaUnit::copy(0, prefix_size as u64);
        write_delta_unit(instruction_stream, &unit);
    }

    // Write middle as literal
    let middle_size = new_size - prefix_size - suffix_size;
    if middle_size > 0 {
        let unit = DeltaUnit::literal(middle_size as u64);
        write_delta_unit(instruction_stream, &unit);
        data_stream.write_bytes(&new_data[prefix_size..new_size - suffix_size]);
    }

    // Write suffix
    if suffix_size > 0 {
        let unit = DeltaUnit::copy((new_size - suffix_size) as u64, suffix_size as u64);
        write_delta_unit(instruction_stream, &unit);
    }
}

/// Encodes the middle section of the data using hash table lookups.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::cast_possible_truncation)]
fn encode_middle_section(
    new_data: &[u8],
    base_data: &[u8],
    start: usize,
    end: usize,
    base_end: usize,
    hash_table: &[u32],
    hash_shift: u32,
    instruction_stream: &mut BufferStream,
    data_stream: &mut BufferStream,
) {
    if start >= end || end - start < WORD_SIZE {
        // Write remaining data as literal
        if start < end {
            let unit = DeltaUnit::literal((end - start) as u64);
            write_delta_unit(instruction_stream, &unit);
            data_stream.write_bytes(&new_data[start..end]);
        }
        return;
    }

    let mut pos = start;
    let mut literal_start = start;
    let mut fingerprint = compute_fingerprint(new_data, pos);

    while pos + WORD_SIZE <= end {
        // Look up in hash table
        let hash_index = (fingerprint >> hash_shift) as usize;
        let base_offset = hash_table[hash_index] as usize;

        // Check if we have a match
        if base_offset > 0
            && base_offset + WORD_SIZE <= base_end
            && new_data[pos..pos + WORD_SIZE] == base_data[base_offset..base_offset + WORD_SIZE]
        {
            // Found a match, extend it
            let match_len = extend_match(new_data, base_data, pos, base_offset, end, base_end);

            // Write pending literal if any
            if pos > literal_start {
                let lit_len = pos - literal_start;
                let unit = DeltaUnit::literal(lit_len as u64);
                write_delta_unit(instruction_stream, &unit);
                data_stream.write_bytes(&new_data[literal_start..pos]);
            }

            // Write copy instruction
            let unit = DeltaUnit::copy(base_offset as u64, match_len as u64);
            write_delta_unit(instruction_stream, &unit);

            // Advance position
            pos += match_len;
            literal_start = pos;

            // Recompute fingerprint
            if pos + WORD_SIZE <= end {
                fingerprint = compute_fingerprint(new_data, pos);
            }
            continue;
        }

        // No match, advance by one byte
        pos += 1;
        if pos + WORD_SIZE <= end {
            fingerprint = roll_fingerprint(fingerprint, new_data[pos + WORD_SIZE - 1]);
        }
    }

    // Write final literal if any
    if literal_start < end {
        let lit_len = end - literal_start;
        let unit = DeltaUnit::literal(lit_len as u64);
        write_delta_unit(instruction_stream, &unit);
        data_stream.write_bytes(&new_data[literal_start..end]);
    }
}

/// Extends a match as far as possible.
fn extend_match(
    new_data: &[u8],
    base_data: &[u8],
    new_pos: usize,
    base_pos: usize,
    new_end: usize,
    base_end: usize,
) -> usize {
    let mut len = WORD_SIZE;

    #[cfg(feature = "simd")]
    {
        use wide::u8x16;

        // Extend in 16-byte chunks with SIMD
        while new_pos + len + 16 <= new_end && base_pos + len + 16 <= base_end {
            let new_chunk = u8x16::new(
                new_data[new_pos + len..new_pos + len + 16]
                    .try_into()
                    .unwrap(),
            );
            let base_chunk = u8x16::new(
                base_data[base_pos + len..base_pos + len + 16]
                    .try_into()
                    .unwrap(),
            );

            if new_chunk != base_chunk {
                break;
            }
            len += 16;
        }
    }

    // Extend in 8-byte chunks
    while new_pos + len + 8 <= new_end && base_pos + len + 8 <= base_end {
        let new_chunk = u64::from_le_bytes(
            new_data[new_pos + len..new_pos + len + 8]
                .try_into()
                .unwrap(),
        );
        let base_chunk = u64::from_le_bytes(
            base_data[base_pos + len..base_pos + len + 8]
                .try_into()
                .unwrap(),
        );
        if new_chunk != base_chunk {
            break;
        }
        len += 8;
    }

    // Extend byte by byte
    while new_pos + len < new_end
        && base_pos + len < base_end
        && new_data[new_pos + len] == base_data[base_pos + len]
    {
        len += 1;
    }

    len
}

/// Finalizes the delta by combining instruction and data streams.
fn finalize_delta(instruction_stream: &BufferStream, data_stream: &BufferStream) -> Vec<u8> {
    let mut result = BufferStream::with_capacity(instruction_stream.len() + data_stream.len() + 10);

    // Write instruction length as varint
    write_varint(&mut result, instruction_stream.len() as u64);

    // Write instructions
    result.write_bytes(instruction_stream.as_slice());

    // Write data
    result.write_bytes(data_stream.as_slice());

    result.into_vec()
}

/// Decodes delta data using the base data.
#[allow(clippy::cast_possible_truncation)]
pub fn decode(delta: &[u8], base_data: &[u8]) -> Result<Vec<u8>> {
    let mut delta_stream = BufferStream::from_slice(delta);

    // Read instruction length
    let instruction_len = read_varint(&mut delta_stream)? as usize;
    let inst_start = delta_stream.position();
    let inst_end = inst_start + instruction_len;

    if inst_end > delta.len() {
        return Err(GDeltaError::InvalidDelta(
            "Instruction length exceeds delta size".to_string(),
        ));
    }

    // Position data stream after instructions
    let data_start = inst_end;
    let mut data_stream = BufferStream::from_slice(&delta[data_start..]);

    // Output buffer
    let mut output = BufferStream::with_capacity(INIT_BUFFER_SIZE);
    let base_stream = BufferStream::from_slice(base_data);

    // Process instructions
    while delta_stream.position() < inst_end {
        let unit = read_delta_unit(&mut delta_stream)?;

        if unit.is_copy {
            // Copy from base data
            let offset = unit.offset as usize;
            let length = unit.length as usize;

            if offset + length > base_data.len() {
                return Err(GDeltaError::InvalidDelta(format!(
                    "Copy offset {} + length {} exceeds base size {}",
                    offset,
                    length,
                    base_data.len()
                )));
            }

            output.copy_from(&base_stream, offset, length)?;
        } else {
            // Copy literal data
            let length = unit.length as usize;
            output.append_from_cursor(&mut data_stream, length)?;
        }
    }

    Ok(output.into_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_common_prefix() {
        let a = b"Hello, World!";
        let b = b"Hello, Rust!";
        assert_eq!(find_common_prefix(a, b), 7);
    }

    #[test]
    fn test_find_common_suffix() {
        let a = b"Hello, World!";
        let b = b"Howdy, World!";
        // Common suffix is ", World!" which is 8 characters
        assert_eq!(find_common_suffix(a, b, 0), 8);
    }

    #[test]
    fn test_encode_decode_simple() {
        let base = b"The quick brown fox jumps over the lazy dog";
        let new = b"The quick brown cat jumps over the lazy dog";

        let delta = encode(new, base).unwrap();
        let decoded = decode(&delta[..], base).unwrap();

        assert_eq!(decoded, new);
    }

    #[test]
    fn test_encode_decode_identical() {
        let data = b"Same data on both sides";

        let delta = encode(data, data).unwrap();
        let decoded = decode(&delta[..], data).unwrap();

        assert_eq!(decoded, data);
        // Delta should be very small for identical data
        assert!(delta.len() < 20);
    }

    #[test]
    fn test_encode_decode_empty() {
        let base = b"Some base data";
        let new = b"";

        let delta = encode(new, base).unwrap();
        let decoded = decode(&delta[..], base).unwrap();

        assert_eq!(decoded, new);
    }
}
