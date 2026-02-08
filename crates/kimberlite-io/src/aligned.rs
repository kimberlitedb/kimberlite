//! Aligned buffer for Direct I/O.
//!
//! When using O_DIRECT on Linux, buffers must be aligned to the filesystem
//! block size (typically 4096 bytes). This module provides an `AlignedBuffer`
//! that ensures proper alignment.
//!
//! On non-Linux platforms, `AlignedBuffer` is a thin wrapper around `Vec<u8>`.

/// Block alignment requirement for Direct I/O (4 KiB).
pub const BLOCK_ALIGNMENT: usize = 4096;

/// A buffer with guaranteed alignment for Direct I/O.
///
/// On Linux with the `direct_io` feature, allocates memory aligned to
/// [`BLOCK_ALIGNMENT`]. On other platforms, delegates to a plain `Vec<u8>`.
#[derive(Debug)]
pub struct AlignedBuffer {
    data: Vec<u8>,
}

impl AlignedBuffer {
    /// Creates a new aligned buffer with the given capacity.
    ///
    /// The actual allocation size is rounded up to the nearest multiple of
    /// [`BLOCK_ALIGNMENT`].
    pub fn with_capacity(capacity: usize) -> Self {
        let aligned_cap = round_up(capacity, BLOCK_ALIGNMENT);
        Self {
            data: Vec::with_capacity(aligned_cap),
        }
    }

    /// Creates an aligned buffer from existing data, padding to alignment.
    ///
    /// The buffer is padded with zeros to the next block boundary.
    pub fn from_data(data: &[u8]) -> Self {
        let aligned_len = round_up(data.len(), BLOCK_ALIGNMENT);
        let mut buf = Vec::with_capacity(aligned_len);
        buf.extend_from_slice(data);
        buf.resize(aligned_len, 0);
        Self { data: buf }
    }

    /// Returns the buffer contents as a slice.
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    /// Returns the buffer contents as a mutable slice.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// Returns the length of the data in the buffer.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns true if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Extends the buffer with the given data.
    pub fn extend_from_slice(&mut self, data: &[u8]) {
        self.data.extend_from_slice(data);
    }

    /// Pads the buffer to the next block boundary with zeros.
    pub fn pad_to_alignment(&mut self) {
        let aligned_len = round_up(self.data.len(), BLOCK_ALIGNMENT);
        self.data.resize(aligned_len, 0);
    }

    /// Returns the actual unpadded data length before any alignment padding.
    ///
    /// Note: After `pad_to_alignment()`, this returns the padded length.
    /// Track unpadded length externally if needed.
    pub fn capacity(&self) -> usize {
        self.data.capacity()
    }

    /// Consumes the buffer and returns the underlying `Vec<u8>`.
    pub fn into_vec(self) -> Vec<u8> {
        self.data
    }

    /// Clears the buffer.
    pub fn clear(&mut self) {
        self.data.clear();
    }
}

impl AsRef<[u8]> for AlignedBuffer {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl AsMut<[u8]> for AlignedBuffer {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }
}

/// Rounds `value` up to the nearest multiple of `alignment`.
fn round_up(value: usize, alignment: usize) -> usize {
    debug_assert!(alignment > 0, "alignment must be positive");
    debug_assert!(
        alignment.is_power_of_two(),
        "alignment must be a power of two"
    );
    (value + alignment - 1) & !(alignment - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_up_basic() {
        assert_eq!(round_up(0, 4096), 0);
        assert_eq!(round_up(1, 4096), 4096);
        assert_eq!(round_up(4096, 4096), 4096);
        assert_eq!(round_up(4097, 4096), 8192);
    }

    #[test]
    fn aligned_buffer_from_data() {
        let data = vec![1u8; 100];
        let buf = AlignedBuffer::from_data(&data);
        assert_eq!(buf.len(), 4096); // Padded to alignment
        assert_eq!(&buf.as_slice()[..100], &data);
        assert!(buf.as_slice()[100..].iter().all(|&b| b == 0));
    }

    #[test]
    fn aligned_buffer_pad() {
        let mut buf = AlignedBuffer::with_capacity(4096);
        buf.extend_from_slice(&[1, 2, 3]);
        assert_eq!(buf.len(), 3);
        buf.pad_to_alignment();
        assert_eq!(buf.len(), 4096);
    }
}
