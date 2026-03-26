//! Sparse bitmap implementation for efficient set operations.
//!
//! This is a Rust port of sparsemap.c by Gregory Burd.
//! Original: https://codeberg.org/gregburd/sparsemap
//!
//! A sparse bitmap compresses runs of zeros and ones, storing only
//! "mixed" bitvectors explicitly. This provides:
//! - O(1) set, clear, test operations (amortized)
//! - O(n) union, intersection, difference operations where n = number of set bits
//! - Compact memory representation for sparse sets
//!
//! # Compression Strategy
//!
//! Each chunk manages up to 32 bitvectors (2048 bits). Each bitvector
//! uses a 2-bit flag:
//! - `00`: all zeros (not stored)
//! - `11`: all ones (not stored)
//! - `10`: mixed (stored explicitly)
//! - `01`: unused (for capacity management)
//!
//! # Example
//!
//! ```
//! use sparsemap::SparseMap;
//!
//! let mut map = SparseMap::new();
//! map.set(42);
//! map.set(1000);
//! map.set(1_000_000);
//!
//! assert!(map.is_set(42));
//! assert!(!map.is_set(43));
//! assert_eq!(map.count(), 3);
//! ```

use std::fmt;

/// Type for bit indices (supports up to 4 billion bits).
pub type Idx = u32;

/// Type for bitvectors (64-bit chunks).
type BitVec = u64;

/// Bits per bitvector.
const BITS_PER_VEC: usize = 64;

/// Flags per index byte (4 * 2-bit flags = 8 bits).
const FLAGS_PER_BYTE: usize = 4;

/// Number of bitvectors per chunk (32 * 64 = 2048 bits).
const VECS_PER_CHUNK: usize = 32;

/// Maximum bits per chunk.
const BITS_PER_CHUNK: usize = VECS_PER_CHUNK * BITS_PER_VEC;

/// 2-bit flag values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Flag {
    Zeros = 0b00,  // All zeros, not stored
    Ones = 0b11,   // All ones, not stored
    Mixed = 0b10,  // Mixed, stored explicitly
    Unused = 0b01, // Unused slot
}

impl Flag {
    fn from_bits(bits: u8) -> Self {
        match bits & 0b11 {
            0b00 => Flag::Zeros,
            0b11 => Flag::Ones,
            0b10 => Flag::Mixed,
            0b01 => Flag::Unused,
            _ => unreachable!(),
        }
    }
}

/// A compressed chunk managing up to 2048 bits.
struct Chunk {
    /// Index byte containing 2-bit flags for first 4 bitvectors.
    /// Remaining flags stored in subsequent bytes.
    flags: [u8; VECS_PER_CHUNK / FLAGS_PER_BYTE],

    /// Explicitly stored bitvectors (only for Mixed flags).
    vectors: Vec<BitVec>,
}

impl Chunk {
    fn new() -> Self {
        Self {
            flags: [0; VECS_PER_CHUNK / FLAGS_PER_BYTE],
            vectors: Vec::new(),
        }
    }

    /// Get the flag for bitvector `idx` (0-31).
    #[inline]
    fn get_flag(&self, idx: usize) -> Flag {
        debug_assert!(idx < VECS_PER_CHUNK);
        let byte_idx = idx / FLAGS_PER_BYTE;
        let bit_offset = (idx % FLAGS_PER_BYTE) * 2;
        let bits = (self.flags[byte_idx] >> bit_offset) & 0b11;
        Flag::from_bits(bits)
    }

    /// Set the flag for bitvector `idx`.
    #[inline]
    fn set_flag(&mut self, idx: usize, flag: Flag) {
        debug_assert!(idx < VECS_PER_CHUNK);
        let byte_idx = idx / FLAGS_PER_BYTE;
        let bit_offset = (idx % FLAGS_PER_BYTE) * 2;
        let mask = !(0b11 << bit_offset);
        self.flags[byte_idx] = (self.flags[byte_idx] & mask) | ((flag as u8) << bit_offset);
    }

    /// Get the position in `vectors` for bitvector `idx`.
    #[inline]
    fn vector_position(&self, idx: usize) -> usize {
        let mut pos = 0;
        for i in 0..idx {
            if self.get_flag(i) == Flag::Mixed {
                pos += 1;
            }
        }
        pos
    }

    /// Get the bitvector at `idx`, handling compression.
    fn get_vector(&self, idx: usize) -> BitVec {
        match self.get_flag(idx) {
            Flag::Zeros => 0,
            Flag::Ones => !0,
            Flag::Mixed => self.vectors[self.vector_position(idx)],
            Flag::Unused => 0,
        }
    }

    /// Set the bitvector at `idx`, updating compression state.
    fn set_vector(&mut self, idx: usize, vec: BitVec) {
        let flag = self.get_flag(idx);

        // Determine new flag based on vector content
        let new_flag = if vec == 0 {
            Flag::Zeros
        } else if vec == !0 {
            Flag::Ones
        } else {
            Flag::Mixed
        };

        if flag == new_flag {
            // No compression state change, just update if mixed
            if new_flag == Flag::Mixed {
                let pos = self.vector_position(idx);
                self.vectors[pos] = vec;
            }
        } else if new_flag == Flag::Mixed {
            // Transitioning to mixed: insert vector
            let pos = self.vector_position(idx);
            self.vectors.insert(pos, vec);
            self.set_flag(idx, Flag::Mixed);
        } else if flag == Flag::Mixed {
            // Transitioning from mixed: remove vector
            let pos = self.vector_position(idx);
            self.vectors.remove(pos);
            self.set_flag(idx, new_flag);
        } else {
            // Both compressed, just update flag
            self.set_flag(idx, new_flag);
        }
    }

    /// Set bit `bit_idx` (0-2047) in this chunk.
    fn set_bit(&mut self, bit_idx: usize) {
        debug_assert!(bit_idx < BITS_PER_CHUNK);
        let vec_idx = bit_idx / BITS_PER_VEC;
        let bit_offset = bit_idx % BITS_PER_VEC;
        let mut vec = self.get_vector(vec_idx);
        vec |= 1 << bit_offset;
        self.set_vector(vec_idx, vec);
    }

    /// Clear bit `bit_idx` in this chunk.
    fn clear_bit(&mut self, bit_idx: usize) {
        debug_assert!(bit_idx < BITS_PER_CHUNK);
        let vec_idx = bit_idx / BITS_PER_VEC;
        let bit_offset = bit_idx % BITS_PER_VEC;
        let mut vec = self.get_vector(vec_idx);
        vec &= !(1 << bit_offset);
        self.set_vector(vec_idx, vec);
    }

    /// Test if bit `bit_idx` is set.
    fn is_set(&self, bit_idx: usize) -> bool {
        debug_assert!(bit_idx < BITS_PER_CHUNK);
        let vec_idx = bit_idx / BITS_PER_VEC;
        let bit_offset = bit_idx % BITS_PER_VEC;
        let vec = self.get_vector(vec_idx);
        (vec & (1 << bit_offset)) != 0
    }

    /// Count set bits in this chunk.
    fn count(&self) -> u32 {
        let mut count = 0;
        for i in 0..VECS_PER_CHUNK {
            match self.get_flag(i) {
                Flag::Zeros | Flag::Unused => {}
                Flag::Ones => count += BITS_PER_VEC as u32,
                Flag::Mixed => {
                    let vec = self.get_vector(i);
                    count += vec.count_ones();
                }
            }
        }
        count
    }
}

/// A sparse bitmap supporting up to 4 billion bits.
pub struct SparseMap {
    /// Chunks indexed by (bit_index / BITS_PER_CHUNK).
    /// Only allocated chunks are stored.
    chunks: Vec<Option<Box<Chunk>>>,
}

impl SparseMap {
    /// Create a new empty sparse map.
    #[must_use]
    pub fn new() -> Self {
        Self {
            chunks: Vec::new(),
        }
    }

    /// Set bit `idx`.
    pub fn set(&mut self, idx: Idx) {
        let chunk_idx = (idx as usize) / BITS_PER_CHUNK;
        let bit_idx = (idx as usize) % BITS_PER_CHUNK;

        // Grow chunks vector if needed
        if chunk_idx >= self.chunks.len() {
            self.chunks.resize_with(chunk_idx + 1, || None);
        }

        // Allocate chunk if needed
        if self.chunks[chunk_idx].is_none() {
            self.chunks[chunk_idx] = Some(Box::new(Chunk::new()));
        }

        self.chunks[chunk_idx].as_mut().unwrap().set_bit(bit_idx);
    }

    /// Clear bit `idx`.
    pub fn clear(&mut self, idx: Idx) {
        let chunk_idx = (idx as usize) / BITS_PER_CHUNK;
        let bit_idx = (idx as usize) % BITS_PER_CHUNK;

        if let Some(Some(chunk)) = self.chunks.get_mut(chunk_idx) {
            chunk.clear_bit(bit_idx);
        }
        // If chunk doesn't exist, bit is already clear
    }

    /// Test if bit `idx` is set.
    #[must_use]
    pub fn is_set(&self, idx: Idx) -> bool {
        let chunk_idx = (idx as usize) / BITS_PER_CHUNK;
        let bit_idx = (idx as usize) % BITS_PER_CHUNK;

        self.chunks
            .get(chunk_idx)
            .and_then(|c| c.as_ref())
            .is_some_and(|chunk| chunk.is_set(bit_idx))
    }

    /// Count total set bits.
    #[must_use]
    pub fn count(&self) -> u32 {
        self.chunks
            .iter()
            .filter_map(|c| c.as_ref())
            .map(|chunk| chunk.count())
            .sum()
    }

    /// Union this map with `other` (self |= other).
    pub fn union(&mut self, other: &Self) {
        for (idx, other_chunk) in other.chunks.iter().enumerate() {
            if let Some(other_chunk) = other_chunk {
                // Ensure we have space
                if idx >= self.chunks.len() {
                    self.chunks.resize_with(idx + 1, || None);
                }

                // Allocate our chunk if needed
                if self.chunks[idx].is_none() {
                    self.chunks[idx] = Some(Box::new(Chunk::new()));
                }

                let self_chunk = self.chunks[idx].as_mut().unwrap();

                // Union each bitvector
                for vec_idx in 0..VECS_PER_CHUNK {
                    let self_vec = self_chunk.get_vector(vec_idx);
                    let other_vec = other_chunk.get_vector(vec_idx);
                    self_chunk.set_vector(vec_idx, self_vec | other_vec);
                }
            }
        }
    }

    /// Intersect this map with `other` (self &= other).
    pub fn intersect(&mut self, other: &Self) {
        for (idx, self_chunk) in self.chunks.iter_mut().enumerate() {
            if let Some(self_chunk) = self_chunk {
                if let Some(Some(other_chunk)) = other.chunks.get(idx) {
                    // Both have chunks, intersect
                    for vec_idx in 0..VECS_PER_CHUNK {
                        let self_vec = self_chunk.get_vector(vec_idx);
                        let other_vec = other_chunk.get_vector(vec_idx);
                        self_chunk.set_vector(vec_idx, self_vec & other_vec);
                    }
                } else {
                    // Other doesn't have this chunk, result is all zeros
                    **self_chunk = Chunk::new();
                }
            }
        }
    }

    /// Iterate over all set bit indices.
    pub fn iter(&self) -> impl Iterator<Item = Idx> + '_ {
        self.chunks
            .iter()
            .enumerate()
            .filter_map(|(chunk_idx, chunk)| {
                chunk.as_ref().map(|c| (chunk_idx, c.as_ref()))
            })
            .flat_map(|(chunk_idx, chunk)| {
                (0..VECS_PER_CHUNK).flat_map(move |vec_idx| {
                    let vec = chunk.get_vector(vec_idx);
                    (0..BITS_PER_VEC).filter_map(move |bit_offset| {
                        if (vec & (1 << bit_offset)) != 0 {
                            let idx = (chunk_idx * BITS_PER_CHUNK)
                                + (vec_idx * BITS_PER_VEC)
                                + bit_offset;
                            Some(idx as Idx)
                        } else {
                            None
                        }
                    })
                })
            })
    }
}

impl Default for SparseMap {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for SparseMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SparseMap {{ count: {} }}", self.count())
    }
}

impl Clone for SparseMap {
    fn clone(&self) -> Self {
        Self {
            chunks: self.chunks.iter().map(|c| c.as_ref().map(|chunk| {
                Box::new(Chunk {
                    flags: chunk.flags,
                    vectors: chunk.vectors.clone(),
                })
            })).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_operations() {
        let mut map = SparseMap::new();
        assert_eq!(map.count(), 0);

        map.set(42);
        assert!(map.is_set(42));
        assert!(!map.is_set(43));
        assert_eq!(map.count(), 1);

        map.set(1000);
        assert!(map.is_set(1000));
        assert_eq!(map.count(), 2);

        map.clear(42);
        assert!(!map.is_set(42));
        assert_eq!(map.count(), 1);
    }

    #[test]
    fn test_sparse_large_indices() {
        let mut map = SparseMap::new();
        map.set(0);
        map.set(1_000_000);
        map.set(100_000_000);
        assert_eq!(map.count(), 3);
    }

    #[test]
    fn test_union() {
        let mut map1 = SparseMap::new();
        map1.set(1);
        map1.set(2);

        let mut map2 = SparseMap::new();
        map2.set(2);
        map2.set(3);

        map1.union(&map2);
        assert!(map1.is_set(1));
        assert!(map1.is_set(2));
        assert!(map1.is_set(3));
        assert_eq!(map1.count(), 3);
    }

    #[test]
    fn test_intersect() {
        let mut map1 = SparseMap::new();
        map1.set(1);
        map1.set(2);
        map1.set(3);

        let mut map2 = SparseMap::new();
        map2.set(2);
        map2.set(3);
        map2.set(4);

        map1.intersect(&map2);
        assert!(!map1.is_set(1));
        assert!(map1.is_set(2));
        assert!(map1.is_set(3));
        assert!(!map1.is_set(4));
        assert_eq!(map1.count(), 2);
    }

    #[test]
    fn test_iter() {
        let mut map = SparseMap::new();
        map.set(5);
        map.set(10);
        map.set(15);

        let collected: Vec<Idx> = map.iter().collect();
        assert_eq!(collected, vec![5, 10, 15]);
    }
}
