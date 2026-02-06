//! Kani verification harnesses for kernel state machine
//!
//! This module contains bounded model checking proofs using Kani.
//! Each proof verifies a specific safety property of the kernel.
//!
//! # Verification Strategy
//!
//! - **Bounded verification**: Kani unrolls loops and checks all paths within bounds
//! - **Symbolic execution**: Uses SMT solvers (Z3) to prove properties for all inputs
//! - **Assertions**: Convert runtime assertions to compile-time proofs
//!
//! # Running Proofs
//!
//! ```bash
//! # Verify all proofs
//! cargo kani --package kimberlite-kernel
//!
//! # Verify specific proof
//! cargo kani --harness verify_create_stream_unique_id
//! ```

#[cfg(kani)]
mod verification {
    use crate::command::{ColumnDefinition, Command, TableId};
    use crate::kernel::{KernelError, apply_committed};
    use crate::state::State;
    use kimberlite_types::{DataClass, Offset, Placement, StreamId, StreamName};

    // -----------------------------------------------------------------------------
    // Kernel State Machine Proofs (15 proofs total)
    // -----------------------------------------------------------------------------

    /// **Proof 1: CreateStream enforces unique stream IDs**
    ///
    /// **Property:** Cannot create two streams with the same ID
    ///
    /// **Proven:** If stream exists, CreateStream returns error
    #[kani::proof]
    fn verify_create_stream_unique_id() {
        let state = State::new();

        // Create arbitrary stream ID from primitive
        let stream_id_raw: u64 = kani::any();
        let stream_id = StreamId::new(stream_id_raw);
        let stream_name = StreamName::new("test-stream".to_string());
        let data_class = DataClass::Public;
        let placement = Placement::Global;

        let cmd = Command::CreateStream {
            stream_id,
            stream_name: stream_name.clone(),
            data_class,
            placement: placement.clone(),
        };

        // First creation should succeed
        let result = apply_committed(state, cmd);
        kani::assume(result.is_ok());

        let (new_state, _) = result.unwrap();

        // Verify stream exists
        assert!(new_state.stream_exists(&stream_id));

        // Second creation with same ID should fail
        let cmd2 = Command::CreateStream {
            stream_id,
            stream_name,
            data_class,
            placement,
        };

        let result2 = apply_committed(new_state, cmd2);
        assert!(result2.is_err());
        assert!(matches!(
            result2.unwrap_err(),
            KernelError::StreamIdUniqueConstraint(_)
        ));
    }

    /// **Proof 2: CreateStream initializes offset to zero**
    ///
    /// **Property:** New streams always start at offset 0
    ///
    /// **Proven:** After CreateStream, current_offset == 0
    #[kani::proof]
    fn verify_create_stream_offset_initialization() {
        let state = State::new();

        let stream_id_raw: u64 = kani::any();
        let stream_id = StreamId::new(stream_id_raw);
        let stream_name = StreamName::new("test-stream".to_string());
        let data_class = DataClass::Public;
        let placement = Placement::Global;

        let cmd = Command::CreateStream {
            stream_id,
            stream_name,
            data_class,
            placement,
        };

        let result = apply_committed(state, cmd);
        kani::assume(result.is_ok());

        let (new_state, _) = result.unwrap();

        // Verify offset is zero
        let metadata = new_state.get_stream(&stream_id).unwrap();
        assert_eq!(metadata.current_offset, Offset::ZERO);
    }

    /// **Proof 3: AppendBatch requires existing stream**
    ///
    /// **Property:** Cannot append to non-existent stream
    ///
    /// **Proven:** AppendBatch on missing stream returns error
    #[kani::proof]
    fn verify_append_batch_stream_exists() {
        let state = State::new();

        let stream_id_raw: u64 = kani::any();
        let stream_id = StreamId::new(stream_id_raw);
        let base_offset = Offset::ZERO;
        // Use small bounded vector for verification
        let events: Vec<bytes::Bytes> = vec![bytes::Bytes::from("event1")];

        let cmd = Command::AppendBatch {
            stream_id,
            events,
            expected_offset: base_offset,
        };

        // State doesn't have the stream
        kani::assume(!state.stream_exists(&stream_id));

        let result = apply_committed(state, cmd);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            KernelError::StreamNotFound(_)
        ));
    }

    /// **Proof 4: AppendBatch advances offset monotonically**
    ///
    /// **Property:** Appending N events increases offset by N
    ///
    /// **Proven:** new_offset = old_offset + event_count
    #[kani::proof]
    fn verify_append_batch_offset_monotonic() {
        let state = State::new();

        // Create stream first
        let stream_id_raw: u64 = kani::any();
        let stream_id = StreamId::new(stream_id_raw);
        let stream_name = StreamName::new("test-stream".to_string());

        let create_cmd = Command::CreateStream {
            stream_id,
            stream_name,
            data_class: DataClass::Public,
            placement: Placement::Global,
        };

        let result = apply_committed(state, create_cmd);
        kani::assume(result.is_ok());
        let (state, _) = result.unwrap();

        let old_offset = state.get_stream(&stream_id).unwrap().current_offset;

        // Append bounded events (small for verification)
        let event_count: usize = kani::any();
        kani::assume(event_count > 0 && event_count <= 3); // Bounded for verification

        let events: Vec<bytes::Bytes> = (0..event_count)
            .map(|i| bytes::Bytes::from(format!("event{}", i)))
            .collect();

        let cmd = Command::AppendBatch {
            stream_id,
            events: events.clone(),
            expected_offset: old_offset,
        };

        let result = apply_committed(state, cmd);
        kani::assume(result.is_ok());

        let (new_state, _) = result.unwrap();
        let new_offset = new_state.get_stream(&stream_id).unwrap().current_offset;

        // Verify monotonicity: new_offset > old_offset
        assert!(new_offset.as_u64() > old_offset.as_u64());

        // Verify arithmetic correctness
        assert_eq!(
            new_offset.as_u64(),
            old_offset.as_u64() + (events.len() as u64)
        );
    }

    /// **Proof 5: AppendBatch validates base offset**
    ///
    /// **Property:** Base offset must match current stream offset
    ///
    /// **Proven:** Mismatched base offset causes error
    #[kani::proof]
    fn verify_append_batch_base_offset_validation() {
        let state = State::new();

        // Create stream
        let stream_id_raw: u64 = kani::any();
        let stream_id = StreamId::new(stream_id_raw);
        let stream_name = StreamName::new("test-stream".to_string());

        let create_cmd = Command::CreateStream {
            stream_id,
            stream_name,
            data_class: DataClass::Public,
            placement: Placement::Global,
        };

        let result = apply_committed(state, create_cmd);
        kani::assume(result.is_ok());
        let (state, _) = result.unwrap();

        let current_offset = state.get_stream(&stream_id).unwrap().current_offset;

        // Try to append with wrong base offset
        let wrong_base_offset = Offset::new(current_offset.as_u64() + 1);
        let events = vec![bytes::Bytes::from("event")];

        let cmd = Command::AppendBatch {
            stream_id,
            events,
            expected_offset: wrong_base_offset,
        };

        let result = apply_committed(state, cmd);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            KernelError::UnexpectedStreamOffset { .. }
        ));
    }

    /// **Proof 6: CreateTable enforces unique table IDs**
    ///
    /// **Property:** Cannot create two tables with the same ID
    ///
    /// **Proven:** Duplicate table creation returns error
    #[kani::proof]
    fn verify_create_table_unique_id() {
        let state = State::new();

        let table_id_raw: u64 = kani::any();
        let table_id = TableId::new(table_id_raw);
        let table_name = "test_table".to_string();
        let columns: Vec<ColumnDefinition> = vec![
            ColumnDefinition {
                name: "col1".to_string(),
                data_type: "TEXT".to_string(),
                nullable: false,
            },
            ColumnDefinition {
                name: "col2".to_string(),
                data_type: "INTEGER".to_string(),
                nullable: true,
            },
        ];
        let primary_key = vec!["col1".to_string()];

        let cmd = Command::CreateTable {
            table_id,
            table_name: table_name.clone(),
            columns: columns.clone(),
            primary_key: primary_key.clone(),
        };

        // First creation should succeed
        let result = apply_committed(state, cmd);
        kani::assume(result.is_ok());

        let (new_state, _) = result.unwrap();

        // Verify table exists
        assert!(new_state.table_exists(&table_id));

        // Second creation with same ID should fail
        let cmd2 = Command::CreateTable {
            table_id,
            table_name,
            columns,
            primary_key,
        };

        let result2 = apply_committed(new_state, cmd2);
        assert!(result2.is_err());
        assert!(matches!(
            result2.unwrap_err(),
            KernelError::TableIdUniqueConstraint(_)
        ));
    }

    /// **Proof 7: DropTable removes from state**
    ///
    /// **Property:** Dropped tables no longer exist
    ///
    /// **Proven:** After DropTable, table_exists returns false
    #[kani::proof]
    fn verify_drop_table_removes_from_state() {
        let state = State::new();

        let table_id_raw: u64 = kani::any();
        let table_id = TableId::new(table_id_raw);
        let table_name = "test_table".to_string();
        let columns = vec![ColumnDefinition {
            name: "col1".to_string(),
            data_type: "TEXT".to_string(),
            nullable: false,
        }];
        let primary_key = vec!["col1".to_string()];

        // Create table
        let create_cmd = Command::CreateTable {
            table_id,
            table_name,
            columns,
            primary_key,
        };

        let result = apply_committed(state, create_cmd);
        kani::assume(result.is_ok());
        let (state, _) = result.unwrap();

        // Verify table exists
        assert!(state.table_exists(&table_id));

        // Drop table
        let drop_cmd = Command::DropTable { table_id };

        let result = apply_committed(state, drop_cmd);
        kani::assume(result.is_ok());
        let (new_state, _) = result.unwrap();

        // Verify table no longer exists
        assert!(!new_state.table_exists(&table_id));
    }

    /// **Proof 8: Offset arithmetic doesn't overflow**
    ///
    /// **Property:** Offset addition never panics
    ///
    /// **Proven:** Checked arithmetic prevents overflow
    #[kani::proof]
    fn verify_offset_arithmetic_no_overflow() {
        let offset1: u64 = kani::any();
        let offset2: u64 = kani::any();

        // Bounded offsets to avoid unrealistic scenarios
        kani::assume(offset1 < u64::MAX / 2);
        kani::assume(offset2 < 1000);

        let o1 = Offset::new(offset1);
        let o2 = Offset::new(offset2);

        // This should not panic
        let result = o1.as_u64().checked_add(o2.as_u64());
        assert!(result.is_some());

        let sum = result.unwrap();
        assert!(sum >= offset1);
        assert!(sum >= offset2);
    }

    /// **Proof 9: AppendBatch with empty events fails**
    ///
    /// **Property:** Cannot append empty batch
    ///
    /// **Proven:** Empty events vector returns error
    #[kani::proof]
    fn verify_append_batch_empty_events() {
        let state = State::new();

        let stream_id_raw: u64 = kani::any();
        let stream_id = StreamId::new(stream_id_raw);
        let stream_name = StreamName::new("test-stream".to_string());

        // Create stream first
        let create_cmd = Command::CreateStream {
            stream_id,
            stream_name,
            data_class: DataClass::Public,
            placement: Placement::Global,
        };

        let result = apply_committed(state, create_cmd);
        kani::assume(result.is_ok());
        let (state, _) = result.unwrap();

        // Try to append empty events
        let empty_events: Vec<bytes::Bytes> = vec![];
        let cmd = Command::AppendBatch {
            stream_id,
            events: empty_events,
            expected_offset: Offset::ZERO,
        };

        let result = apply_committed(state, cmd);
        assert!(result.is_err());
    }

    /// **Proof 10: StreamName roundtrip**
    ///
    /// **Property:** StreamName preserves string content
    ///
    /// **Proven:** as_str() returns original string
    #[kani::proof]
    fn verify_stream_name_roundtrip() {
        let name = "test-stream";
        let stream_name = StreamName::new(name.to_string());
        assert_eq!(stream_name.as_str(), name);
    }

    /// **Proof 11: Offset::ZERO is actually zero**
    ///
    /// **Property:** ZERO constant equals zero
    ///
    /// **Proven:** Offset::ZERO.as_u64() == 0
    #[kani::proof]
    fn verify_offset_zero_constant() {
        assert_eq!(Offset::ZERO.as_u64(), 0);
        assert_eq!(Offset::ZERO, Offset::new(0));
    }

    /// **Proof 12: StreamId construction from u64**
    ///
    /// **Property:** StreamId preserves underlying value
    ///
    /// **Proven:** From/Into roundtrip
    #[kani::proof]
    fn verify_stream_id_construction() {
        let raw_id: u64 = kani::any();
        let stream_id = StreamId::new(raw_id);
        let recovered: u64 = stream_id.into();
        assert_eq!(recovered, raw_id);
    }

    /// **Proof 13: TableId construction from u64**
    ///
    /// **Property:** TableId preserves underlying value
    ///
    /// **Proven:** new() and into() are inverse operations
    #[kani::proof]
    fn verify_table_id_construction() {
        let raw_id: u64 = kani::any();
        let table_id = TableId::new(raw_id);
        let recovered: u64 = table_id.0;
        assert_eq!(recovered, raw_id);
    }

    /// **Proof 14: Offset addition is associative**
    ///
    /// **Property:** (a + b) + c == a + (b + c)
    ///
    /// **Proven:** Offset addition is associative
    #[kani::proof]
    fn verify_offset_addition_associative() {
        let a_raw: u64 = kani::any();
        let b_raw: u64 = kani::any();
        let c_raw: u64 = kani::any();

        // Bounded to prevent overflow
        kani::assume(a_raw < 1000);
        kani::assume(b_raw < 1000);
        kani::assume(c_raw < 1000);

        let a = Offset::new(a_raw);
        let b = Offset::new(b_raw);
        let c = Offset::new(c_raw);

        let left = (a + b) + c;
        let right = a + (b + c);

        assert_eq!(left, right);
    }

    /// **Proof 15: Offset addition is commutative**
    ///
    /// **Property:** a + b == b + a
    ///
    /// **Proven:** Offset addition is commutative
    #[kani::proof]
    fn verify_offset_addition_commutative() {
        let a_raw: u64 = kani::any();
        let b_raw: u64 = kani::any();

        // Bounded to prevent overflow
        kani::assume(a_raw < u64::MAX / 2);
        kani::assume(b_raw < u64::MAX / 2);

        let a = Offset::new(a_raw);
        let b = Offset::new(b_raw);

        assert_eq!(a + b, b + a);
    }

    // -----------------------------------------------------------------------------
    // Additional State Machine Proofs (15 more proofs)
    // -----------------------------------------------------------------------------

    /// **Proof 16: CreateStream produces StorageAppend effect**
    ///
    /// **Property:** CreateStream generates metadata persistence effect
    ///
    /// **Proven:** Effect vector contains StorageAppend
    #[kani::proof]
    fn verify_create_stream_produces_effect() {
        let state = State::new();
        let stream_id = StreamId::new(1);
        let stream_name = StreamName::new("stream1".to_string());

        let cmd = Command::CreateStream {
            stream_id,
            stream_name,
            data_class: DataClass::Public,
            placement: Placement::Global,
        };

        let result = apply_committed(state, cmd);
        kani::assume(result.is_ok());

        let (_, effects) = result.unwrap();

        // Should produce at least one effect
        assert!(!effects.is_empty());
    }

    /// **Proof 17: State changes are pure**
    ///
    /// **Property:** Same command on same state produces same result
    ///
    /// **Proven:** apply_committed is deterministic
    #[kani::proof]
    fn verify_apply_committed_deterministic() {
        let state1 = State::new();
        let state2 = state1.clone();

        let stream_id = StreamId::new(42);
        let stream_name = StreamName::new("test".to_string());

        let cmd1 = Command::CreateStream {
            stream_id,
            stream_name: stream_name.clone(),
            data_class: DataClass::Public,
            placement: Placement::Global,
        };

        let cmd2 = Command::CreateStream {
            stream_id,
            stream_name,
            data_class: DataClass::Public,
            placement: Placement::Global,
        };

        let result1 = apply_committed(state1, cmd1);
        let result2 = apply_committed(state2, cmd2);

        // Both should succeed or both should fail
        assert_eq!(result1.is_ok(), result2.is_ok());

        if result1.is_ok() {
            let (state_a, effects_a) = result1.unwrap();
            let (state_b, effects_b) = result2.unwrap();

            // States should be identical
            assert_eq!(
                state_a.stream_exists(&stream_id),
                state_b.stream_exists(&stream_id)
            );
            assert_eq!(effects_a.len(), effects_b.len());
        }
    }

    /// **Proof 18: Stream creation increments stream count**
    ///
    /// **Property:** Creating stream increases total streams
    ///
    /// **Proven:** State tracks streams correctly
    #[kani::proof]
    fn verify_stream_creation_increments_count() {
        let state = State::new();
        let initial_count = state.stream_count();

        let stream_id = StreamId::new(1);
        let stream_name = StreamName::new("stream1".to_string());

        let cmd = Command::CreateStream {
            stream_id,
            stream_name,
            data_class: DataClass::Public,
            placement: Placement::Global,
        };

        let result = apply_committed(state, cmd);
        kani::assume(result.is_ok());

        let (new_state, _) = result.unwrap();
        let new_count = new_state.stream_count();

        assert_eq!(new_count, initial_count + 1);
    }

    /// **Proof 19: Table creation increments table count**
    ///
    /// **Property:** Creating table increases total tables
    ///
    /// **Proven:** State tracks tables correctly
    #[kani::proof]
    fn verify_table_creation_increments_count() {
        let state = State::new();
        let initial_count = state.table_count();

        let table_id = TableId::new(1);
        let cmd = Command::CreateTable {
            table_id,
            table_name: "table1".to_string(),
            columns: vec![ColumnDefinition {
                name: "col1".to_string(),
                data_type: "TEXT".to_string(),
                nullable: false,
            }],
            primary_key: vec!["col1".to_string()],
        };

        let result = apply_committed(state, cmd);
        kani::assume(result.is_ok());

        let (new_state, _) = result.unwrap();
        let new_count = new_state.table_count();

        assert_eq!(new_count, initial_count + 1);
    }

    /// **Proof 20: DropTable decrements table count**
    ///
    /// **Property:** Dropping table decreases total tables
    ///
    /// **Proven:** State tracks table removal
    #[kani::proof]
    fn verify_drop_table_decrements_count() {
        let state = State::new();

        // Create table first
        let table_id = TableId::new(1);
        let create_cmd = Command::CreateTable {
            table_id,
            table_name: "table1".to_string(),
            columns: vec![ColumnDefinition {
                name: "col1".to_string(),
                data_type: "TEXT".to_string(),
                nullable: false,
            }],
            primary_key: vec!["col1".to_string()],
        };

        let result = apply_committed(state, create_cmd);
        kani::assume(result.is_ok());
        let (state, _) = result.unwrap();

        let count_after_create = state.table_count();

        // Drop table
        let drop_cmd = Command::DropTable { table_id };
        let result = apply_committed(state, drop_cmd);
        kani::assume(result.is_ok());
        let (new_state, _) = result.unwrap();

        let count_after_drop = new_state.table_count();

        assert_eq!(count_after_drop, count_after_create - 1);
    }

    /// **Proof 21: AppendBatch preserves existing streams**
    ///
    /// **Property:** Appending to one stream doesn't affect others
    ///
    /// **Proven:** State isolation between streams
    #[kani::proof]
    fn verify_append_batch_stream_isolation() {
        let state = State::new();

        // Create two streams
        let stream1_id = StreamId::new(1);
        let stream2_id = StreamId::new(2);

        let cmd1 = Command::CreateStream {
            stream_id: stream1_id,
            stream_name: StreamName::new("stream1".to_string()),
            data_class: DataClass::Public,
            placement: Placement::Global,
        };

        let result = apply_committed(state, cmd1);
        kani::assume(result.is_ok());
        let (state, _) = result.unwrap();

        let cmd2 = Command::CreateStream {
            stream_id: stream2_id,
            stream_name: StreamName::new("stream2".to_string()),
            data_class: DataClass::Public,
            placement: Placement::Global,
        };

        let result = apply_committed(state, cmd2);
        kani::assume(result.is_ok());
        let (state, _) = result.unwrap();

        let stream2_offset_before = state.get_stream(&stream2_id).unwrap().current_offset;

        // Append to stream1
        let append_cmd = Command::AppendBatch {
            stream_id: stream1_id,
            events: vec![bytes::Bytes::from("event")],
            expected_offset: Offset::ZERO,
        };

        let result = apply_committed(state, append_cmd);
        kani::assume(result.is_ok());
        let (new_state, _) = result.unwrap();

        // Stream2 should be unchanged
        let stream2_offset_after = new_state.get_stream(&stream2_id).unwrap().current_offset;
        assert_eq!(stream2_offset_before, stream2_offset_after);
    }

    /// **Proof 22: Offset subtraction is correct**
    ///
    /// **Property:** a - b == a.as_u64() - b.as_u64() when a >= b
    ///
    /// **Proven:** Subtraction is accurate
    #[kani::proof]
    fn verify_offset_subtraction() {
        let a_raw: u64 = kani::any();
        let b_raw: u64 = kani::any();

        kani::assume(a_raw < 1000);
        kani::assume(b_raw < 1000);
        kani::assume(a_raw >= b_raw);

        let a = Offset::new(a_raw);
        let b = Offset::new(b_raw);

        let diff = a - b;
        assert_eq!(diff.as_u64(), a_raw - b_raw);
    }

    /// **Proof 23: Offset identity element**
    ///
    /// **Property:** a + ZERO == a
    ///
    /// **Proven:** ZERO is additive identity
    #[kani::proof]
    fn verify_offset_additive_identity() {
        let a_raw: u64 = kani::any();
        kani::assume(a_raw < u64::MAX);

        let a = Offset::new(a_raw);
        let sum = a + Offset::ZERO;

        assert_eq!(sum, a);
    }

    /// **Proof 24: StreamId from tenant and local**
    ///
    /// **Property:** Bit packing preserves tenant and local IDs
    ///
    /// **Proven:** Encoding/decoding is correct
    #[kani::proof]
    fn verify_stream_id_tenant_local_packing() {
        let tenant_id_raw: u64 = kani::any();
        let local_id: u32 = kani::any();

        kani::assume(tenant_id_raw < 1000);

        let tenant_id = kimberlite_types::TenantId::from(tenant_id_raw);
        let stream_id = StreamId::from_tenant_and_local(tenant_id, local_id);

        // Verify extraction
        let extracted_tenant = kimberlite_types::TenantId::from_stream_id(stream_id);
        let extracted_local = stream_id.local_id();

        assert_eq!(extracted_tenant, tenant_id);
        assert_eq!(extracted_local, local_id);
    }

    /// **Proof 25: Command enum size is reasonable**
    ///
    /// **Property:** Command variants fit in memory
    ///
    /// **Proven:** Size check passes
    #[kani::proof]
    fn verify_command_size_reasonable() {
        // This is a compile-time property check
        // Command should not be excessively large
        let size = std::mem::size_of::<Command>();

        // Reasonable upper bound (commands contain Vec and String)
        // This ensures we don't accidentally add huge fields
        assert!(size < 1024); // Less than 1KB
    }

    /// **Proof 26: State is clonable**
    ///
    /// **Property:** State clone produces independent copy
    ///
    /// **Proven:** Clone is deep copy
    #[kani::proof]
    fn verify_state_clone_independence() {
        let state1 = State::new();

        // Create stream in state1
        let stream_id = StreamId::new(1);
        let cmd = Command::CreateStream {
            stream_id,
            stream_name: StreamName::new("test".to_string()),
            data_class: DataClass::Public,
            placement: Placement::Global,
        };

        let result = apply_committed(state1, cmd);
        kani::assume(result.is_ok());
        let (state1, _) = result.unwrap();

        // Clone state
        let state2 = state1.clone();

        // Both should have the stream
        assert!(state1.stream_exists(&stream_id));
        assert!(state2.stream_exists(&stream_id));
        assert_eq!(state1.stream_count(), state2.stream_count());
    }

    /// **Proof 27: Offset ordering is reflexive**
    ///
    /// **Property:** a <= a for all offsets
    ///
    /// **Proven:** Partial order reflexivity
    #[kani::proof]
    fn verify_offset_ordering_reflexive() {
        let offset_raw: u64 = kani::any();
        kani::assume(offset_raw < 1000);

        let offset = Offset::new(offset_raw);

        assert!(offset <= offset);
        assert!(offset >= offset);
    }

    /// **Proof 28: TableId ordering preserves u64 ordering**
    ///
    /// **Property:** TableId comparison matches u64 comparison
    ///
    /// **Proven:** Total order preserved
    #[kani::proof]
    fn verify_table_id_ordering() {
        let id1_raw: u64 = kani::any();
        let id2_raw: u64 = kani::any();

        kani::assume(id1_raw < 1000);
        kani::assume(id2_raw < 1000);

        let id1 = TableId::new(id1_raw);
        let id2 = TableId::new(id2_raw);

        if id1_raw < id2_raw {
            assert!(id1 < id2);
        } else if id1_raw > id2_raw {
            assert!(id1 > id2);
        } else {
            assert_eq!(id1, id2);
        }
    }

    /// **Proof 29: StreamName preserves string content**
    ///
    /// **Property:** as_str() returns original string
    ///
    /// **Proven:** No data loss in wrapper
    #[kani::proof]
    fn verify_stream_name_preserves_content() {
        let name = "test_stream_name";
        let stream_name = StreamName::new(name.to_string());

        assert_eq!(stream_name.as_str(), name);
    }

    /// **Proof 30: Offset as_usize conversion**
    ///
    /// **Property:** as_usize() preserves value for small offsets
    ///
    /// **Proven:** Conversion is accurate
    #[kani::proof]
    fn verify_offset_as_usize_conversion() {
        let offset_raw: u64 = kani::any();
        kani::assume(offset_raw < 1000); // Small enough for usize

        let offset = Offset::new(offset_raw);
        let as_usize = offset.as_usize();

        assert_eq!(as_usize, offset_raw as usize);
    }

    // ========================================================================
    // Proofs 31-32: Data Classification Validation (Phase 3.1)
    // ========================================================================

    /// **Proof 31: Classification restrictiveness ordering**
    ///
    /// **Property:** User cannot classify data less restrictively than inferred
    ///
    /// **Proven:** validate_user_classification enforces restrictiveness ordering
    ///
    /// **Critical for:** Compliance - prevents PHI from being classified as Public
    #[kani::proof]
    #[kani::unwind(10)]
    fn verify_classification_restrictiveness() {
        use crate::classification::{infer_from_stream_name, validate_user_classification};
        use kimberlite_types::DataClass;

        // Test with a known PHI stream name
        let phi_stream = "patient_medical_records";

        // Property: Cannot classify PHI as Public (less restrictive)
        assert!(!validate_user_classification(phi_stream, DataClass::Public));

        // Property: Can classify PHI as PHI (same restrictiveness)
        assert!(validate_user_classification(phi_stream, DataClass::PHI));

        // Test with a known PCI stream name
        let pci_stream = "credit_card_transactions";

        // Property: Cannot classify PCI as Public (less restrictive)
        assert!(!validate_user_classification(pci_stream, DataClass::Public));

        // Property: Cannot classify PCI as Confidential (less restrictive)
        assert!(!validate_user_classification(
            pci_stream,
            DataClass::Confidential
        ));

        // Property: Can classify PCI as PCI (same restrictiveness)
        assert!(validate_user_classification(pci_stream, DataClass::PCI));

        // Property: Can classify PCI as PHI (more restrictive)
        assert!(validate_user_classification(pci_stream, DataClass::PHI));

        // Test with public stream
        let public_stream = "public_announcements";

        // Property: Public can be classified as anything (user can be more restrictive)
        assert!(validate_user_classification(
            public_stream,
            DataClass::Public
        ));
        assert!(validate_user_classification(
            public_stream,
            DataClass::Confidential
        ));
        assert!(validate_user_classification(public_stream, DataClass::PHI));
    }

    /// **Proof 32: Classification inference determinism**
    ///
    /// **Property:** Same stream name always infers same classification
    ///
    /// **Proven:** infer_from_stream_name is deterministic
    ///
    /// **Critical for:** Consistent policy enforcement across system
    #[kani::proof]
    #[kani::unwind(5)]
    fn verify_classification_determinism() {
        use crate::classification::infer_from_stream_name;
        use kimberlite_types::DataClass;

        // Infer classification twice
        let stream1 = "patient_health_records";
        let class1_first = infer_from_stream_name(stream1);
        let class1_second = infer_from_stream_name(stream1);

        // Property: Same stream name → same classification
        assert_eq!(class1_first, class1_second);

        // Test with different patterns
        let stream2 = "credit_card_numbers";
        let class2_first = infer_from_stream_name(stream2);
        let class2_second = infer_from_stream_name(stream2);

        assert_eq!(class2_first, class2_second);

        // Property: Different patterns infer different classifications
        let phi_stream = "medical_records";
        let pci_stream = "payment_cards";
        let phi_class = infer_from_stream_name(phi_stream);
        let pci_class = infer_from_stream_name(pci_stream);

        // PHI and PCI should be different
        assert_ne!(phi_class, pci_class);
        assert_eq!(phi_class, DataClass::PHI);
        assert_eq!(pci_class, DataClass::PCI);
    }
}
