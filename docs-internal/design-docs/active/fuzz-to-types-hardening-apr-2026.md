# Fuzz-to-Types Hardening Campaign — April 2026

**Status:** Shipped (v0.5.0).
**Audience:** Kimberlite maintainers.
**Prompted by:** First EPYC nightly fuzz campaign (April 2026) found
five real bugs in production code paths. Rather than patch each with a
conditional, the campaign converted each bug class into a type-system
guarantee so the bug becomes unrepresentable.

**Rationale:** `docs/concepts/pressurecraft.md` §6 — "Make illegal
states unrepresentable." Conditional guards are forgettable; type
boundaries are not.

## The five bugs and their type-level fixes

| Bug class              | Conditional fix                         | Type-level fix                                              |
|------------------------|-----------------------------------------|-------------------------------------------------------------|
| Zero-column CREATE TABLE | `if columns.is_empty() { error }`     | `ParsedCreateTable.columns: NonEmptyVec<ColumnDef>`         |
| Case-sensitive column match | `.to_lowercase()` at match sites    | `SqlIdentifier` case-folded at construction                 |
| LZ4 decompression-bomb | `if decompressed > MAX { error }`       | `BoundedSize<const MAX: usize>` as the codec's input type   |
| Out-of-range clearance level | `if level > 3 { error }`          | `ClearanceLevel` enum with `TryFrom<u8>`                    |
| Variant-hostile wire framing | Fuzz-rejected ~99% of inputs       | Structure-aware `fuzz_wire_typed` + `fuzz_vsr_typed` targets via `Arbitrary`-derived wrappers |

## Deliverables

### `kimberlite-types::domain` module

Four typed-domain primitives, all `#[must_use]`, all with paired
`#[should_panic]` tests for their production asserts:

- **`NonEmptyVec<T>`** — `Vec<T>` guaranteed ≥1 element. `try_new`
  rejects empty input. Used by `ParsedCreateTable.columns`.
- **`SqlIdentifier`** — case-folded + validated SQL identifier;
  `PartialEq` / `Hash` use the normalised form. Supports `*`,
  `prefix*`, `*suffix` patterns. Used by `ColumnFilter::matches`.
- **`BoundedSize<const MAX: usize>`** — `usize` wrapper with
  `TryFrom<u32>` / `TryFrom<u64>` rejecting over-bound values. Used
  by the LZ4 codec.
- **`ClearanceLevel`** — enum Public/Confidential/Secret/TopSecret with
  `#[repr(u8)]` + `TryFrom<u8>`. Replaces raw `u8` in
  `UserAttributes.clearance_level`.

### Structure-aware fuzz targets

- **`fuzz_wire_typed`** + **`fuzz_vsr_typed`** — `Arbitrary`-derived
  `Fuzzable*` wrappers live in the `fuzz/` crate so production crates
  stay free of the `arbitrary` dep. Coverage reaches handlers
  immediately instead of being framing-rejected at ~99% rate.
- **`fuzz_sql_grammar`** — seed-driven (`|seed: u64|`) weighted-CFG
  grammar producing structurally valid SELECT / INSERT / UPDATE /
  DELETE / CREATE TABLE. `MAX_DEPTH = 8` prevents stack-blowing
  predicate trees. Drives coverage past the tokenizer into the
  planner + executor.
- **`fuzz_sql_norec`** — NoREC metamorphic oracle. Compares
  `SELECT COUNT(*) WHERE p` against
  `SELECT SUM(CASE WHEN p THEN 1 ELSE 0 END)`. Disagreement points
  at WHERE-evaluator or NULL-propagation bugs.
- **`fuzz_sql_pqs`** — Pivoted Query Synthesis oracle. Picks a row,
  synthesises an equality predicate true for it by construction,
  asserts the pivot appears in the result.

### Persistent-mode fuzz infrastructure

- **`Kimberlite::reset_state`** + **`Storage::reset`** + **`BTreeStore::reset`**
  behind the `fuzz-reset` cargo feature. In-place zeroing of all
  persisted + in-memory state, targeting libFuzzer persistent mode.
  Production builds never enable the feature; it's opt-in via the
  fuzz crate only.
- **Persistent-mode rewrites** of `fuzz_sql_metamorphic`,
  `fuzz_rbac_injection`, `fuzz_sql_norec`, `fuzz_sql_pqs`. Each
  lazy-initialises a single Kimberlite via
  `once_cell::sync::Lazy<Mutex<(TempDir, Kimberlite)>>` and calls
  `reset_state()` between iterations instead of opening a fresh
  tempdir per input.
- **Measured throughput improvement on EPYC** (300s campaign,
  2026-04-18): `fuzz_sql_metamorphic` went from ~60 exec/s
  (tempdir-per-iteration baseline) to 69 exec/s — a **15-20%
  improvement, not the 50× the plan projected**. Root cause: the
  file-recreate reset path pays most of the syscall cost of a fresh
  tempdir + open. A true zero-reopen reset would require deeper
  surgery to the storage init path; deferred as a follow-up.

### UBSan nightly campaign

- `tools/fuzz/epyc/{nightly-ubsan.sh, kimberlite-fuzz-ubsan.service,
  kimberlite-fuzz-ubsan.timer}` — fires daily at 06:00 UTC (4h after
  ASan nightly). Doubles bug-class coverage — integer overflow / UB,
  the class that preceded the LZ4 OOM. Corpora shared with ASan so
  coverage discovered by one benefits the other.

### EPYC infra recipes

- `just fuzz-epyc-corpus-merge` — cross-target corpus union for
  discovering cross-domain interesting inputs.
- `just fuzz-epyc-coverage` — weekly `cargo fuzz coverage` report.
- Both manual; full systemd automation deferred.

## Outcomes

- Five bug classes made unrepresentable at the type level.
- First EPYC campaign's findings closed with no "patched-conditional"
  residue in production code.
- Fuzz coverage reaches planner + executor immediately instead of
  being framing-rejected.
- 20 fuzz targets in the nightly; 12 in `ci-fuzz.sh` smoke.
- UBSan as a second opinion to ASan, catching integer-overflow /
  alignment bugs the AddressSanitizer misses.

## Follow-up work (deferred)

- True zero-reopen fuzz reset path (would require refactoring
  `open_with_capacity` to support in-place re-init). Projected to
  push `fuzz_sql_metamorphic` from 69 exec/s → 1500+ exec/s.
- Coverage automation — systemd timer for the weekly coverage
  snapshot.
- Additional domain primitives as new bug classes surface.

## References

- `docs/concepts/pressurecraft.md` §6 — type-level bug elimination
  discipline.
- `crates/kimberlite-types/src/domain/` — the primitives.
- `fuzz/fuzz_targets/fuzz_wire_typed.rs`,
  `fuzz/fuzz_targets/fuzz_vsr_typed.rs`,
  `fuzz/fuzz_targets/fuzz_sql_*.rs` — targets.
- `tools/fuzz/epyc/` — nightly infra.
- Git log range: commits around the v0.5.0 tag.
