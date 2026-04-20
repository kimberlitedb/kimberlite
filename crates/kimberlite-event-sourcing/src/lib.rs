//! Event-sourcing toolkit for Kimberlite aggregates.
//!
//! AUDIT-2026-04 S2.4f — ports notebar's `repo-kit.ts`
//! event-sourcing primitives into a standalone SDK crate.
//! Apps building on Kimberlite's append-only log (healthcare
//! encounter streams, finance ledgers, legal matter histories)
//! repeatedly rebuild the same pattern:
//!
//! 1. Read every event on a stream.
//! 2. Filter to the aggregate instance.
//! 3. Fold through a reducer to get current state.
//! 4. Execute a command → produce new events.
//! 5. Append the new events with optimistic-concurrency check.
//! 6. Update projection + emit audit.
//!
//! This crate provides the trait + helpers for steps 1-3 and
//! 5 (replay + append-with-concurrency). Steps 4 and 6 stay at
//! the caller (they're domain-specific). Keeping the crate
//! Client-agnostic means it can be used with the sync `Client`,
//! the future `AsyncClient`, or any other reader/writer that
//! exposes the minimum trait.
//!
//! # Example
//!
//! ```
//! use kimberlite_event_sourcing::{Aggregate, replay};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! enum PatientEvent {
//!     Registered { id: u64, name: String },
//!     Renamed { id: u64, new_name: String },
//! }
//!
//! #[derive(Default, Clone)]
//! struct PatientState {
//!     name: Option<String>,
//! }
//!
//! struct Patient;
//! impl Aggregate for Patient {
//!     type Event = PatientEvent;
//!     type State = PatientState;
//!     fn apply(state: Self::State, event: &Self::Event) -> Self::State {
//!         match event {
//!             PatientEvent::Registered { name, .. } => PatientState {
//!                 name: Some(name.clone()),
//!             },
//!             PatientEvent::Renamed { new_name, .. } => PatientState {
//!                 name: Some(new_name.clone()),
//!             },
//!         }
//!     }
//! }
//!
//! // Byte-encoded events, as they come off a stream.
//! let bytes: Vec<Vec<u8>> = vec![
//!     serde_json::to_vec(&PatientEvent::Registered {
//!         id: 1,
//!         name: "Alice".into(),
//!     })
//!     .unwrap(),
//!     serde_json::to_vec(&PatientEvent::Renamed {
//!         id: 1,
//!         new_name: "Alice Smith".into(),
//!     })
//!     .unwrap(),
//! ];
//!
//! let state = replay::<Patient, _, _>(bytes.iter().map(|b| b.as_slice()), |_| true).unwrap();
//! assert_eq!(state.name.as_deref(), Some("Alice Smith"));
//! ```

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Event-sourced aggregate specification.
///
/// Minimal surface: define the event type, the state type, and
/// an `apply` fold. The state type must be `Default + Clone` so
/// `replay` can construct a starting state and return owned
/// copies.
pub trait Aggregate {
    type Event: Serialize + DeserializeOwned;
    type State: Default + Clone;

    /// Fold an event into state. Pure function — no I/O, no
    /// mutation beyond the returned value.
    fn apply(state: Self::State, event: &Self::Event) -> Self::State;
}

/// Replay a sequence of raw event bytes through an aggregate's
/// reducer.
///
/// Each event is JSON-decoded (via `serde_json`) into
/// `A::Event`. The `filter` callback runs after decoding — use
/// it to narrow the stream to a specific aggregate instance
/// (e.g. events whose `patient_id == 42`).
///
/// # Errors
///
/// Returns [`ReplayError::Decode`] the first time an event
/// fails to decode, with the event's stream index for debug.
/// Malformed events are fatal by design — silently skipping
/// them would let a write-side schema regression corrupt
/// replay state.
pub fn replay<'a, A, I, F>(events: I, filter: F) -> Result<A::State, ReplayError>
where
    A: Aggregate,
    I: IntoIterator<Item = &'a [u8]>,
    F: Fn(&A::Event) -> bool,
{
    let mut state = A::State::default();
    for (index, raw) in events.into_iter().enumerate() {
        let event: A::Event = serde_json::from_slice(raw)
            .map_err(|source| ReplayError::Decode { index, source })?;
        if !filter(&event) {
            continue;
        }
        state = A::apply(state, &event);
    }
    Ok(state)
}

/// Encode a batch of events to wire-ready bytes for append.
///
/// Byte-for-byte matches what [`replay`] will decode. Using the
/// same crate on both sides avoids schema drift between the
/// writer and the reader.
pub fn encode_events<A: Aggregate>(
    events: &[A::Event],
) -> Result<Vec<Vec<u8>>, ReplayError> {
    events
        .iter()
        .map(|e| serde_json::to_vec(e).map_err(ReplayError::Encode))
        .collect()
}

/// Replay error.
#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    /// Event at `index` in the stream failed to decode.
    #[error("failed to decode event at index {index}: {source}")]
    Decode {
        index: usize,
        #[source]
        source: serde_json::Error,
    },
    /// Event encode failed (caller-supplied event type).
    #[error("failed to encode event: {0}")]
    Encode(#[source] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, Clone)]
    enum CounterEvent {
        Inc,
        Dec,
        Set(i64),
    }

    #[derive(Default, Clone, Debug)]
    struct CounterState {
        value: i64,
    }

    struct Counter;
    impl Aggregate for Counter {
        type Event = CounterEvent;
        type State = CounterState;
        fn apply(state: Self::State, event: &Self::Event) -> Self::State {
            match event {
                CounterEvent::Inc => CounterState { value: state.value + 1 },
                CounterEvent::Dec => CounterState { value: state.value - 1 },
                CounterEvent::Set(v) => CounterState { value: *v },
            }
        }
    }

    fn enc(events: &[CounterEvent]) -> Vec<Vec<u8>> {
        events
            .iter()
            .map(|e| serde_json::to_vec(e).unwrap())
            .collect()
    }

    #[test]
    fn empty_stream_returns_default_state() {
        let bytes: Vec<Vec<u8>> = Vec::new();
        let s = replay::<Counter, _, _>(bytes.iter().map(Vec::as_slice), |_| true).unwrap();
        assert_eq!(s.value, 0);
    }

    #[test]
    fn folds_every_event_into_state() {
        let bytes = enc(&[CounterEvent::Inc, CounterEvent::Inc, CounterEvent::Dec]);
        let s = replay::<Counter, _, _>(bytes.iter().map(Vec::as_slice), |_| true).unwrap();
        assert_eq!(s.value, 1);
    }

    #[test]
    fn filter_skips_events() {
        let bytes = enc(&[
            CounterEvent::Inc,
            CounterEvent::Dec,
            CounterEvent::Inc,
        ]);
        // Only count Inc events → state.value == 2.
        let s = replay::<Counter, _, _>(
            bytes.iter().map(Vec::as_slice),
            |e| matches!(e, CounterEvent::Inc),
        )
        .unwrap();
        assert_eq!(s.value, 2);
    }

    #[test]
    fn set_overrides_prior_state() {
        let bytes = enc(&[CounterEvent::Inc, CounterEvent::Set(100), CounterEvent::Dec]);
        let s = replay::<Counter, _, _>(bytes.iter().map(Vec::as_slice), |_| true).unwrap();
        assert_eq!(s.value, 99);
    }

    #[test]
    fn decode_error_surfaces_index() {
        let mut bytes = enc(&[CounterEvent::Inc, CounterEvent::Inc]);
        bytes.push(b"{not valid json".to_vec());
        bytes.push(serde_json::to_vec(&CounterEvent::Dec).unwrap());
        let err = replay::<Counter, _, _>(bytes.iter().map(Vec::as_slice), |_| true).unwrap_err();
        match err {
            ReplayError::Decode { index, .. } => assert_eq!(index, 2),
            other => panic!("expected Decode error, got {other:?}"),
        }
    }

    #[test]
    fn encode_events_produces_roundtripable_bytes() {
        let original = vec![CounterEvent::Set(7), CounterEvent::Inc];
        let bytes = encode_events::<Counter>(&original).unwrap();
        let s = replay::<Counter, _, _>(bytes.iter().map(Vec::as_slice), |_| true).unwrap();
        assert_eq!(s.value, 8);
    }

    // AUDIT-2026-04 S3.7 — property tests for the replay fold.

    use proptest::prelude::*;

    fn counter_event_strategy() -> impl Strategy<Value = CounterEvent> {
        prop_oneof![
            Just(CounterEvent::Inc),
            Just(CounterEvent::Dec),
            (i64::MIN / 2..=i64::MAX / 2).prop_map(CounterEvent::Set),
        ]
    }

    proptest! {
        /// The fold is pure: running `replay` twice over the
        /// same bytes must produce the same state, regardless of
        /// event sequence.
        #[test]
        fn prop_replay_is_deterministic(
            events in prop::collection::vec(counter_event_strategy(), 0..50),
        ) {
            let bytes = enc(&events);
            let a = replay::<Counter, _, _>(
                bytes.iter().map(Vec::as_slice),
                |_| true,
            )
            .unwrap();
            let b = replay::<Counter, _, _>(
                bytes.iter().map(Vec::as_slice),
                |_| true,
            )
            .unwrap();
            prop_assert_eq!(a.value, b.value);
        }

        /// `encode` + `replay` round-trip preserves the fold
        /// result. A writer using `encode_events` and a reader
        /// using `replay` see identical state.
        #[test]
        fn prop_encode_replay_roundtrip(
            events in prop::collection::vec(counter_event_strategy(), 0..50),
        ) {
            let bytes = encode_events::<Counter>(&events).unwrap();
            let state = replay::<Counter, _, _>(
                bytes.iter().map(Vec::as_slice),
                |_| true,
            )
            .unwrap();
            // Compute the expected state by direct fold.
            let mut expected = CounterState::default();
            for e in &events {
                expected = Counter::apply(expected, e);
            }
            prop_assert_eq!(state.value, expected.value);
        }

        /// `filter(|_| true)` is equivalent to no filter; the
        /// resulting state depends only on the event sequence.
        #[test]
        fn prop_all_pass_filter_matches_direct_fold(
            events in prop::collection::vec(counter_event_strategy(), 0..30),
        ) {
            let bytes = enc(&events);
            let via_replay = replay::<Counter, _, _>(
                bytes.iter().map(Vec::as_slice),
                |_| true,
            )
            .unwrap();
            let direct = events
                .iter()
                .fold(CounterState::default(), |s, e| Counter::apply(s, e));
            prop_assert_eq!(via_replay.value, direct.value);
        }

        /// `filter(|_| false)` always produces the default state.
        #[test]
        fn prop_reject_all_filter_yields_default(
            events in prop::collection::vec(counter_event_strategy(), 0..30),
        ) {
            let bytes = enc(&events);
            let state = replay::<Counter, _, _>(
                bytes.iter().map(Vec::as_slice),
                |_| false,
            )
            .unwrap();
            prop_assert_eq!(state.value, 0);
        }
    }

    /// Pressurecraft §3 — assertion density: a pure `apply` fold
    /// is deterministic, so re-applying the same events must
    /// yield the same state. Pin it.
    #[test]
    fn replay_is_deterministic() {
        let bytes = enc(&[
            CounterEvent::Inc,
            CounterEvent::Inc,
            CounterEvent::Set(42),
            CounterEvent::Dec,
        ]);
        let s1 = replay::<Counter, _, _>(bytes.iter().map(Vec::as_slice), |_| true).unwrap();
        let s2 = replay::<Counter, _, _>(bytes.iter().map(Vec::as_slice), |_| true).unwrap();
        assert_eq!(s1.value, s2.value);
        assert_eq!(s1.value, 41);
    }
}
