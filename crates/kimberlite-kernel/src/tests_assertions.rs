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
