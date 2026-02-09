#![no_main]

use libfuzzer_sys::fuzz_target;
use kimberlite_oracle::{DuckDbOracle, OracleRunner};

// SQL differential fuzzing target.
//
// This fuzzer tests the DuckDB oracle wrapper for crashes and panics when
// executing SQL queries. It generates a sequence of SQL operations from fuzz
// input and executes them through the oracle interface.
//
// **What it tests:**
// - DuckDB oracle wrapper doesn't panic on any SQL input
// - Query execution handles errors gracefully
// - Result conversion from DuckDB types to Kimberlite types is safe
//
// **Future enhancement:**
// When KimberliteOracle is fully implemented (Task #5), this will become a
// true differential fuzzer comparing Kimberlite vs DuckDB results.
//
// **Input format:**
// - Bytes are interpreted as a sequence of SQL operations
// - Byte 0: Number of operations (1-10)
// - For each operation:
//   - Byte: Operation type (0=CREATE, 1=INSERT, 2=SELECT, 3=UPDATE, 4=DELETE)
//   - Remaining bytes: Parameters for the operation

fuzz_target!(|data: &[u8]| {
    // Need at least 2 bytes: operation count + one operation
    if data.len() < 2 {
        return;
    }

    // Create a fresh DuckDB oracle for each fuzz iteration
    let mut oracle = match DuckDbOracle::new() {
        Ok(o) => o,
        Err(_) => return, // Skip this iteration if oracle creation fails
    };

    // Extract operation count (limit to 10 to avoid timeouts)
    let op_count = (data[0] as usize % 10).max(1);
    let mut offset = 1;

    // Execute a sequence of SQL operations
    for _ in 0..op_count {
        if offset >= data.len() {
            break;
        }

        let op_type = data[offset] % 5;
        offset += 1;

        match op_type {
            0 => {
                // CREATE TABLE with random columns
                let table_name = format!("t{}", data[offset % data.len()] % 10);
                let col_count = (data.get(offset + 1).unwrap_or(&2) % 5).max(1);

                let mut columns = Vec::new();
                for i in 0..col_count {
                    let col_type = match data.get(offset + 2 + i as usize).unwrap_or(&0) % 4 {
                        0 => "INTEGER",
                        1 => "TEXT",
                        2 => "REAL",
                        _ => "BOOLEAN",
                    };
                    columns.push(format!("c{} {}", i, col_type));
                }

                let sql = format!(
                    "CREATE TABLE IF NOT EXISTS {} ({})",
                    table_name,
                    columns.join(", ")
                );

                let _ = oracle.execute(&sql);
                offset += 2 + col_count as usize;
            }
            1 => {
                // INSERT random values
                let table_name = format!("t{}", data.get(offset).unwrap_or(&0) % 10);
                let value_count = (data.get(offset + 1).unwrap_or(&2) % 5).max(1);

                let mut values = Vec::new();
                for i in 0..value_count {
                    let val = data.get(offset + 2 + i as usize).unwrap_or(&42);
                    // Mix of different value types
                    let value = match val % 4 {
                        0 => format!("{}", i32::from(*val)),
                        1 => format!("'{}'", val),
                        2 => format!("{}.{}", val, val),
                        _ => if *val % 2 == 0 { "TRUE" } else { "FALSE" }.to_string(),
                    };
                    values.push(value);
                }

                let sql = format!(
                    "INSERT INTO {} VALUES ({})",
                    table_name,
                    values.join(", ")
                );

                let _ = oracle.execute(&sql);
                offset += 2 + value_count as usize;
            }
            2 => {
                // SELECT with various clauses
                let table_name = format!("t{}", data.get(offset).unwrap_or(&0) % 10);
                let clause_type = data.get(offset + 1).unwrap_or(&0) % 4;

                let sql = match clause_type {
                    0 => format!("SELECT * FROM {}", table_name),
                    1 => format!("SELECT COUNT(*) FROM {}", table_name),
                    2 => {
                        let limit = data.get(offset + 2).unwrap_or(&10);
                        format!("SELECT * FROM {} LIMIT {}", table_name, limit)
                    }
                    _ => {
                        let col_num = data.get(offset + 2).unwrap_or(&0) % 5;
                        format!("SELECT c{} FROM {}", col_num, table_name)
                    }
                };

                let _ = oracle.execute(&sql);
                offset += 3;
            }
            3 => {
                // UPDATE random rows
                let table_name = format!("t{}", data.get(offset).unwrap_or(&0) % 10);
                let col_num = data.get(offset + 1).unwrap_or(&0) % 5;
                let value = data.get(offset + 2).unwrap_or(&42);

                let sql = format!(
                    "UPDATE {} SET c{} = {} WHERE c0 < 100",
                    table_name, col_num, value
                );

                let _ = oracle.execute(&sql);
                offset += 3;
            }
            4 => {
                // DELETE random rows
                let table_name = format!("t{}", data.get(offset).unwrap_or(&0) % 10);
                let threshold = data.get(offset + 1).unwrap_or(&50);

                let sql = format!(
                    "DELETE FROM {} WHERE c0 > {}",
                    table_name, threshold
                );

                let _ = oracle.execute(&sql);
                offset += 2;
            }
            _ => unreachable!(),
        }
    }

    // Final verification query - ensure oracle can handle result extraction
    let _ = oracle.execute("SELECT 1 AS test_column");
});
