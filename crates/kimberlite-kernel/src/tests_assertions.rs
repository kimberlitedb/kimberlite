//! Tests for production assertions promoted from `debug_assert!()`
//!
//! This module verifies that kernel state machine invariants are enforced.
//! Tests verify that all 4 promoted kernel assertions maintain state integrity.

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use kimberlite_types::{DataClass, Offset, Placement, Region, StreamId, StreamName, TenantId};

    use crate::command::Command;
    use crate::kernel::apply_committed;
    use crate::state::State;

    #[test]
    fn stream_creation_produces_correct_effects() {
        // Tests assertion: CreateStream must produce exactly 2 effects
        let state = State::new();
        let stream_id = StreamId::from_tenant_and_local(TenantId::new(1), 1);
        let cmd = Command::CreateStream {
            stream_id,
            stream_name: StreamName::from("test"),
            data_class: DataClass::PHI,
            placement: Placement::Region(Region::USEast1),
        };

        let (_, effects) = apply_committed(state, cmd).unwrap();

        // The kernel asserts this internally; we verify it here
        assert_eq!(
            effects.len(),
            2,
            "CreateStream must produce exactly 2 effects (metadata + audit)"
        );
    }

    #[test]
    fn stream_exists_after_creation() {
        // Tests assertion: stream must exist after creation
        let state = State::new();
        let stream_id = StreamId::from_tenant_and_local(TenantId::new(1), 1);
        let cmd = Command::CreateStream {
            stream_id,
            stream_name: StreamName::from("test"),
            data_class: DataClass::PHI,
            placement: Placement::Region(Region::USEast1),
        };

        let (new_state, _) = apply_committed(state, cmd).unwrap();

        // The kernel asserts this internally; we verify it here
        assert!(
            new_state.stream_exists(&stream_id),
            "stream must exist after creation"
        );
    }

    #[test]
    fn offset_never_decreases() {
        // Tests assertion: offset must never decrease (append-only)
        let state = State::new();
        let stream_id = StreamId::from_tenant_and_local(TenantId::new(1), 1);

        // Create stream
        let cmd1 = Command::CreateStream {
            stream_id,
            stream_name: StreamName::from("test"),
            data_class: DataClass::PHI,
            placement: Placement::Region(Region::USEast1),
        };
        let (state, _) = apply_committed(state, cmd1).unwrap();
        let offset0 = state.get_stream(&stream_id).unwrap().current_offset;

        // Append events
        let cmd2 = Command::AppendBatch {
            stream_id,
            events: vec![Bytes::from("event1")],
            expected_offset: offset0,
        };
        let (state, _) = apply_committed(state, cmd2).unwrap();
        let offset1 = state.get_stream(&stream_id).unwrap().current_offset;

        // Append more events
        let cmd3 = Command::AppendBatch {
            stream_id,
            events: vec![Bytes::from("event2"), Bytes::from("event3")],
            expected_offset: offset1,
        };
        let (state, _) = apply_committed(state, cmd3).unwrap();
        let offset2 = state.get_stream(&stream_id).unwrap().current_offset;

        // Offset must have increased (append-only guarantee)
        assert!(offset2 > offset1, "offset must never decrease");
        assert!(offset1 > offset0, "offset must never decrease");

        // Offset arithmetic must be correct
        assert_eq!(
            offset2,
            offset1 + Offset::from(2),
            "offset must equal base + event count"
        );
    }

    #[test]
    fn explicit_create_stream_does_not_collide_with_auto_allocated_backing_stream() {
        // Regression: before this fix, Command::CreateStream with an
        // explicit-id stream did not advance State::next_stream_id, and the
        // next Command::CreateTable's auto-allocated backing stream could
        // land on the same slot — the two streams would share storage and
        // every append on either would see events belonging to the other.
        //
        // The concrete failure this pins down:
        //   1. Tenant 0 creates an `identity_events` stream via
        //      CreateStream with explicit id = (0 << 32) | 1 = StreamId(1).
        //   2. Tenant 0 later creates a projection table whose backing
        //      stream is allocated via `with_new_stream` starting from the
        //      unchanged `next_stream_id = StreamId(1)`.
        //   3. That auto-allocation would have clobbered the explicit
        //      stream's metadata in place.
        use crate::command::{ColumnDefinition, Command, TableId};
        use bytes::Bytes;

        let state = State::new();

        // Step 1: explicit-id stream at the same slot the auto allocator
        // would otherwise pick next.
        let user_stream_id = StreamId::from_tenant_and_local(TenantId::new(0), 1);
        let (state, _) = apply_committed(
            state,
            Command::CreateStream {
                stream_id: user_stream_id,
                stream_name: StreamName::from("identity_events"),
                data_class: DataClass::PHI,
                placement: Placement::Region(Region::USEast1),
            },
        )
        .unwrap();
        assert!(state.stream_exists(&user_stream_id));

        // Step 2: create a table and observe that its backing stream lands
        // on a *different* slot.
        let table_id = TableId::new(42);
        let (state, _) = apply_committed(
            state,
            Command::CreateTable {
                tenant_id: TenantId::new(0),
                table_id,
                table_name: "t".to_string(),
                columns: vec![ColumnDefinition {
                    name: "id".to_string(),
                    data_type: "BIGINT".to_string(),
                    nullable: false,
                }],
                primary_key: vec!["id".to_string()],
            },
        )
        .unwrap();

        let table_stream_id = state.get_table(&table_id).expect("table registered").stream_id;
        assert_ne!(
            table_stream_id, user_stream_id,
            "CreateTable's auto-allocated backing stream must not collide with an earlier explicit-id stream"
        );
        assert!(state.stream_exists(&table_stream_id));
        // The original explicit-id stream still exists and is untouched.
        assert!(state.stream_exists(&user_stream_id));

        // Step 3: writes to the original stream must not bleed into the
        // table's backing stream or vice versa.
        let (state, _) = apply_committed(
            state,
            Command::AppendBatch {
                stream_id: user_stream_id,
                events: vec![Bytes::from("user-event")],
                expected_offset: Offset::ZERO,
            },
        )
        .unwrap();
        assert_eq!(
            state.get_stream(&user_stream_id).unwrap().current_offset,
            Offset::from(1),
        );
        assert_eq!(
            state.get_stream(&table_stream_id).unwrap().current_offset,
            Offset::ZERO,
            "table's backing stream must not see the user stream's append"
        );
    }

    // Summary of Promoted Kernel Assertions (4 total):
    //
    // 1. CreateStream - metadata stream_id matches command stream_id
    // 2. CreateStream - exactly 2 effects produced (audit completeness)
    // 3. CreateStream - stream exists after creation (postcondition)
    // 4. AppendBatch - offset never decreases (append-only guarantee)
    //    Also: offset arithmetic correctness (new = base + count)
    //
    // These postcondition assertions verify internal consistency and catch
    // bugs in the kernel logic. All are tested through normal operations above
    // and through property-based tests in the main test module.
}
