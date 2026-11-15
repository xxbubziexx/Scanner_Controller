//! Exact, sample-accurate circular buffer for pre-roll transmission capture.
//!
//! Port of `PreRollRingBuffer` from `audio/recorder.py:54-96`.
//! Guarantees `capacity_samples` of chronological audio with zero chunk-rounding
//! or off-by-one errors across wraps.

/// Sample-accurate circular buffer storing raw 16-bit signed PCM samples.
#[derive(Debug, Clone)]
pub struct PreRollRingBuffer {
    capacity: usize,
    buffer: Vec<i16>,
    write_idx: usize,
    count: usize,
}

impl PreRollRingBuffer {
    /// Creates a new `PreRollRingBuffer` capable of holding `capacity_samples` `i16` samples.
    #[must_use]
    pub fn new(capacity_samples: usize) -> Self {
        let capacity = capacity_samples.max(1);
        Self {
            capacity,
            buffer: vec![0_i16; capacity],
            write_idx: 0,
            count: 0,
        }
    }

    /// Returns the total sample capacity of the ring buffer.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the number of valid samples currently stored in the ring buffer (`0..=capacity`).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Returns `true` if no samples have been written yet.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Clears all buffered samples without deallocating storage.
    pub fn clear(&mut self) {
        self.write_idx = 0;
        self.count = 0;
    }

    /// Appends raw `i16` PCM samples to the circular buffer.
    pub fn write(&mut self, samples: &[i16]) {
        let n = samples.len();
        if n == 0 {
            return;
        }
        if n >= self.capacity {
            let start = n - self.capacity;
            self.buffer.copy_from_slice(&samples[start..]);
            self.write_idx = 0;
            self.count = self.capacity;
            return;
        }

        let end = self.write_idx + n;
        if end <= self.capacity {
            self.buffer[self.write_idx..end].copy_from_slice(samples);
        } else {
            let first = self.capacity - self.write_idx;
            self.buffer[self.write_idx..].copy_from_slice(&samples[..first]);
            let rem = end % self.capacity;
            self.buffer[..rem].copy_from_slice(&samples[first..]);
        }
        self.write_idx = end % self.capacity;
        self.count = self.capacity.min(self.count + n);
    }

    /// Returns chronological `i16` PCM samples up to `capacity`.
    #[must_use]
    pub fn get_preroll_samples(&self) -> Vec<i16> {
        if self.count == 0 {
            return Vec::new();
        }
        if self.count < self.capacity {
            return self.buffer[..self.count].to_vec();
        }
        let mut out = Vec::with_capacity(self.capacity);
        out.extend_from_slice(&self.buffer[self.write_idx..]);
        out.extend_from_slice(&self.buffer[..self.write_idx]);
        out
    }

    /// Returns exact chronological little-endian `i16` PCM bytes up to `capacity * 2`.
    #[must_use]
    pub fn get_preroll_bytes(&self) -> Vec<u8> {
        if self.count == 0 {
            return Vec::new();
        }
        let mut bytes = Vec::with_capacity(self.count * 2);
        if self.count < self.capacity {
            for &sample in &self.buffer[..self.count] {
                bytes.extend_from_slice(&sample.to_le_bytes());
            }
        } else {
            for &sample in &self.buffer[self.write_idx..] {
                bytes.extend_from_slice(&sample.to_le_bytes());
            }
            for &sample in &self.buffer[..self.write_idx] {
                bytes.extend_from_slice(&sample.to_le_bytes());
            }
        }
        bytes
    }
}
