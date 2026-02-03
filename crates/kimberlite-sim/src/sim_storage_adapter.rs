//! Storage adapter for VSR replicas in simulation.
//!
//! This module adapts the simulation's `SimStorage` for use with VSR replicas.
//! VSR uses an effect-based I/O model where state transitions produce `Effect`s
//! that must be executed externally. This adapter executes those effects through
//! the deterministic `SimStorage`.
//!
//! ## Design
//!
//! The adapter maintains:
//! - Per-replica logs (for consistency checking)
//! - Effect execution with simulated latency
//! - Deterministic storage behavior via SimRng
//!
//! ## Usage
//!
//! ```ignore
//! let storage = SimStorage::new(StorageConfig::reliable());
//! let mut adapter = SimStorageAdapter::new(storage);
//!
//! // Execute a VSR effect
//! adapter.write_effect(&effect, rng)?;
//! ```

use kimberlite_kernel::Effect;

use crate::{SimError, SimRng, SimStorage};

// ============================================================================
// Storage Adapter
// ============================================================================

/// Adapts `SimStorage` for VSR replica effect execution.
///
/// This adapter provides a bridge between VSR's effect-based I/O model
/// and the simulation's deterministic storage. It:
///
/// - Executes kernel effects through SimStorage
/// - Provides deterministic behavior via SimRng
/// - Tracks storage operations for testing
#[derive(Debug)]
pub struct SimStorageAdapter {
    /// The underlying simulated storage.
    storage: SimStorage,

    /// Next storage address to allocate.
    ///
    /// Each write gets a unique address for simplicity.
    next_address: u64,
}

impl SimStorageAdapter {
    /// Creates a new storage adapter wrapping the given SimStorage.
    pub fn new(storage: SimStorage) -> Self {
        Self {
            storage,
            next_address: 0,
        }
    }

    /// Executes a kernel effect through the storage layer.
    ///
    /// Effects are translated to storage operations:
    /// - `Effect::StorageAppend`: Write events to storage
    /// - `Effect::StreamMetadataWrite`: Write stream metadata
    /// - `Effect::WakeProjection`: Signal projection to catch up
    /// - `Effect::AuditLogAppend`: Write to audit log
    ///
    /// For Phase 1, we implement simplified effect execution focused on
    /// storage operations. Full projection and audit log support will be
    /// added in later phases.
    ///
    /// # Parameters
    ///
    /// - `effect`: The effect to execute
    /// - `rng`: Random number generator for simulated latency
    ///
    /// # Returns
    ///
    /// `Ok(())` on success, or an error if the storage operation fails.
    pub fn write_effect(&mut self, effect: &Effect, rng: &mut SimRng) -> Result<(), SimError> {
        match effect {
            Effect::StorageAppend {
                stream_id,
                base_offset,
                events,
            } => {
                // Write events to storage
                let address = self.allocate_address();

                // Serialize the append operation (simplified)
                // For Phase 1, we use a simple format - just concatenate the data
                let mut data = Vec::new();
                // Write a simplified header (we'll just use bincode for the whole thing)
                let header = bincode::serialize(&(stream_id, base_offset, events.len()))
                    .unwrap_or_default();
                data.extend_from_slice(&header);
                for event in events {
                    data.extend_from_slice(event);
                }

                // Write with retry logic (up to 3 retries for partial writes)
                self.write_with_retry(address, &data, rng, 3)?;
                Ok(())
            }
            Effect::StreamMetadataWrite(metadata) => {
                // Write stream metadata to storage
                let address = self.allocate_address();

                // Serialize metadata (simplified - use bincode)
                let data = bincode::serialize(metadata)
                    .map_err(|e| SimError::Serialization(format!("{}", e)))?;

                // Write with retry logic (up to 3 retries for partial writes)
                self.write_with_retry(address, &data, rng, 3)?;
                Ok(())
            }
            Effect::WakeProjection { .. } => {
                // Projection wakeup is a signal, no storage I/O needed
                // In real system, this would trigger projection catch-up
                Ok(())
            }
            Effect::AuditLogAppend(_action) => {
                // Write to audit log
                let address = self.allocate_address();
                let data = vec![0u8; 64]; // Placeholder audit entry
                // Write with retry logic (up to 3 retries for partial writes)
                self.write_with_retry(address, &data, rng, 3)?;
                Ok(())
            }
            Effect::TableMetadataWrite(_metadata) => {
                // Write table metadata
                let address = self.allocate_address();
                let data = vec![0u8; 128]; // Placeholder metadata
                // Write with retry logic (up to 3 retries for partial writes)
                self.write_with_retry(address, &data, rng, 3)?;
                Ok(())
            }
            Effect::TableMetadataDrop(_table_id) => {
                // Drop table metadata - no storage I/O needed for simulation
                Ok(())
            }
            Effect::IndexMetadataWrite(_metadata) => {
                // Write index metadata
                let address = self.allocate_address();
                let data = vec![0u8; 128]; // Placeholder metadata
                // Write with retry logic (up to 3 retries for partial writes)
                self.write_with_retry(address, &data, rng, 3)?;
                Ok(())
            }
            Effect::UpdateProjection { .. } => {
                // Projection update - signal only, no storage I/O in simulation
                Ok(())
            }
        }
    }


    /// Returns a reference to the underlying SimStorage.
    pub fn storage(&self) -> &SimStorage {
        &self.storage
    }

    /// Returns a mutable reference to the underlying SimStorage.
    pub fn storage_mut(&mut self) -> &mut SimStorage {
        &mut self.storage
    }

    /// Allocates a new storage address.
    ///
    /// Each write gets a unique address for simplicity.
    fn allocate_address(&mut self) -> u64 {
        let addr = self.next_address;
        self.next_address += 1;
        addr
    }

    /// Writes data to storage with automatic retry on partial writes.
    ///
    /// This simulates how real systems handle transient storage failures.
    /// Partial writes are retried up to `max_retries` times. Hard failures
    /// (corruption, unavailable storage) are not retried.
    ///
    /// # Parameters
    ///
    /// - `address`: Storage address to write to
    /// - `data`: Data to write
    /// - `rng`: Random number generator for simulated latency
    /// - `max_retries`: Maximum number of retry attempts for partial writes
    ///
    /// # Returns
    ///
    /// `Ok(())` on success, or an error if all retries exhausted or hard failure.
    fn write_with_retry(
        &mut self,
        address: u64,
        data: &[u8],
        rng: &mut SimRng,
        max_retries: u32,
    ) -> Result<(), SimError> {
        for attempt in 0..=max_retries {
            let result = self.storage.write(address, data.to_vec(), rng);

            match result {
                crate::WriteResult::Success { .. } => {
                    // Success on first try or after retries
                    return Ok(());
                }
                crate::WriteResult::Failed { reason, .. } => {
                    // Hard failure - don't retry
                    // These represent permanent failures like corruption or unavailable storage
                    return Err(SimError::StorageFailure(format!("{:?}", reason)));
                }
                crate::WriteResult::Partial { bytes_written, .. } => {
                    // Transient failure - retry unless we've exhausted attempts
                    if attempt == max_retries {
                        return Err(SimError::StorageFailure(format!(
                            "partial write after {} retries ({} of {} bytes written)",
                            max_retries,
                            bytes_written,
                            data.len()
                        )));
                    }
                    // Continue to next retry attempt
                    // In a real system, we might log this or add backoff delay
                }
            }
        }

        unreachable!("loop should return before reaching this point")
    }

}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StorageConfig;
    use bytes::Bytes;
    use kimberlite_types::{Offset, StreamId, StreamMetadata, TenantId};

    fn test_storage() -> SimStorage {
        SimStorage::new(StorageConfig::reliable())
    }

    #[test]
    fn adapter_creation() {
        let storage = test_storage();
        let adapter = SimStorageAdapter::new(storage);

        assert_eq!(adapter.storage().block_count(), 0);
    }

    #[test]
    fn execute_storage_append_effect() {
        let storage = test_storage();
        let mut adapter = SimStorageAdapter::new(storage);
        let mut rng = SimRng::new(42);

        let effect = Effect::StorageAppend {
            stream_id: StreamId::from_tenant_and_local(TenantId::new(1), 1),
            base_offset: Offset::ZERO,
            events: vec![Bytes::from(b"event1".to_vec()), Bytes::from(b"event2".to_vec())],
        };

        let result = adapter.write_effect(&effect, &mut rng);
        assert!(result.is_ok());

        // Check that storage was written to
        assert_eq!(adapter.storage().stats().writes, 1);
        assert_eq!(adapter.storage().stats().writes_successful, 1);
    }

    #[test]
    fn execute_metadata_write_effect() {
        let storage = test_storage();
        let mut adapter = SimStorageAdapter::new(storage);
        let mut rng = SimRng::new(42);

        use kimberlite_types::{DataClass, Placement, Region, StreamName};

        let metadata = StreamMetadata {
            stream_id: StreamId::from_tenant_and_local(TenantId::new(1), 1),
            stream_name: StreamName::from("test_stream"),
            data_class: DataClass::PHI,
            placement: Placement::Region(Region::USEast1),
            current_offset: Offset::ZERO,
        };

        let effect = Effect::StreamMetadataWrite(metadata);

        let result = adapter.write_effect(&effect, &mut rng);
        assert!(result.is_ok());

        assert_eq!(adapter.storage().stats().writes, 1);
    }

    #[test]
    fn execute_wake_projection_effect() {
        let storage = test_storage();
        let mut adapter = SimStorageAdapter::new(storage);
        let mut rng = SimRng::new(42);

        let effect = Effect::WakeProjection {
            stream_id: StreamId::from_tenant_and_local(TenantId::new(1), 1),
            from_offset: Offset::ZERO,
            to_offset: Offset::from(10),
        };

        let result = adapter.write_effect(&effect, &mut rng);
        assert!(result.is_ok());

        // Wake projection doesn't write to storage
        assert_eq!(adapter.storage().stats().writes, 0);
    }

    #[test]
    fn execute_audit_log_effect() {
        let storage = test_storage();
        let mut adapter = SimStorageAdapter::new(storage);
        let mut rng = SimRng::new(42);

        use kimberlite_types::{AuditAction, DataClass, Placement, Region, StreamName};

        let effect = Effect::AuditLogAppend(AuditAction::StreamCreated {
            stream_id: StreamId::from_tenant_and_local(TenantId::new(1), 1),
            stream_name: StreamName::from("test_stream"),
            data_class: DataClass::PHI,
            placement: Placement::Region(Region::USEast1),
        });

        let result = adapter.write_effect(&effect, &mut rng);
        assert!(result.is_ok());

        assert_eq!(adapter.storage().stats().writes, 1);
    }
}
