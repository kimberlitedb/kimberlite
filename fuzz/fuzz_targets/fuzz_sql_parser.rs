#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Convert bytes to UTF-8 string (ignore invalid UTF-8)
    if let Ok(sql) = std::str::from_utf8(data) {
        // Try to parse the SQL - should never panic, only return Err
        let _ = kimberlite_query::parse_statement(sql);
    }
});
