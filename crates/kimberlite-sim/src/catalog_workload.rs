//! DDL/DML workload generation for AUDIT-2026-04 C-3.
//!
//! The April-2026 audit found that `multi_tenant_isolation` scenario's
//! workload generator could not express a `CREATE TABLE`, `INSERT`, or
//! `DELETE` — the seven `OpType` variants covered only key/value-style
//! traffic (Read / Write / RMW / Scan / tx-control). This module
//! provides:
//!
//! - `CatalogOp` — DDL/DML workload variants. A sibling of the existing
//!   `OpType` because `OpType` is `#[derive(Copy)]` and cannot hold
//!   `NonEmptyVec` / `Bytes` payloads without a breaking refactor.
//! - `SimCatalog` — per-scenario state tracking `table_id → tenant_id`
//!   ownership. This is what lets `CatalogOperationApplied` events
//!   carry `table_tenant_id` so the C-2 checker can compare it to the
//!   command's claimed tenant.
//! - `translate_to_command` — 1:1 mapping `CatalogOp → Command`. Pure
//!   function, trivially unit-testable.
//! - `emit_isolation_events` — produces the `CatalogOperationApplied` /
//!   `DmlRowObserved` events the C-2 wire consumes.
//! - `CatalogWorkloadGenerator` — drives a mixed DDL/DML workload
//!   across N tenants for the `catalog_stress` VOPR scenario.
//!
//! # PRESSURECRAFT mapping
//!
//! - §3 Parse, don't validate: `CatalogOp` variants take
//!   `SqlIdentifier` / `NonEmptyVec` from `kimberlite-types::domain` at
//!   construction — a flat-string column name cannot survive to
//!   translation.
//! - §1 FCIS: `translate_to_command` and `emit_isolation_events` are
//!   pure; the generator is the impure shell that consumes an `SimRng`.
//! - §2 Illegal states: `SimCatalog::insert` only permits inserting
//!   table ownership; the `Option<TenantId>` return of `owner` is the
//!   type-level statement that a fabricated table_id cannot mask as a
//!   known one.

use crate::SimRng;
use crate::event::EventKind;
use bytes::Bytes;
use kimberlite_kernel::command::{ColumnDefinition, Command, IndexId, TableId};
use kimberlite_types::TenantId;
use kimberlite_types::domain::{NonEmptyVec, SqlIdentifier};
use std::collections::BTreeMap;

// ============================================================================
// CatalogOp — workload-level DDL/DML variants
// ============================================================================

/// A DDL/DML workload operation. Carries already-validated domain types
/// so a flat-string column name cannot reach translation.
#[derive(Debug, Clone)]
pub enum CatalogOp {
    CreateTable {
        tenant_id: TenantId,
        table_id: TableId,
        name: SqlIdentifier,
        columns: NonEmptyVec<ColumnDefinition>,
        primary_key: NonEmptyVec<String>,
    },
    DropTable {
        tenant_id: TenantId,
        table_id: TableId,
    },
    CreateIndex {
        tenant_id: TenantId,
        index_id: IndexId,
        table_id: TableId,
        name: SqlIdentifier,
        columns: NonEmptyVec<String>,
    },
    Insert {
        tenant_id: TenantId,
        table_id: TableId,
        row: Bytes,
    },
    Update {
        tenant_id: TenantId,
        table_id: TableId,
        row: Bytes,
    },
    Delete {
        tenant_id: TenantId,
        table_id: TableId,
        row: Bytes,
    },
}

impl CatalogOp {
    /// Tenant ID the command claims to come from.
    pub fn cmd_tenant_id(&self) -> TenantId {
        match self {
            CatalogOp::CreateTable { tenant_id, .. }
            | CatalogOp::DropTable { tenant_id, .. }
            | CatalogOp::CreateIndex { tenant_id, .. }
            | CatalogOp::Insert { tenant_id, .. }
            | CatalogOp::Update { tenant_id, .. }
            | CatalogOp::Delete { tenant_id, .. } => *tenant_id,
        }
    }

    /// Table ID this operation targets, if any. `CreateTable` returns
    /// `Some` because the op names a to-be-created table; the checker
    /// uses it to compare against `cmd_tenant_id` — but there's no
    /// existing owner to check against (see `emit_isolation_events`).
    pub fn target_table_id(&self) -> Option<TableId> {
        match self {
            CatalogOp::CreateTable { table_id, .. }
            | CatalogOp::DropTable { table_id, .. }
            | CatalogOp::CreateIndex { table_id, .. }
            | CatalogOp::Insert { table_id, .. }
            | CatalogOp::Update { table_id, .. }
            | CatalogOp::Delete { table_id, .. } => Some(*table_id),
        }
    }
}

// ============================================================================
// SimCatalog — in-sim table-ownership tracker
// ============================================================================

/// Tracks which tenant owns which `TableId` over the lifetime of a
/// scenario. Populated by `CreateTable` ops, consulted by the
/// checker-dispatch to determine the table's *owning* tenant when a
/// command claims to be from a different tenant.
#[derive(Debug, Default, Clone)]
pub struct SimCatalog {
    owners: BTreeMap<TableId, TenantId>,
}

impl SimCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `table_id` is owned by `tenant_id`.
    ///
    /// Returns the previous owner if any — useful for scenarios that
    /// want to assert uniqueness.
    pub fn insert(&mut self, table_id: TableId, tenant_id: TenantId) -> Option<TenantId> {
        self.owners.insert(table_id, tenant_id)
    }

    /// Remove ownership of `table_id`. Used by `DropTable`.
    pub fn remove(&mut self, table_id: TableId) -> Option<TenantId> {
        self.owners.remove(&table_id)
    }

    /// Look up the tenant that owns `table_id`, or `None` if the table
    /// is not known (newly created in this op, or a fabricated id).
    pub fn owner(&self, table_id: TableId) -> Option<TenantId> {
        self.owners.get(&table_id).copied()
    }

    pub fn len(&self) -> usize {
        self.owners.len()
    }

    pub fn is_empty(&self) -> bool {
        self.owners.is_empty()
    }
}

// ============================================================================
// Pure translation + event emission
// ============================================================================

/// Translate a `CatalogOp` into a `kimberlite_kernel::Command`.
///
/// Pure — no side effects. `NonEmptyVec` → `Vec` conversion is
/// infallible since the domain type guarantees non-emptiness; the
/// resulting `Command` is validly constructable by the kernel's
/// `apply_committed`.
pub fn translate_to_command(op: CatalogOp) -> Command {
    match op {
        CatalogOp::CreateTable {
            tenant_id,
            table_id,
            name,
            columns,
            primary_key,
        } => Command::CreateTable {
            tenant_id,
            table_id,
            table_name: name.original().to_string(),
            columns: columns.into_vec(),
            primary_key: primary_key.into_vec(),
        },
        CatalogOp::DropTable {
            tenant_id,
            table_id,
        } => Command::DropTable {
            tenant_id,
            table_id,
        },
        CatalogOp::CreateIndex {
            tenant_id,
            index_id,
            table_id,
            name,
            columns,
        } => Command::CreateIndex {
            tenant_id,
            index_id,
            table_id,
            index_name: name.original().to_string(),
            columns: columns.into_vec(),
        },
        CatalogOp::Insert {
            tenant_id,
            table_id,
            row,
        } => Command::Insert {
            tenant_id,
            table_id,
            row_data: row,
        },
        CatalogOp::Update {
            tenant_id,
            table_id,
            row,
        } => Command::Update {
            tenant_id,
            table_id,
            row_data: row,
        },
        CatalogOp::Delete {
            tenant_id,
            table_id,
            row,
        } => Command::Delete {
            tenant_id,
            table_id,
            row_data: row,
        },
    }
}

/// Produce the isolation events a `CatalogOp` should emit on the sim
/// bus, given the current catalog state.
///
/// Semantics:
/// - For `CreateTable`: emits no catalog-isolation event — there is no
///   prior owner to compare against. The scenario driver is expected
///   to call `SimCatalog::insert` *after* this emission, so subsequent
///   ops against the table can be isolation-checked.
/// - For every other DDL/DML variant: if the catalog knows the table's
///   owner, emits `CatalogOperationApplied { cmd, owner }`. Otherwise
///   (foreign or never-created table) the op is a no-op for
///   isolation — the real kernel would reject it for "table not
///   found" before any tenant check.
pub fn emit_isolation_events(op: &CatalogOp, catalog: &SimCatalog) -> Vec<EventKind> {
    let cmd_tenant_id = u64::from(op.cmd_tenant_id());
    let Some(table_id) = op.target_table_id() else {
        return vec![];
    };

    if matches!(op, CatalogOp::CreateTable { .. }) {
        // No prior owner; the checker has nothing to compare against.
        return vec![];
    }

    match catalog.owner(table_id) {
        Some(owner) => vec![EventKind::CatalogOperationApplied {
            cmd_tenant_id,
            table_tenant_id: u64::from(owner),
        }],
        None => vec![],
    }
}

// ============================================================================
// CatalogWorkloadGenerator — drives mixed DDL/DML across N tenants
// ============================================================================

/// Generates a mixed DDL/DML workload for the `catalog_stress`
/// scenario.
///
/// The generator maintains its own `SimCatalog` mirroring what the
/// scenario driver should apply — in production code the two are
/// separate (driver applies, generator plans), but the generator's
/// internal mirror lets it produce *valid* follow-up ops (e.g. only
/// `Insert` into tables that have been created).
pub struct CatalogWorkloadGenerator {
    num_tenants: u64,
    next_table_id: u64,
    next_index_id: u64,
    tables_per_tenant: BTreeMap<TenantId, Vec<TableId>>,
    /// Mirrors the driver's `SimCatalog`. Kept in sync as the
    /// generator plans each op.
    planned_catalog: SimCatalog,
}

impl CatalogWorkloadGenerator {
    pub fn new(num_tenants: u64) -> Self {
        Self {
            num_tenants: num_tenants.max(2), // isolation testing needs >=2 tenants
            next_table_id: 1,
            next_index_id: 1,
            tables_per_tenant: BTreeMap::new(),
            planned_catalog: SimCatalog::new(),
        }
    }

    /// Snapshot of the planned catalog. Scenarios can seed their
    /// driver's `SimCatalog` from this if they want the generator to
    /// drive a fresh sequence.
    pub fn planned_catalog(&self) -> &SimCatalog {
        &self.planned_catalog
    }

    /// Generate a single op. The mix is chosen by `rng` and biased so
    /// that early ops are CreateTable (to populate the catalog), while
    /// later ops favour DDL + DML on existing tables.
    pub fn next_op(&mut self, rng: &mut SimRng) -> CatalogOp {
        // If the catalog is too small, always create a table.
        if self.planned_catalog.len() < (self.num_tenants as usize) * 2 {
            return self.plan_create_table(rng);
        }

        // Uniform mix: 20% each variant among the 5 non-create arms,
        // plus occasional CreateTable for growth. Keep numbers coarse
        // to keep the scenario deterministic.
        let choice = rng.next_usize(100);
        match choice {
            0..=14 => self.plan_create_table(rng),
            15..=24 => self.plan_drop_table(rng),
            25..=39 => self.plan_create_index(rng),
            40..=69 => self.plan_insert(rng),
            70..=84 => self.plan_update(rng),
            _ => self.plan_delete(rng),
        }
    }

    fn plan_create_table(&mut self, rng: &mut SimRng) -> CatalogOp {
        let tenant_id = TenantId::new(rng.next_usize(self.num_tenants as usize) as u64);
        let table_id = TableId::new(self.next_table_id);
        self.next_table_id += 1;

        // SqlIdentifier accepts alphanumeric + underscore. Make it
        // unique-per-tenant so table-name-uniqueness tests can fire.
        let name_raw = format!("tbl_{}_{}", u64::from(tenant_id), table_id.0);
        let name = SqlIdentifier::try_new(&name_raw).expect("generated identifier is valid");

        let columns = NonEmptyVec::try_new(vec![
            ColumnDefinition {
                name: "id".to_string(),
                data_type: "BIGINT".to_string(),
                nullable: false,
            },
            ColumnDefinition {
                name: "data".to_string(),
                data_type: "TEXT".to_string(),
                nullable: true,
            },
        ])
        .expect("generator provides non-empty columns");
        let primary_key =
            NonEmptyVec::try_new(vec!["id".to_string()]).expect("generator provides non-empty pk");

        self.tables_per_tenant
            .entry(tenant_id)
            .or_default()
            .push(table_id);
        self.planned_catalog.insert(table_id, tenant_id);

        CatalogOp::CreateTable {
            tenant_id,
            table_id,
            name,
            columns,
            primary_key,
        }
    }

    fn plan_drop_table(&mut self, rng: &mut SimRng) -> CatalogOp {
        let (tenant_id, table_id) = self.pick_existing_table(rng);
        // Remove from the planned mirror; the scenario driver does the
        // same after applying.
        if let Some(v) = self.tables_per_tenant.get_mut(&tenant_id) {
            v.retain(|t| *t != table_id);
        }
        self.planned_catalog.remove(table_id);
        CatalogOp::DropTable {
            tenant_id,
            table_id,
        }
    }

    fn plan_create_index(&mut self, rng: &mut SimRng) -> CatalogOp {
        let (tenant_id, table_id) = self.pick_existing_table(rng);
        let index_id = IndexId::new(self.next_index_id);
        self.next_index_id += 1;
        let name_raw = format!("idx_{}_{}", u64::from(tenant_id), index_id.0);
        let name = SqlIdentifier::try_new(&name_raw).expect("generated identifier is valid");
        let columns = NonEmptyVec::try_new(vec!["id".to_string()]).expect("non-empty columns");
        CatalogOp::CreateIndex {
            tenant_id,
            index_id,
            table_id,
            name,
            columns,
        }
    }

    fn plan_insert(&mut self, rng: &mut SimRng) -> CatalogOp {
        let (tenant_id, table_id) = self.pick_existing_table(rng);
        let id: u64 = rng.next_u32() as u64;
        let row = Bytes::from(format!("{{\"id\":{id},\"data\":\"x\"}}"));
        CatalogOp::Insert {
            tenant_id,
            table_id,
            row,
        }
    }

    fn plan_update(&mut self, rng: &mut SimRng) -> CatalogOp {
        let (tenant_id, table_id) = self.pick_existing_table(rng);
        let id: u64 = rng.next_u32() as u64;
        let row = Bytes::from(format!("{{\"id\":{id},\"data\":\"y\"}}"));
        CatalogOp::Update {
            tenant_id,
            table_id,
            row,
        }
    }

    fn plan_delete(&mut self, rng: &mut SimRng) -> CatalogOp {
        let (tenant_id, table_id) = self.pick_existing_table(rng);
        let id: u64 = rng.next_u32() as u64;
        let row = Bytes::from(format!("{{\"id\":{id}}}"));
        CatalogOp::Delete {
            tenant_id,
            table_id,
            row,
        }
    }

    /// Pick a (tenant, table) pair from the catalog. Falls back to
    /// planning a CreateTable if somehow empty — the invariant is that
    /// the catalog is non-empty once `next_op` has run `num_tenants*2`
    /// times, but defensive against misuse.
    fn pick_existing_table(&mut self, rng: &mut SimRng) -> (TenantId, TableId) {
        let owners: Vec<(TableId, TenantId)> = self
            .tables_per_tenant
            .iter()
            .flat_map(|(tenant, ids)| ids.iter().map(move |id| (*id, *tenant)))
            .collect();
        if owners.is_empty() {
            // Extremely unlikely; should have been short-circuited by
            // the growth check in `next_op`. Defensive fallback.
            let tenant_id = TenantId::new(0);
            let table_id = TableId::new(self.next_table_id);
            self.next_table_id += 1;
            self.planned_catalog.insert(table_id, tenant_id);
            return (tenant_id, table_id);
        }
        let idx = rng.next_usize(owners.len());
        let (table_id, tenant_id) = owners[idx];
        (tenant_id, table_id)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_catalog(ops: &[(TableId, TenantId)]) -> SimCatalog {
        let mut c = SimCatalog::new();
        for (t, o) in ops {
            c.insert(*t, *o);
        }
        c
    }

    #[test]
    fn sim_catalog_tracks_ownership_and_removals() {
        let mut c = SimCatalog::new();
        let t1 = TableId::new(1);
        let t2 = TableId::new(2);
        assert!(c.is_empty());
        assert_eq!(c.insert(t1, TenantId::new(10)), None);
        assert_eq!(c.insert(t2, TenantId::new(20)), None);
        assert_eq!(c.len(), 2);
        assert_eq!(c.owner(t1), Some(TenantId::new(10)));
        assert_eq!(c.owner(t2), Some(TenantId::new(20)));
        assert_eq!(c.owner(TableId::new(99)), None);
        assert_eq!(c.remove(t1), Some(TenantId::new(10)));
        assert_eq!(c.owner(t1), None);
    }

    #[test]
    fn translate_create_table_preserves_all_fields() {
        let op = CatalogOp::CreateTable {
            tenant_id: TenantId::new(5),
            table_id: TableId::new(7),
            name: SqlIdentifier::try_new("users").unwrap(),
            columns: NonEmptyVec::try_new(vec![ColumnDefinition {
                name: "id".to_string(),
                data_type: "BIGINT".to_string(),
                nullable: false,
            }])
            .unwrap(),
            primary_key: NonEmptyVec::try_new(vec!["id".to_string()]).unwrap(),
        };

        match translate_to_command(op) {
            Command::CreateTable {
                tenant_id,
                table_id,
                table_name,
                columns,
                primary_key,
            } => {
                assert_eq!(tenant_id, TenantId::new(5));
                assert_eq!(table_id, TableId::new(7));
                assert_eq!(table_name.to_lowercase(), "users");
                assert_eq!(columns.len(), 1);
                assert_eq!(columns[0].name, "id");
                assert_eq!(primary_key, vec!["id".to_string()]);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn translate_insert_preserves_row_bytes() {
        let row = Bytes::from_static(b"{\"id\":1}");
        let op = CatalogOp::Insert {
            tenant_id: TenantId::new(1),
            table_id: TableId::new(1),
            row: row.clone(),
        };
        match translate_to_command(op) {
            Command::Insert { row_data, .. } => assert_eq!(row_data, row),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    /// AUDIT-2026-04 C-3 critical: CreateTable emits no isolation
    /// event (nothing to compare against); every other DDL/DML op
    /// against a known table emits exactly one.
    #[test]
    fn emit_isolation_events_create_table_is_silent() {
        let catalog = SimCatalog::new();
        let op = CatalogOp::CreateTable {
            tenant_id: TenantId::new(5),
            table_id: TableId::new(7),
            name: SqlIdentifier::try_new("t").unwrap(),
            columns: NonEmptyVec::try_new(vec![ColumnDefinition {
                name: "id".to_string(),
                data_type: "BIGINT".to_string(),
                nullable: false,
            }])
            .unwrap(),
            primary_key: NonEmptyVec::try_new(vec!["id".to_string()]).unwrap(),
        };
        assert!(emit_isolation_events(&op, &catalog).is_empty());
    }

    /// Same-tenant DML: emits the event, and when the C-2 checker
    /// consumes it, it must match.
    #[test]
    fn emit_isolation_events_same_tenant_insert_emits_matching_event() {
        let catalog = mk_catalog(&[(TableId::new(7), TenantId::new(5))]);
        let op = CatalogOp::Insert {
            tenant_id: TenantId::new(5),
            table_id: TableId::new(7),
            row: Bytes::from_static(b"{}"),
        };
        let events = emit_isolation_events(&op, &catalog);
        assert_eq!(events.len(), 1);
        match &events[0] {
            EventKind::CatalogOperationApplied {
                cmd_tenant_id,
                table_tenant_id,
            } => {
                assert_eq!(*cmd_tenant_id, 5);
                assert_eq!(*table_tenant_id, 5);
            }
            other => panic!("wrong event: {other:?}"),
        }
    }

    /// AUDIT-2026-04 C-3 core: cross-tenant DML produces a
    /// CatalogOperationApplied with mismatched tenants. When fed to
    /// the C-2 checker, it triggers a violation. This is the
    /// end-to-end matched-pair proof.
    #[test]
    fn emit_isolation_events_cross_tenant_insert_is_flagged_by_checker() {
        use crate::query_invariants::TenantIsolationChecker;

        let catalog = mk_catalog(&[(TableId::new(7), TenantId::new(5))]);
        let forged = CatalogOp::Insert {
            tenant_id: TenantId::new(9), // different from owner
            table_id: TableId::new(7),
            row: Bytes::from_static(b"{}"),
        };
        let events = emit_isolation_events(&forged, &catalog);
        assert_eq!(events.len(), 1);

        // Consume the event the same way vopr.rs's main loop does.
        let mut checker = TenantIsolationChecker::new();
        match &events[0] {
            EventKind::CatalogOperationApplied {
                cmd_tenant_id,
                table_tenant_id,
            } => {
                let result = checker.verify_catalog_isolation(*table_tenant_id, *cmd_tenant_id);
                assert!(
                    !result.is_ok(),
                    "cross-tenant event must surface as a violation in the C-2 checker",
                );
            }
            other => panic!("wrong event: {other:?}"),
        }
    }

    /// Op against a table the catalog has never seen: no event. This
    /// matches the real kernel's behaviour (it rejects the op for
    /// "table not found" before reaching the tenant check), so it's
    /// not a compliance concern and shouldn't produce noise.
    #[test]
    fn emit_isolation_events_unknown_table_is_silent() {
        let catalog = SimCatalog::new();
        let op = CatalogOp::Insert {
            tenant_id: TenantId::new(5),
            table_id: TableId::new(999),
            row: Bytes::from_static(b"{}"),
        };
        assert!(emit_isolation_events(&op, &catalog).is_empty());
    }

    #[test]
    fn generator_produces_valid_ops_across_multiple_tenants() {
        let mut generator = CatalogWorkloadGenerator::new(5);
        let mut rng = SimRng::new(42);

        // First ~10 ops must all be CreateTable (growth phase).
        for _ in 0..10 {
            let op = generator.next_op(&mut rng);
            assert!(
                matches!(op, CatalogOp::CreateTable { .. }),
                "growth phase should emit CreateTable, got {op:?}",
            );
        }
        assert!(generator.planned_catalog().len() >= 10);

        // Generate a bunch more and sanity-check variety.
        let mut variants = std::collections::BTreeSet::new();
        for _ in 0..500 {
            let op = generator.next_op(&mut rng);
            let name: &str = match op {
                CatalogOp::CreateTable { .. } => "create_table",
                CatalogOp::DropTable { .. } => "drop_table",
                CatalogOp::CreateIndex { .. } => "create_index",
                CatalogOp::Insert { .. } => "insert",
                CatalogOp::Update { .. } => "update",
                CatalogOp::Delete { .. } => "delete",
            };
            variants.insert(name);
        }
        // We should see at least 4 variants in 500 ops — the mix is
        // deterministic per seed, this locks in coverage.
        assert!(
            variants.len() >= 4,
            "generator should produce variety, got: {variants:?}",
        );
    }

    /// Determinism: same seed → same ops.
    #[test]
    fn generator_is_deterministic() {
        let mut g1 = CatalogWorkloadGenerator::new(3);
        let mut g2 = CatalogWorkloadGenerator::new(3);
        let mut r1 = SimRng::new(12345);
        let mut r2 = SimRng::new(12345);

        for _ in 0..50 {
            let o1 = g1.next_op(&mut r1);
            let o2 = g2.next_op(&mut r2);

            // Compare by round-tripping to Command — Command has
            // derived PartialEq.
            let c1 = translate_to_command(o1);
            let c2 = translate_to_command(o2);
            assert_eq!(c1, c2, "same seed must produce same command sequence");
        }
    }
}
