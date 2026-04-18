#![no_main]
//! Grammar-driven SQL fuzzing.
//!
//! The byte-level `fuzz_sql_parser` target tokenizer-rejects ~99% of
//! mutations. This target swaps the byte input for a `u64` seed that
//! feeds `kimberlite_sim::sql_grammar::generate`, which emits
//! structurally valid SQL against a small fixed identifier pool. The
//! parser + planner + executor run; any panic is a bug.
//!
//! Execute errors are tolerated (the grammar sometimes emits
//! type-mismatched predicates the planner rejects) — the point is
//! to exercise these code paths under mutation, not to make every
//! statement succeed.

use kimberlite::{Kimberlite, TenantId};
use kimberlite_sim::sql_grammar;
use libfuzzer_sys::fuzz_target;
use tempfile::tempdir;

fuzz_target!(|seed: u64| {
    let sql = sql_grammar::generate(seed);

    let Ok(dir) = tempdir() else { return };
    let Ok(db) = Kimberlite::open(dir.path()) else {
        return;
    };
    let tenant = db.tenant(TenantId::new(1));

    // Best-effort bootstrap: most generated SELECTs reference the
    // `events` table that `Kimberlite::open` already registers, so
    // parse/plan coverage lands whether or not a matching CREATE TABLE
    // statement precedes the query. For statements that create their
    // own tables (CREATE TABLE, INSERT into fresh tables), the shared
    // process-wide state does the right thing.
    let _ = tenant.execute(&sql, &[]);
});
