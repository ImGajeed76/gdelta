//! Variable-length integer encoding for efficient space utilization.
//!
//! This module implements variable-length integer encoding where each byte
//! stores 7 bits of the value and 1 bit indicating if more bytes follow.

use crate::buffer::BufferStream;
use crate::error::Result;

/// Number of value bits per byte in varint encoding.
const VARINT_BITS: u8 = 7;

/// Mask for extracting varint value bits.
const VARINT_MASK: u64 = (1 << VARINT_BITS) - 1;

/// Number of value bits in the head byte of a delta unit.
const HEAD_VARINT_BITS: u8 = 6;

/// Mask for extracting head varint value bits.
const HEAD_VARINT_MASK: u64 = (1 << HEAD_VARINT_BITS) - 1;

/// Writes a variable-length integer to the buffer.
///
/// The integer is encoded as a sequence of bytes, where each byte stores
/// 7 bits of the value. The high bit of each byte indicates whether more
/// bytes follow (1) or if this is the last byte (0).
pub fn write_varint(buffer: &mut BufferStream, value: u64) {
    // Fast paths for common small values (no loop)
    if value < 128 {
        buffer.write_u8(value as u8);
        return;
    }

    if value < 16384 {
        // 2 bytes
        buffer.write_u8(((value & 0x7F) | 0x80) as u8);
        buffer.write_u8((value >> 7) as u8);
        return;
    }

    // General case
    let mut val = value;
    loop {
        let byte_val = (val & VARINT_MASK) as u8;
        val >>= VARINT_BITS;
        if val == 0 {
            buffer.write_u8(byte_val);
            break;
        }
        buffer.write_u8(byte_val | 0x80);
    }
}

/// Reads a variable-length integer from the buffer.
#[allow(clippy::cast_lossless)]
pub fn read_varint(buffer: &mut BufferStream) -> Result<u64> {
    let byte = buffer.read_u8()?;

    // Fast path: single byte
    if (byte & 0x80) == 0 {
        return Ok(byte as u64);
    }

    // Two bytes
    let byte2 = buffer.read_u8()?;
    if (byte2 & 0x80) == 0 {
        return Ok(((byte & 0x7F) as u64) | ((byte2 as u64) << 7));
    }

    // General case
    let mut value = ((byte & 0x7F) as u64) | (((byte2 & 0x7F) as u64) << 7);
    let mut shift = 14u8;

    loop {
        let byte = buffer.read_u8()?;
        let more = (byte & 0x80) != 0;
        value |= ((byte & 0x7F) as u64) << shift;
        shift += 7;
        if !more {
            break;
        }
    }

    Ok(value)
}

/// A delta instruction unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeltaUnit {
    /// If true, this is a copy instruction; if false, it's a literal.
    pub is_copy: bool,
    /// Length of the data to copy or literal data length.
    pub length: u64,
    /// For copy instructions, the offset in the base data.
    pub offset: u64,
}

impl DeltaUnit {
    /// Creates a new copy instruction.
    pub fn copy(offset: u64, length: u64) -> Self {
        Self {
            is_copy: true,
            length,
            offset,
        }
    }

    /// Creates a new literal instruction.
    pub fn literal(length: u64) -> Self {
        Self {
            is_copy: false,
            length,
            offset: 0,
        }
    }
}

/// Writes a delta unit to the buffer.
///
/// Format:
/// - Head byte: [flag:1][more:1][length:6]
/// - Optional varint: remaining length bits (if more=1)
/// - Optional varint: offset (if flag=1)
#[allow(clippy::cast_lossless)]
pub fn write_delta_unit(buffer: &mut BufferStream, unit: &DeltaUnit) {
    let flag = (unit.is_copy) as u8;
    let head_length = (unit.length & HEAD_VARINT_MASK) as u8;
    let remaining_length = unit.length >> HEAD_VARINT_BITS;
    let more = (remaining_length > 0) as u8;

    // Write head byte: [flag:1][more:1][length:6]
    let head_byte = (flag << 7) | (more << 6) | head_length;
    buffer.write_u8(head_byte);

    // Write remaining length if needed
    if remaining_length > 0 {
        write_varint(buffer, remaining_length);
    }

    // Write offset for copy instructions
    if unit.is_copy {
        write_varint(buffer, unit.offset);
    }
}

/// Reads a delta unit from the buffer.
#[allow(clippy::cast_lossless)]
pub fn read_delta_unit(buffer: &mut BufferStream) -> Result<DeltaUnit> {
    let head_byte = buffer.read_u8()?;

    let is_copy = (head_byte & 0x80) != 0;
    let more = (head_byte & 0x40) != 0;
    let mut length = (head_byte & 0x3F) as u64;

    if more {
        let remaining = read_varint(buffer)?;
        length |= remaining << HEAD_VARINT_BITS;
    }

    let offset = if is_copy { read_varint(buffer)? } else { 0 };

    Ok(DeltaUnit {
        is_copy,
        length,
        offset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_varint_encoding() {
        let mut buffer = BufferStream::with_capacity(10);

        write_varint(&mut buffer, 127);
        write_varint(&mut buffer, 128);
        write_varint(&mut buffer, 16383);

        buffer.set_position(0);

        assert_eq!(read_varint(&mut buffer).unwrap(), 127);
        assert_eq!(read_varint(&mut buffer).unwrap(), 128);
        assert_eq!(read_varint(&mut buffer).unwrap(), 16383);
    }

    #[test]
    fn test_delta_unit_copy() {
        let mut buffer = BufferStream::with_capacity(20);

        let unit = DeltaUnit::copy(1000, 500);
        write_delta_unit(&mut buffer, &unit);

        buffer.set_position(0);

        let decoded = read_delta_unit(&mut buffer).unwrap();
        assert_eq!(decoded, unit);
    }

    #[test]
    fn test_delta_unit_literal() {
        let mut buffer = BufferStream::with_capacity(20);

        let unit = DeltaUnit::literal(250);
        write_delta_unit(&mut buffer, &unit);

        buffer.set_position(0);

        let decoded = read_delta_unit(&mut buffer).unwrap();
        assert_eq!(decoded, unit);
    }

    #[test]
    fn test_delta_unit_large_length() {
        let mut buffer = BufferStream::with_capacity(20);

        let unit = DeltaUnit::literal(100_000);
        write_delta_unit(&mut buffer, &unit);

        buffer.set_position(0);

        let decoded = read_delta_unit(&mut buffer).unwrap();
        assert_eq!(decoded, unit);
    }
}
