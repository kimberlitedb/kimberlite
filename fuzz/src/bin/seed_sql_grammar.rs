//! Writes 500 grammar-generated SQL statements to
//! `fuzz/corpus/fuzz_sql_grammar/` as the initial libFuzzer seed
//! corpus for `fuzz_sql_grammar`, `fuzz_sql_norec`, and
//! `fuzz_sql_pqs`.
//!
//! Invoked by `just fuzz-seed-sql-grammar`. Not wired into the
//! normal build — this is a one-shot utility whose output is
//! checked into git and shipped to the EPYC box via
//! `just fuzz-epyc-deploy`.

use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("corpus")
        .join("fuzz_sql_grammar");
    std::fs::create_dir_all(&out_dir).expect("create corpus directory");

    let mut written = 0usize;
    for seed in 0u64..500 {
        // libFuzzer treats every file in the corpus directory as an
        // input. The grammar target reads the input as a `u64` seed,
        // so each corpus entry is the 8 raw bytes of the seed. This
        // also works as seed input for `fuzz_sql_norec` and
        // `fuzz_sql_pqs`, which accept arbitrary `&[u8]` and will
        // mutate freely from there.
        let path = out_dir.join(format!("{seed:016x}.bin"));
        std::fs::write(&path, seed.to_le_bytes()).expect("write seed file");
        written += 1;
    }
    println!("Wrote {written} seeds to {}", out_dir.display());
}
