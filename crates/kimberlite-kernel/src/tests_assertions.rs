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

        let table_stream_id = state
            .get_table(&table_id)
            .expect("table registered")
            .stream_id;
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

    // ------------------------------------------------------------------
    // ROADMAP v0.6.0 Tier 1 #5 — ALTER TABLE schema_version assertion
    // pairing (per `docs/internals/testing/assertions-inventory.md`).
    //
    // The four production `assert!()` calls in `kernel.rs:503..=539,
    // 582..=614` enforce schema_version monotonicity and column-count
    // arithmetic on the ALTER TABLE ADD/DROP COLUMN paths. The
    // pressurecraft policy is "every production assert gets a
    // should_panic test". The tests below force the preconditions
    // those asserts guard and confirm the process aborts with the
    // expected message.
    //
    // Note: the strict-monotonicity `assert!` at kernel.rs:508 and
    // :586 is a belt-and-braces check; the preceding `.checked_add(1)
    // .expect(...)` is what fires first when prior_version hits
    // u32::MAX. We pair the `.expect()` here because it is the
    // concrete violation surface an attacker/tenant could hit by
    // spamming u32::MAX ALTERs. Each paired test uses
    // `State::with_table_metadata` (crate-internal) to forge the
    // prior state directly — doing it via `apply_committed` would
    // take 4 billion iterations.
    // ------------------------------------------------------------------

    use crate::command::ColumnDefinition;
    use crate::state::TableMetadata;

    /// Helper: craft a TableMetadata at an arbitrary schema_version
    /// without going through `apply_committed`. Used only by the
    /// assertion-pairing tests below.
    fn forged_table_meta(schema_version: u32) -> TableMetadata {
        let tenant_id = TenantId::new(0);
        let stream_id = StreamId::from_tenant_and_local(tenant_id, 42);
        TableMetadata {
            tenant_id,
            table_id: crate::command::TableId::new(42),
            table_name: "forged".to_string(),
            columns: vec![ColumnDefinition {
                name: "id".to_string(),
                data_type: "BIGINT".to_string(),
                nullable: false,
            }],
            primary_key: vec!["id".to_string()],
            stream_id,
            schema_version,
        }
    }

    /// Pairs kernel.rs:503-505 — `schema_version.checked_add(1).expect(...)`.
    ///
    /// If a table's `schema_version` is already `u32::MAX`, the next
    /// ADD COLUMN must panic rather than wrap to zero (which would
    /// violate strict monotonicity and silently re-use a prior
    /// version). We synthesise the u32::MAX prior state because
    /// reaching it through 4B apply_committed calls is impractical.
    #[test]
    #[should_panic(expected = "schema_version overflow")]
    fn alter_table_add_column_u32_max_schema_version_panics() {
        use crate::command::Command;

        let mut state = State::new();
        // Seed `next_table_id` past 42 so `with_table_metadata` is
        // consistent with what `apply_committed` would produce next.
        let meta = forged_table_meta(u32::MAX);
        state = state.with_table_metadata(meta.clone());

        let _ = apply_committed(
            state,
            Command::AlterTableAddColumn {
                tenant_id: meta.tenant_id,
                table_id: meta.table_id,
                column: ColumnDefinition {
                    name: "overflow".to_string(),
                    data_type: "TEXT".to_string(),
                    nullable: true,
                },
            },
        );
    }

    /// Pairs kernel.rs:582-584 — same overflow guard on the DROP
    /// path. Every mutating ALTER on a u32::MAX table must panic;
    /// a DROP that wraps would corrupt schema_version history just
    /// as badly as an ADD that wraps.
    #[test]
    #[should_panic(expected = "schema_version overflow")]
    fn alter_table_drop_column_u32_max_schema_version_panics() {
        use crate::command::Command;

        // Table with two columns so the DROP can succeed *if* the
        // overflow guard is ever removed (the paired assertion must
        // fire first, not the column-count `assert_eq!`).
        let mut meta = forged_table_meta(u32::MAX);
        meta.columns.push(ColumnDefinition {
            name: "tombstone".to_string(),
            data_type: "TEXT".to_string(),
            nullable: true,
        });

        let mut state = State::new();
        state = state.with_table_metadata(meta.clone());

        let _ = apply_committed(
            state,
            Command::AlterTableDropColumn {
                tenant_id: meta.tenant_id,
                table_id: meta.table_id,
                column_name: "tombstone".to_string(),
            },
        );
    }

    /// Pairs kernel.rs:508-512 — post-arithmetic strict-monotonicity
    /// `assert!`. This is structurally a defence-in-depth check
    /// (the preceding `.checked_add(1)` already enforces
    /// `new > prior`), but it is still a production assertion and
    /// thus still requires a paired test under the policy.
    ///
    /// We exercise it positively here: every successful ADD must
    /// leave `post.schema_version > pre.schema_version`. A
    /// regression that silently drops the bump would panic on
    /// apply — which is exactly what the assert defends against.
    #[test]
    fn alter_table_schema_version_strictly_increasing_smoke() {
        use crate::command::Command;

        let state = State::new();
        let meta = forged_table_meta(1);
        let table_id = meta.table_id;
        let tenant_id = meta.tenant_id;
        let state = state.with_table_metadata(meta);

        let (next_state, _) = apply_committed(
            state,
            Command::AlterTableAddColumn {
                tenant_id,
                table_id,
                column: ColumnDefinition {
                    name: "email".to_string(),
                    data_type: "TEXT".to_string(),
                    nullable: true,
                },
            },
        )
        .expect("ADD COLUMN should succeed on schema_version=1");

        let post = next_state.get_table(&table_id).expect("table survived");
        assert!(
            post.schema_version > 1,
            "schema_version must be > 1 after ADD (was {})",
            post.schema_version,
        );
        assert_eq!(post.schema_version, 2, "ADD COLUMN must bump by exactly 1",);
    }

    // Summary of Promoted Kernel Assertions (4 original + 4 v0.6.0):
    //
    // 1. CreateStream - metadata stream_id matches command stream_id
    // 2. CreateStream - exactly 2 effects produced (audit completeness)
    // 3. CreateStream - stream exists after creation (postcondition)
    // 4. AppendBatch - offset never decreases (append-only guarantee)
    //    Also: offset arithmetic correctness (new = base + count)
    //
    // v0.6.0 Tier 1 #5 additions:
    // 5. AlterTableAddColumn - schema_version.checked_add(1) overflow
    // 6. AlterTableDropColumn - schema_version.checked_add(1) overflow
    // 7. AlterTableAddColumn - schema_version strictly increasing (smoke)
    // 8. AlterTableAddColumn - column-count grows by exactly 1 (via the
    //    main test module's `alter_table_add_column_bumps_...` test)
    //
    // These postcondition assertions verify internal consistency and catch
    // bugs in the kernel logic. All are tested through normal operations above
    // and through property-based tests in the main test module.
}
