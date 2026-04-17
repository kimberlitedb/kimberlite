#![no_main]

use std::collections::HashMap;

use bytes::Bytes;
use libfuzzer_sys::fuzz_target;

use kimberlite_types::{DataClass, Offset, Placement, StreamId, StreamName};
use kmb_kernel::command::{ColumnDefinition, Command, TableId};
use kmb_kernel::effects::Effect;
use kmb_kernel::kernel::apply_committed;
use kmb_kernel::state::State;

/// Model-side mirror of kernel state used as an oracle. Tracks only the
/// invariants this fuzz target asserts on; not a full replica of `State`.
#[derive(Default)]
struct Model {
    stream_offsets: HashMap<StreamId, u64>,
    tables: HashMap<TableId, String>,
}

/// Build a Command from raw bytes. For `AppendBatch`, prefer the model's
/// tracked offset most of the time (so the success path is exercised), but
/// occasionally force a mismatch so the error path is covered too.
fn command_from_bytes(data: &[u8], model: &Model) -> Option<Command> {
    if data.is_empty() {
        return None;
    }

    let variant = data[0] % 7;
    let rest = &data[1..];

    match variant {
        0 => {
            if rest.len() < 10 {
                return None;
            }
            let stream_id = u64::from_le_bytes(rest[0..8].try_into().ok()?);
            let name_len = (rest[8] as usize % 32).min(rest.len().saturating_sub(10));
            let name_bytes = &rest[10..10 + name_len];
            let name_str = std::str::from_utf8(name_bytes).unwrap_or("s");
            let data_class = match rest[9] % 4 {
                0 => DataClass::Public,
                1 => DataClass::PII,
                2 => DataClass::Confidential,
                _ => DataClass::PHI,
            };
            Some(Command::CreateStream {
                stream_id: StreamId::new(stream_id),
                stream_name: StreamName::new(name_str),
                data_class,
                placement: Placement::Global,
            })
        }
        1 => {
            if rest.len() < 18 {
                return None;
            }
            let stream_choice = rest[0] as usize;
            let num_events = (rest[16] as usize % 4) + 1;
            let mismatch_mask = rest[17] & 0x03;

            // Prefer an existing stream from the model; fall back to fuzz-derived
            // ID when no streams exist yet.
            let (stream_id, tracked_offset) = if !model.stream_offsets.is_empty() {
                let idx = stream_choice % model.stream_offsets.len();
                let (sid, off) = model
                    .stream_offsets
                    .iter()
                    .nth(idx)
                    .expect("index in range");
                (*sid, *off)
            } else {
                let raw = u64::from_le_bytes(rest[0..8].try_into().ok()?);
                (StreamId::new(raw), 0)
            };

            // Occasionally override the offset to test the stale-offset error path.
            let expected_offset = if mismatch_mask == 0 {
                Offset::new(tracked_offset)
            } else {
                Offset::new(tracked_offset.wrapping_add(u64::from(mismatch_mask)))
            };

            let event_data = &rest[18..];
            let chunk_size = if event_data.is_empty() {
                0
            } else {
                (event_data.len() / num_events).max(1)
            };

            let mut events = Vec::with_capacity(num_events);
            for i in 0..num_events {
                let start = i * chunk_size;
                let end = ((i + 1) * chunk_size).min(event_data.len());
                if start < event_data.len() {
                    events.push(Bytes::copy_from_slice(&event_data[start..end]));
                } else {
                    events.push(Bytes::from_static(b"fuzz"));
                }
            }

            Some(Command::AppendBatch {
                stream_id,
                events,
                expected_offset,
            })
        }
        2 => {
            if rest.len() < 9 {
                return None;
            }
            let table_id = u64::from_le_bytes(rest[0..8].try_into().ok()?);
            let name_len = (rest[8] as usize % 16).min(rest.len().saturating_sub(9));
            let name_bytes = &rest[9..9 + name_len];
            let table_name = std::str::from_utf8(name_bytes).unwrap_or("t").to_string();
            Some(Command::CreateTable {
                table_id: TableId(table_id),
                table_name,
                columns: vec![ColumnDefinition {
                    name: "id".to_string(),
                    data_type: "BIGINT".to_string(),
                    nullable: false,
                }],
                primary_key: vec!["id".to_string()],
            })
        }
        3 => {
            if rest.len() < 8 {
                return None;
            }
            let table_id = u64::from_le_bytes(rest[0..8].try_into().ok()?);
            Some(Command::DropTable {
                table_id: TableId(table_id),
            })
        }
        4 => {
            if rest.len() < 8 {
                return None;
            }
            let table_id = u64::from_le_bytes(rest[0..8].try_into().ok()?);
            let row_data = Bytes::copy_from_slice(&rest[8..]);
            Some(Command::Insert {
                table_id: TableId(table_id),
                row_data,
            })
        }
        5 => {
            if rest.len() < 8 {
                return None;
            }
            let table_id = u64::from_le_bytes(rest[0..8].try_into().ok()?);
            let row_data = Bytes::copy_from_slice(&rest[8..]);
            Some(Command::Update {
                table_id: TableId(table_id),
                row_data,
            })
        }
        _ => {
            if rest.len() < 8 {
                return None;
            }
            let table_id = u64::from_le_bytes(rest[0..8].try_into().ok()?);
            let row_data = Bytes::copy_from_slice(&rest[8..]);
            Some(Command::Delete {
                table_id: TableId(table_id),
                row_data,
            })
        }
    }
}

fuzz_target!(|data: &[u8]| {
    // Drive a sequence of commands through the kernel and assert model-based
    // invariants after each step:
    //
    //   * Determinism: applying the same (state, command) twice is bit-identical.
    //   * CreateStream: stream_exists becomes true; initial offset is 0.
    //   * AppendBatch: success ⇒ effect base_offset equals the tracked offset,
    //                    stream's current_offset advances by events.len().
    //   * CreateTable / DropTable: table_exists reflects the transition.

    if data.len() < 2 {
        return;
    }

    let num_commands = (data[0] as usize % 8) + 1;
    let cmd_data = &data[1..];

    let mut state = State::new();
    let mut model = Model::default();

    let chunk_size = if cmd_data.is_empty() {
        return;
    } else {
        (cmd_data.len() / num_commands).max(1)
    };

    for i in 0..num_commands {
        let start = i * chunk_size;
        let end = ((i + 1) * chunk_size).min(cmd_data.len());
        if start >= cmd_data.len() {
            break;
        }

        let cmd = match command_from_bytes(&cmd_data[start..end], &model) {
            Some(c) => c,
            None => continue,
        };

        // Determinism check: apply twice from a clone and compare.
        let (a_state, a_effects) = match apply_committed(state.clone(), cmd.clone()) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let (b_state, b_effects) = apply_committed(state.clone(), cmd.clone())
            .expect("deterministic success on repeat");
        assert_eq!(a_state, b_state, "apply_committed must be deterministic");
        assert_eq!(a_effects, b_effects, "effects must be deterministic");

        match &cmd {
            Command::CreateStream { stream_id, .. } => {
                assert!(
                    a_state.stream_exists(stream_id),
                    "CreateStream must make stream_exists true"
                );
                let meta = a_state
                    .get_stream(stream_id)
                    .expect("stream just created");
                assert_eq!(
                    meta.current_offset.as_u64(),
                    0,
                    "newly created stream starts at offset 0"
                );
                model.stream_offsets.insert(*stream_id, 0);
            }
            Command::AppendBatch {
                stream_id,
                events,
                expected_offset,
            } => {
                let tracked = model
                    .stream_offsets
                    .get(stream_id)
                    .copied()
                    .expect("AppendBatch success implies the stream exists in model");
                assert_eq!(
                    expected_offset.as_u64(),
                    tracked,
                    "AppendBatch success implies expected_offset == tracked_offset"
                );

                let append = a_effects.iter().find_map(|e| match e {
                    Effect::StorageAppend {
                        stream_id: sid,
                        base_offset,
                        events: effect_events,
                    } if sid == stream_id => Some((base_offset, effect_events)),
                    _ => None,
                });
                let (base_offset, effect_events) =
                    append.expect("AppendBatch success must produce StorageAppend");
                assert_eq!(
                    base_offset.as_u64(),
                    expected_offset.as_u64(),
                    "effect base_offset must match expected_offset"
                );
                assert_eq!(
                    effect_events.len(),
                    events.len(),
                    "effect events count matches command"
                );

                let meta = a_state
                    .get_stream(stream_id)
                    .expect("stream still exists after append");
                let expected_new = tracked + events.len() as u64;
                assert_eq!(
                    meta.current_offset.as_u64(),
                    expected_new,
                    "stream offset must advance by events.len()"
                );
                model.stream_offsets.insert(*stream_id, expected_new);
            }
            Command::CreateTable {
                table_id,
                table_name,
                ..
            } => {
                assert!(
                    a_state.table_exists(table_id),
                    "CreateTable must make table_exists true"
                );
                model.tables.insert(*table_id, table_name.clone());
            }
            Command::DropTable { table_id } => {
                assert!(
                    !a_state.table_exists(table_id),
                    "DropTable must make table_exists false"
                );
                model.tables.remove(table_id);
            }
            _ => {}
        }

        state = a_state;
    }
});
