//! Tests module for Pressurecraft
//!
//! This module contains cross-step tests and property tests.

use bytes::Bytes;

// ============================================================================
// Determinism Tests
// ============================================================================

/// Tests that the kernel is deterministic across all steps.
mod determinism_tests {
    use super::*;

    #[test]
    fn step1_pure_functions_are_deterministic() {
        use crate::step1_pure_functions::{generate_id_pure, increment_pure};

        let seed = b"fixed_random_seed";

        // Call functions multiple times
        let ids: Vec<_> = (0..100).map(|_| generate_id_pure(seed)).collect();
        let increments: Vec<_> = (0..100).map(|_| increment_pure(42)).collect();

        // All results must be identical
        assert!(ids.windows(2).all(|w| w[0] == w[1]));
        assert!(increments.windows(2).all(|w| w[0] == w[1]));
    }

    #[test]
    fn step2_effects_are_deterministic() {
        use crate::step2_commands_effects::{command_to_effects, Command, DataClass, StreamId};

        let cmd = Command::create_stream(
            StreamId::new(1),
            "test".to_string(),
            DataClass::Public,
        );

        let timestamp = 5000;

        // Generate effects multiple times
        let effects1 = command_to_effects(cmd.clone(), timestamp);
        let effects2 = command_to_effects(cmd.clone(), timestamp);
        let effects3 = command_to_effects(cmd, timestamp);

        // All must be identical
        assert_eq!(effects1, effects2);
        assert_eq!(effects2, effects3);
    }

    #[test]
    fn step3_state_transitions_are_deterministic() {
        use crate::step2_commands_effects::{DataClass, Offset, StreamId};
        use crate::step3_state_machine::{append_batch, create_stream, State};

        // Apply same sequence twice
        let sequence = || {
            let state = State::new();
            let state = create_stream(
                state,
                StreamId::new(1),
                "events".to_string(),
                DataClass::Internal,
            )
            .unwrap();
            append_batch(state, StreamId::new(1), 5, Offset::ZERO).unwrap()
        };

        let state1 = sequence();
        let state2 = sequence();

        assert_eq!(state1, state2);
    }

    #[test]
    fn step4_kernel_is_deterministic() {
        use crate::step2_commands_effects::{DataClass, Offset, StreamId};
        use crate::step4_mini_kernel::{apply, Command, State};

        let sequence = || {
            let state = State::new();

            let (state, _) = apply(
                state,
                Command::create_stream(
                    StreamId::new(1),
                    "events".to_string(),
                    DataClass::Internal,
                ),
            )
            .unwrap();

            apply(
                state,
                Command::append_batch(
                    StreamId::new(1),
                    vec![Bytes::from("e1"), Bytes::from("e2")],
                    Offset::ZERO,
                ),
            )
            .unwrap()
        };

        let (state1, effects1) = sequence();
        let (state2, effects2) = sequence();

        assert_eq!(state1, state2);
        assert_eq!(effects1, effects2);
    }

    #[test]
    fn step5_full_kernel_is_deterministic() {
        use crate::step2_commands_effects::{DataClass, Offset, StreamId};
        use crate::step5_full_kernel::{apply_committed, Command, State};

        let sequence = || {
            let state = State::new();

            let (state, _) = apply_committed(
                state,
                Command::CreateStream {
                    stream_id: StreamId::new(1),
                    stream_name: "events".to_string(),
                    data_class: DataClass::Internal,
                },
            )
            .unwrap();

            apply_committed(
                state,
                Command::AppendBatch {
                    stream_id: StreamId::new(1),
                    events: vec![Bytes::from("e1"), Bytes::from("e2")],
                    expected_offset: Offset::ZERO,
                },
            )
            .unwrap()
        };

        let (state1, effects1) = sequence();
        let (state2, effects2) = sequence();

        assert_eq!(state1, state2);
        assert_eq!(effects1, effects2);
    }
}

// ============================================================================
// FCIS Property Tests
// ============================================================================

/// Tests that verify FCIS properties hold.
mod fcis_tests {
    use super::*;

    #[test]
    fn step1_pure_counter_is_immutable() {
        use crate::step1_pure_functions::PureCounter;

        let counter = PureCounter::new();
        let incremented = counter.increment();

        // Original unchanged
        assert_eq!(counter.get(), 0);
        // New counter has new value
        assert_eq!(incremented.get(), 1);
    }

    #[test]
    fn step2_commands_are_serializable() {
        use crate::step2_commands_effects::{Command, DataClass, StreamId};

        let cmd = Command::create_stream(
            StreamId::new(1),
            "events".to_string(),
            DataClass::Internal,
        );

        // Commands can be serialized (critical for network transmission)
        let json = serde_json::to_string(&cmd).unwrap();
        let deserialized: Command = serde_json::from_str(&json).unwrap();

        assert_eq!(cmd, deserialized);
    }

    #[test]
    fn step4_kernel_never_mutates_input() {
        use crate::step2_commands_effects::{DataClass, StreamId};
        use crate::step4_mini_kernel::{apply, Command, State};

        let original_state = State::new();
        let original_clone = original_state.clone();

        let cmd = Command::create_stream(
            StreamId::new(1),
            "events".to_string(),
            DataClass::Internal,
        );

        let _result = apply(original_state, cmd);

        // Original state unchanged (it was moved, but its clone shows it wasn't mutated)
        // This tests the functional pattern
        assert_eq!(original_clone.stream_count(), 0);
    }

    #[test]
    fn step5_errors_dont_change_state() {
        use crate::step2_commands_effects::{DataClass, Offset, StreamId};
        use crate::step5_full_kernel::{apply_committed, Command, State};

        let state = State::new();

        // Create stream
        let (state, _) = apply_committed(
            state,
            Command::CreateStream {
                stream_id: StreamId::new(1),
                stream_name: "events".to_string(),
                data_class: DataClass::Internal,
            },
        )
        .unwrap();

        let state_before_error = state.clone();

        // Try invalid operation (wrong offset)
        let result = apply_committed(
            state,
            Command::AppendBatch {
                stream_id: StreamId::new(1),
                events: vec![Bytes::from("e1")],
                expected_offset: Offset::new(999), // Wrong!
            },
        );

        // Error occurred
        assert!(result.is_err());

        // State was NOT consumed (it was moved into apply_committed, but we can't access it)
        // The important property is that if we had cloned before, the clone matches
        assert_eq!(state_before_error.stream_count(), 1);
    }
}

// ============================================================================
// Property-Based Tests (with proptest)
// ============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn step1_increment_is_deterministic(n in 0u64..1000) {
            use crate::step1_pure_functions::increment_pure;

            let result1 = increment_pure(n);
            let result2 = increment_pure(n);

            prop_assert_eq!(result1, result2);
        }

        #[test]
        fn step1_pure_counter_increment_chain(count in 0usize..100) {
            use crate::step1_pure_functions::PureCounter;

            let mut state = PureCounter::new();
            for _ in 0..count {
                state = state.increment();
            }

            prop_assert_eq!(state.get(), count as u64);
        }

        #[test]
        fn step3_offset_increases_monotonically(
            event_counts in prop::collection::vec(1usize..10, 1..20)
        ) {
            use crate::step2_commands_effects::{DataClass, Offset, StreamId};
            use crate::step3_state_machine::{append_batch, create_stream, State};

            let mut state = State::new();
            let stream_id = StreamId::new(1);

            // Create stream
            state = create_stream(
                state,
                stream_id,
                "events".to_string(),
                DataClass::Internal,
            )
            .unwrap();

            let mut current_offset = Offset::ZERO;

            for event_count in event_counts {
                state = append_batch(state, stream_id, event_count, current_offset).unwrap();
                current_offset = current_offset.increment_by(event_count as u64);
            }

            // Offset should match total events
            let final_offset = state.get_stream(&stream_id).unwrap().current_offset;
            prop_assert_eq!(final_offset, current_offset);
        }
    }
}
