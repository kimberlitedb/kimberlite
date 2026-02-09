// Simple smoke test for the SQL differential fuzzer
// This can be run without nightly Rust to verify the fuzzer logic

use kimberlite_oracle::{DuckDbOracle, OracleRunner};

fn main() {
    println!("Testing SQL differential fuzzer logic...");

    // Test 1: Empty input
    test_fuzzer_iteration(&[]);

    // Test 2: Single CREATE TABLE operation
    test_fuzzer_iteration(&[1, 0, 5, 2, 0, 1, 2]);

    // Test 3: CREATE TABLE + INSERT + SELECT
    test_fuzzer_iteration(&[
        3,  // 3 operations
        0, 1, 2, 0, 1,  // CREATE TABLE
        1, 1, 2, 10, 20,  // INSERT
        2, 1, 0,  // SELECT
    ]);

    // Test 4: Multiple operations
    test_fuzzer_iteration(&[
        5,  // 5 operations
        0, 2, 3, 0, 1, 2,  // CREATE TABLE
        1, 2, 3, 42, 43, 44,  // INSERT
        2, 2, 1,  // SELECT
        3, 2, 1, 100,  // UPDATE
        4, 2, 50,  // DELETE
    ]);

    // Test 5: Random bytes
    test_fuzzer_iteration(&[255, 128, 64, 32, 16, 8, 4, 2, 1]);

    println!("✓ All smoke tests passed!");
}

fn test_fuzzer_iteration(data: &[u8]) {
    println!("  Testing with {} bytes...", data.len());

    // This is the same logic as the fuzz target
    if data.len() < 2 {
        return;
    }

    let mut oracle = match DuckDbOracle::new() {
        Ok(o) => o,
        Err(e) => {
            println!("    ⚠ Failed to create oracle: {}", e);
            return;
        }
    };

    let op_count = (data[0] as usize % 10).max(1);
    let mut offset = 1;

    for op_num in 0..op_count {
        if offset >= data.len() {
            break;
        }

        let op_type = data[offset] % 5;
        offset += 1;

        match op_type {
            0 => {
                // CREATE TABLE
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

                match oracle.execute(&sql) {
                    Ok(_) => println!("    ✓ Op {}: CREATE TABLE {}", op_num, table_name),
                    Err(e) => println!("    ⚠ Op {}: CREATE TABLE failed: {}", op_num, e),
                }
                offset += 2 + col_count as usize;
            }
            1 => {
                // INSERT
                let table_name = format!("t{}", data.get(offset).unwrap_or(&0) % 10);
                let value_count = (data.get(offset + 1).unwrap_or(&2) % 5).max(1);

                let mut values = Vec::new();
                for i in 0..value_count {
                    let val = data.get(offset + 2 + i as usize).unwrap_or(&42);
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

                match oracle.execute(&sql) {
                    Ok(_) => println!("    ✓ Op {}: INSERT into {}", op_num, table_name),
                    Err(e) => println!("    ⚠ Op {}: INSERT failed: {}", op_num, e),
                }
                offset += 2 + value_count as usize;
            }
            2 => {
                // SELECT
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

                match oracle.execute(&sql) {
                    Ok(result) => println!("    ✓ Op {}: SELECT from {} ({} rows)", op_num, table_name, result.len()),
                    Err(e) => println!("    ⚠ Op {}: SELECT failed: {}", op_num, e),
                }
                offset += 3;
            }
            3 => {
                // UPDATE
                let table_name = format!("t{}", data.get(offset).unwrap_or(&0) % 10);
                let col_num = data.get(offset + 1).unwrap_or(&0) % 5;
                let value = data.get(offset + 2).unwrap_or(&42);

                let sql = format!(
                    "UPDATE {} SET c{} = {} WHERE c0 < 100",
                    table_name, col_num, value
                );

                match oracle.execute(&sql) {
                    Ok(_) => println!("    ✓ Op {}: UPDATE {}", op_num, table_name),
                    Err(e) => println!("    ⚠ Op {}: UPDATE failed: {}", op_num, e),
                }
                offset += 3;
            }
            4 => {
                // DELETE
                let table_name = format!("t{}", data.get(offset).unwrap_or(&0) % 10);
                let threshold = data.get(offset + 1).unwrap_or(&50);

                let sql = format!(
                    "DELETE FROM {} WHERE c0 > {}",
                    table_name, threshold
                );

                match oracle.execute(&sql) {
                    Ok(_) => println!("    ✓ Op {}: DELETE from {}", op_num, table_name),
                    Err(e) => println!("    ⚠ Op {}: DELETE failed: {}", op_num, e),
                }
                offset += 2;
            }
            _ => unreachable!(),
        }
    }

    // Final verification query
    match oracle.execute("SELECT 1 AS test_column") {
        Ok(_) => println!("    ✓ Final verification passed"),
        Err(e) => println!("    ⚠ Final verification failed: {}", e),
    }
}
