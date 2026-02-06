//! Deterministic state hashing for kernel state.
//!
//! This module provides functionality to compute a cryptographic hash of the
//! entire kernel state. The hash is deterministic: same state → same hash.
//!
//! # Purpose
//!
//! State hashing enables:
//! - **Determinism validation**: Same input log → identical state hash
//! - **Replay verification**: Replaying log produces same final state
//! - **Cross-replica consistency**: All replicas at same offset have same hash
//!
//! # Algorithm
//!
//! We use BLAKE3 for fast, secure hashing. The hash includes:
//! - All streams (sorted by `StreamId`)
//! - All tables (sorted by `TableId`)
//! - All indexes (sorted by `IndexId`)
//! - Next ID counters (streams, tables, indexes)
//!
//! Order is critical for determinism - we use `BTreeMap`'s sorted iteration.

use blake3::Hasher;

use crate::state::State;

impl State {
    /// Computes a deterministic hash of the entire kernel state.
    ///
    /// # Determinism
    ///
    /// The hash is computed by hashing all state fields in a fixed order:
    /// 1. Stream count + `next_stream_id`
    /// 2. All streams (sorted by `StreamId`)
    /// 3. Table count + `next_table_id`
    /// 4. All tables (sorted by `TableId`)
    /// 5. Table name index entries (sorted by name)
    /// 6. Index count + `next_index_id`
    /// 7. All indexes (sorted by `IndexId`)
    ///
    /// `BTreeMap` iteration is sorted, ensuring determinism.
    ///
    /// # Returns
    ///
    /// A 32-byte BLAKE3 hash of the state.
    ///
    /// # Examples
    ///
    /// ```
    /// use kimberlite_kernel::State;
    ///
    /// let state1 = State::new();
    /// let state2 = State::new();
    ///
    /// // Same state → same hash
    /// assert_eq!(state1.compute_state_hash(), state2.compute_state_hash());
    /// ```
    pub fn compute_state_hash(&self) -> [u8; 32] {
        let mut hasher = Hasher::new();

        // Hash stream state
        hasher.update(&self.stream_count().to_le_bytes());
        hasher.update(&u64::from(self.next_stream_id()).to_le_bytes());

        // Hash all streams (BTreeMap is sorted)
        for (stream_id, metadata) in self.streams() {
            hasher.update(&u64::from(*stream_id).to_le_bytes());
            hasher.update(metadata.stream_name.as_str().as_bytes());
            hasher.update(&u64::from(metadata.current_offset).to_le_bytes());
            hasher.update(&(metadata.data_class as u8).to_le_bytes());
            // Hash placement
            match &metadata.placement {
                kimberlite_types::Placement::Region(region) => {
                    hasher.update(&[0u8]); // tag for Region
                    // Hash region variant
                    match region {
                        kimberlite_types::Region::USEast1 => {
                            hasher.update(&[0u8]);
                        }
                        kimberlite_types::Region::APSoutheast2 => {
                            hasher.update(&[1u8]);
                        }
                        kimberlite_types::Region::Custom(name) => {
                            hasher.update(&[255u8]);
                            hasher.update(name.as_bytes());
                        }
                    }
                }
                kimberlite_types::Placement::Global => {
                    hasher.update(&[1u8]); // tag for Global
                }
            }
        }

        // Hash table state
        hasher.update(&self.table_count().to_le_bytes());
        hasher.update(&self.next_table_id().0.to_le_bytes());

        // Hash all tables (BTreeMap is sorted)
        for (table_id, table_meta) in self.tables() {
            hasher.update(&table_id.0.to_le_bytes());
            hasher.update(table_meta.table_name.as_bytes());
            hasher.update(&u64::from(table_meta.stream_id).to_le_bytes());

            // Hash columns
            hasher.update(&table_meta.columns.len().to_le_bytes());
            for col in &table_meta.columns {
                hasher.update(col.name.as_bytes());
                hasher.update(col.data_type.as_bytes());
                hasher.update(&[u8::from(col.nullable)]);
            }

            // Hash primary key
            hasher.update(&table_meta.primary_key.len().to_le_bytes());
            for pk_col in &table_meta.primary_key {
                hasher.update(pk_col.as_bytes());
            }
        }

        // Hash table name index (sorted by name)
        hasher.update(&self.table_name_index_len().to_le_bytes());
        for (name, table_id) in self.table_name_index() {
            hasher.update(name.as_bytes());
            hasher.update(&table_id.0.to_le_bytes());
        }

        // Hash index state
        hasher.update(&self.index_count().to_le_bytes());
        hasher.update(&self.next_index_id().0.to_le_bytes());

        // Hash all indexes (BTreeMap is sorted)
        for (index_id, index_meta) in self.indexes() {
            hasher.update(&index_id.0.to_le_bytes());
            hasher.update(index_meta.index_name.as_bytes());
            hasher.update(&index_meta.table_id.0.to_le_bytes());

            // Hash indexed columns
            hasher.update(&index_meta.columns.len().to_le_bytes());
            for col in &index_meta.columns {
                hasher.update(col.as_bytes());
            }
        }

        // Finalize hash
        *hasher.finalize().as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kimberlite_types::{DataClass, Placement, Region, StreamName};

    #[test]
    fn test_empty_state_hash_is_deterministic() {
        let state1 = State::new();
        let state2 = State::new();

        let hash1 = state1.compute_state_hash();
        let hash2 = state2.compute_state_hash();

        assert_eq!(hash1, hash2, "Empty states should have identical hashes");
    }

    #[test]
    fn test_different_states_have_different_hashes() {
        let state1 = State::new();

        let (state2, _meta) = state1.clone().with_new_stream(
            StreamName::new("test-stream"),
            DataClass::Public,
            Placement::Region(Region::USEast1),
        );

        let hash1 = state1.compute_state_hash();
        let hash2 = state2.compute_state_hash();

        assert_ne!(
            hash1, hash2,
            "States with different streams should have different hashes"
        );
    }

    #[test]
    fn test_same_state_multiple_hashes() {
        let (state, _meta) = State::new().with_new_stream(
            StreamName::new("test-stream"),
            DataClass::Public,
            Placement::Global,
        );

        let hash1 = state.compute_state_hash();
        let hash2 = state.compute_state_hash();
        let hash3 = state.compute_state_hash();

        assert_eq!(hash1, hash2);
        assert_eq!(hash2, hash3);
    }

    #[test]
    fn test_stream_offset_affects_hash() {
        let (state1, meta) = State::new().with_new_stream(
            StreamName::new("test-stream"),
            DataClass::Public,
            Placement::Region(Region::USEast1),
        );

        let state2 = state1
            .clone()
            .with_updated_offset(meta.stream_id, kimberlite_types::Offset::from(100u64));

        let hash1 = state1.compute_state_hash();
        let hash2 = state2.compute_state_hash();

        assert_ne!(
            hash1, hash2,
            "Different stream offsets should produce different hashes"
        );
    }

    #[test]
    fn test_hash_includes_all_stream_metadata() {
        // Create two states with same stream ID but different metadata
        let (state1, _) = State::new().with_new_stream(
            StreamName::new("stream-a"),
            DataClass::Public,
            Placement::Region(Region::USEast1),
        );

        let (state2, _) = State::new().with_new_stream(
            StreamName::new("stream-b"),
            DataClass::Public,
            Placement::Region(Region::USEast1),
        );

        let hash1 = state1.compute_state_hash();
        let hash2 = state2.compute_state_hash();

        assert_ne!(
            hash1, hash2,
            "Different stream names should produce different hashes"
        );
    }

    #[test]
    fn test_hash_is_32_bytes() {
        let state = State::new();
        let hash = state.compute_state_hash();
        assert_eq!(hash.len(), 32, "BLAKE3 hash should be 32 bytes");
    }
}
