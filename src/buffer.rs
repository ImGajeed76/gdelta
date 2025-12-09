//! Buffer management utilities for reading and writing data streams.

use crate::error::{GDeltaError, Result};

/// Initial buffer size for allocations.
pub const INIT_BUFFER_SIZE: usize = 128 * 1024;

/// A buffer with a cursor for sequential reading or writing.
pub struct BufferStream {
    buffer: Vec<u8>,
    cursor: usize,
}

impl BufferStream {
    /// Creates a new buffer with the specified initial capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
            cursor: 0,
        }
    }

    /// Creates a new buffer wrapping existing data.
    #[allow(dead_code)]
    pub fn from_vec(buffer: Vec<u8>) -> Self {
        Self { buffer, cursor: 0 }
    }

    /// Creates a new buffer from a slice, positioned at the start.
    pub fn from_slice(data: &[u8]) -> Self {
        Self {
            buffer: data.to_vec(),
            cursor: 0,
        }
    }

    /// Returns the current cursor position.
    #[inline]
    pub fn position(&self) -> usize {
        self.cursor
    }

    /// Sets the cursor position.
    #[inline]
    #[allow(dead_code)]
    pub fn set_position(&mut self, pos: usize) {
        self.cursor = pos;
    }

    /// Returns a reference to the underlying buffer.
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.buffer[..]
    }

    /// Consumes the buffer and returns the underlying vector.
    #[inline]
    pub fn into_vec(self) -> Vec<u8> {
        self.buffer
    }

    /// Returns the total length of the buffer.
    #[inline]
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Returns true if the buffer is empty.
    #[inline]
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Returns the number of bytes remaining from the cursor to the end.
    #[inline]
    #[allow(dead_code)]
    pub fn remaining(&self) -> usize {
        self.buffer.len().saturating_sub(self.cursor)
    }

    /// Writes a single byte to the buffer.
    pub fn write_u8(&mut self, value: u8) {
        self.buffer.push(value);
        self.cursor += 1;
    }

    /// Writes a slice of bytes to the buffer.
    pub fn write_bytes(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
        self.cursor += data.len();
    }

    /// Reads a single byte from the buffer.
    pub fn read_u8(&mut self) -> Result<u8> {
        if self.cursor >= self.buffer.len() {
            return Err(GDeltaError::UnexpectedEndOfData);
        }
        let value = self.buffer[self.cursor];
        self.cursor += 1;
        Ok(value)
    }

    /// Reads a slice of bytes from the buffer.
    pub fn read_bytes(&mut self, len: usize) -> Result<&[u8]> {
        if self.cursor + len > self.buffer.len() {
            return Err(GDeltaError::UnexpectedEndOfData);
        }
        let start = self.cursor;
        self.cursor += len;
        Ok(&self.buffer[start..self.cursor])
    }

    /// Reads bytes from a specific position without moving the cursor.
    pub fn peek_at(&self, position: usize, len: usize) -> Result<&[u8]> {
        if position + len > self.buffer.len() {
            return Err(GDeltaError::UnexpectedEndOfData);
        }
        Ok(&self.buffer[position..position + len])
    }

    /// Copies bytes from another buffer at a specific position.
    pub fn copy_from(&mut self, other: &BufferStream, position: usize, len: usize) -> Result<()> {
        let data = other.peek_at(position, len)?;
        self.write_bytes(data);
        Ok(())
    }

    /// Appends the contents of another buffer from its current cursor position.
    pub fn append_from_cursor(&mut self, other: &mut BufferStream, len: usize) -> Result<()> {
        let data = other.read_bytes(len)?;
        self.write_bytes(data);
        Ok(())
    }

    /// Reserves capacity for at least `additional` more bytes.
    #[allow(dead_code)]
    pub fn reserve(&mut self, additional: usize) {
        self.buffer.reserve(additional);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_read_write() {
        let mut buf = BufferStream::with_capacity(10);

        buf.write_u8(42);
        buf.write_bytes(&[1, 2, 3]);

        assert_eq!(buf.len(), 4);
        assert_eq!(buf.position(), 4);

        buf.set_position(0);

        assert_eq!(buf.read_u8().unwrap(), 42);
        assert_eq!(buf.read_bytes(3).unwrap(), &[1, 2, 3]);
    }

    #[test]
    fn test_buffer_underflow() {
        let mut buf = BufferStream::from_slice(&[1, 2, 3]);

        assert_eq!(buf.read_u8().unwrap(), 1);
        assert_eq!(buf.read_bytes(2).unwrap(), &[2, 3]);
        assert!(buf.read_u8().is_err());
    }
}
