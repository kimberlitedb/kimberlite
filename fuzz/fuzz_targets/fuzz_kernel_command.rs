#![no_main]

use bytes::Bytes;
use libfuzzer_sys::fuzz_target;

use kmb_kernel::command::{ColumnDefinition, Command, TableId};
use kmb_kernel::kernel::apply_committed;
use kmb_kernel::state::State;
use kimberlite_types::{DataClass, Offset, Placement, StreamId, StreamName};

/// Build a Command from raw bytes. Since Command doesn't derive Arbitrary,
/// we manually select a variant from the first byte and fill fields from
/// the remaining bytes.
fn command_from_bytes(data: &[u8]) -> Option<Command> {
    if data.is_empty() {
        return None;
    }

    let variant = data[0] % 7;
    let rest = &data[1..];

    match variant {
        // CreateStream
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
        // AppendBatch
        1 => {
            if rest.len() < 17 {
                return None;
            }
            let stream_id = u64::from_le_bytes(rest[0..8].try_into().ok()?);
            let expected_offset = u64::from_le_bytes(rest[8..16].try_into().ok()?);
            let num_events = (rest[16] as usize % 4) + 1;

            let mut events = Vec::with_capacity(num_events);
            let event_data = &rest[17..];
            let chunk_size = if event_data.is_empty() {
                0
            } else {
                (event_data.len() / num_events).max(1)
            };

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
                stream_id: StreamId::new(stream_id),
                events,
                expected_offset: Offset::new(expected_offset),
            })
        }
        // CreateTable
        2 => {
            if rest.len() < 9 {
                return None;
            }
            let table_id = u64::from_le_bytes(rest[0..8].try_into().ok()?);
            let name_len = (rest[8] as usize % 16).min(rest.len().saturating_sub(9));
            let name_bytes = &rest[9..9 + name_len];
            let table_name = std::str::from_utf8(name_bytes)
                .unwrap_or("t")
                .to_string();

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
        // DropTable
        3 => {
            if rest.len() < 8 {
                return None;
            }
            let table_id = u64::from_le_bytes(rest[0..8].try_into().ok()?);
            Some(Command::DropTable {
                table_id: TableId(table_id),
            })
        }
        // Insert
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
        // Update
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
        // Delete
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
    // Apply a sequence of fuzzed commands to kernel state.
    //
    // This tests:
    // - No panics on any command sequence
    // - Offset monotonicity after appends
    // - State machine consistency under arbitrary input
    // - Error handling for invalid commands (missing streams, wrong offsets)

    if data.len() < 2 {
        return;
    }

    let num_commands = (data[0] as usize % 8) + 1;
    let cmd_data = &data[1..];

    let mut state = State::new();
    let chunk_size = if cmd_data.is_empty() {
        return;
    } else {
        (cmd_data.len() / num_commands).max(1)
    };

    let mut prev_offsets: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();

    for i in 0..num_commands {
        let start = i * chunk_size;
        let end = ((i + 1) * chunk_size).min(cmd_data.len());
        if start >= cmd_data.len() {
            break;
        }

        let cmd = match command_from_bytes(&cmd_data[start..end]) {
            Some(c) => c,
            None => continue,
        };

        match apply_committed(state.clone(), cmd) {
            Ok((new_state, effects)) => {
                // Verify offset monotonicity for streams
                for effect in &effects {
                    if let kmb_kernel::effects::Effect::StorageAppend {
                        stream_id,
                        base_offset,
                        events,
                    } = effect
                    {
                        let sid: u64 = (*stream_id).into();
                        let new_end = base_offset.as_u64() + events.len() as u64;

                        if let Some(&prev) = prev_offsets.get(&sid) {
                            // Offsets must be monotonically increasing
                            assert!(
                                base_offset.as_u64() >= prev,
                                "Offset went backwards: {} < {} for stream {}",
                                base_offset.as_u64(),
                                prev,
                                sid
                            );
                        }
                        prev_offsets.insert(sid, new_end);
                    }
                }

                state = new_state;
            }
            Err(_) => {
                // Errors are expected for invalid commands — no panic is the goal
            }
        }
    }
});
