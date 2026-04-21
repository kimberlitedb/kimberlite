//! Kernel state management.
//!
//! The kernel maintains in-memory state that tracks all streams and their
//! current offsets. State transitions are done by taking ownership and
//! returning a new state (builder pattern).

use std::collections::BTreeMap;

use kimberlite_types::{
    DataClass, Offset, Placement, SealReason, StreamId, StreamMetadata, StreamName, TenantId,
};
use serde::{Deserialize, Serialize};

use crate::command::{ColumnDefinition, IndexId, TableId};
use crate::masking::MaskingPolicyRecord;

/// **AUDIT-2026-04 H-5** — record of a sealed tenant.
///
/// Storing the reason + seal-timestamp (nanoseconds from a sim or
/// production clock) alongside the sealed-tenant id lets later audit
/// queries reconstruct who sealed which tenant and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SealedTenantRecord {
    pub tenant_id: TenantId,
    pub reason: SealReason,
    pub sealed_at_ns: u64,
}

// ============================================================================
// Table Metadata
// ============================================================================

/// Metadata for a SQL table.
///
/// `tenant_id` is the owning tenant; the kernel enforces that any DDL/DML
/// command referencing this table carries the same tenant id. Tables with
/// the same `table_name` may coexist across different tenants.
///
/// `schema_version` is monotonically increased by every `AlterTable*`
/// command. Version 1 is the initial `CreateTable`; each subsequent
/// ADD/DROP COLUMN bumps the counter by exactly one. Readers that cache
/// row layouts key the cache on `(table_id, schema_version)` so a DDL
/// command invalidates every stale cache without a global flush.
/// Invariant: schema_version is strictly increasing per table across
/// kernel history; this is a pressurecraft-grade check enforced in
/// `apply_committed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableMetadata {
    pub tenant_id: TenantId,
    pub table_id: TableId,
    pub table_name: String,
    pub columns: Vec<ColumnDefinition>,
    pub primary_key: Vec<String>,
    /// Underlying stream that stores this table's events.
    pub stream_id: StreamId,
    /// Monotonic schema version. Starts at 1 on `CreateTable`, incremented
    /// by every `AlterTable*` command. Default for backward-compatible
    /// deserialization of pre-v0.5.0 state snapshots (which had no
    /// AlterTable and therefore always carried version 1).
    #[serde(default = "TableMetadata::initial_schema_version")]
    pub schema_version: u32,
}

impl TableMetadata {
    /// Schema version carried by a freshly-created table. Used as a
    /// `serde(default)` fallback so pre-v0.5.0 state snapshots deserialize
    /// without rewriting persisted catalog data.
    pub const fn initial_schema_version() -> u32 {
        1
    }
}

/// Metadata for a SQL index.
///
/// `tenant_id` mirrors the owning table's tenant for symmetry and to keep
/// any index-level reads scoped the same way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexMetadata {
    pub tenant_id: TenantId,
    pub index_id: IndexId,
    pub index_name: String,
    pub table_id: TableId,
    pub columns: Vec<String>,
}

// ============================================================================
// Kernel State
// ============================================================================

/// The kernel's in-memory state.
///
/// State uses a builder pattern - methods take ownership of `self`, mutate,
/// and return `self`. This supports the functional core pattern while
/// avoiding unnecessary clones of the internal `BTreeMap`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct State {
    // Event streams
    streams: BTreeMap<StreamId, StreamMetadata>,
    next_stream_id: StreamId,

    // SQL tables
    tables: BTreeMap<TableId, TableMetadata>,
    next_table_id: TableId,
    /// Tenant-scoped name index: two tenants can own tables of the same name.
    ///
    /// A prior global `BTreeMap<String, TableId>` silently collapsed
    /// different tenants' catalogs — the isolation leak this index exists
    /// to prevent.
    table_name_index: BTreeMap<(TenantId, String), TableId>,

    // SQL indexes
    indexes: BTreeMap<IndexId, IndexMetadata>,
    next_index_id: IndexId,

    /// **AUDIT-2026-04 H-5** — sealed tenants reject every mutating
    /// command (DDL / DML / CreateStream / AppendBatch) with
    /// [`crate::kernel::KernelError::TenantSealed`]. Reads are
    /// unaffected — the seal is a write-freeze, matching healthcare
    /// SOPs where forensic copies must see read-consistent state.
    ///
    /// `#[serde(default)]` so existing on-disk state snapshots from
    /// pre-H-5 builds still deserialize (they are taken as "no
    /// tenants sealed").
    #[serde(default)]
    sealed_tenants: BTreeMap<TenantId, SealedTenantRecord>,

    /// **v0.6.0 Tier 2 #7** — tenant-scoped masking policy catalogue.
    ///
    /// `(tenant_id, policy_name)` → compiled strategy + role guard.
    /// Populated by `Command::CreateMaskingPolicy`, removed by
    /// `Command::DropMaskingPolicy`. `#[serde(default)]` so
    /// pre-v0.6.0 snapshots deserialize as "no policies".
    #[serde(default)]
    masking_policies: BTreeMap<(TenantId, String), MaskingPolicyRecord>,

    /// **v0.6.0 Tier 2 #7** — per-column masking policy attachments.
    ///
    /// `(tenant_id, table_id, column_name)` → `policy_name`. One
    /// policy per column (detach-then-reattach to change). The
    /// referenced policy must still live in `masking_policies`; a
    /// `DROP MASKING POLICY` that would leave a dangling attachment
    /// is rejected at the command boundary (see
    /// `Command::DropMaskingPolicy`).
    #[serde(default)]
    masking_attachments: BTreeMap<(TenantId, TableId, String), String>,
}

impl State {
    /// Creates a new empty state.
    pub fn new() -> Self {
        Self::default()
    }

    // ========================================================================
    // AUDIT-2026-04 H-5 — tenant sealing surface
    // ========================================================================

    /// Returns `true` if `tenant_id` is currently sealed. All mutating
    /// handlers must consult this and reject with
    /// `KernelError::TenantSealed` if it returns true.
    pub fn is_tenant_sealed(&self, tenant_id: TenantId) -> bool {
        self.sealed_tenants.contains_key(&tenant_id)
    }

    /// Returns the sealed-tenant record if the tenant is sealed.
    pub fn sealed_tenant_record(&self, tenant_id: TenantId) -> Option<&SealedTenantRecord> {
        self.sealed_tenants.get(&tenant_id)
    }

    /// Total count of sealed tenants — used in tests and scenarios.
    pub fn sealed_tenant_count(&self) -> usize {
        self.sealed_tenants.len()
    }

    /// Mark a tenant as sealed. `pub(crate)` — external callers go
    /// through `apply_committed(Command::SealTenant { ... })`.
    pub(crate) fn with_sealed_tenant(
        mut self,
        tenant_id: TenantId,
        reason: SealReason,
        sealed_at_ns: u64,
    ) -> Self {
        self.sealed_tenants.insert(
            tenant_id,
            SealedTenantRecord {
                tenant_id,
                reason,
                sealed_at_ns,
            },
        );
        self
    }

    /// Remove a tenant's seal. `pub(crate)` per the same reasoning as
    /// `with_sealed_tenant`.
    pub(crate) fn with_unsealed_tenant(mut self, tenant_id: TenantId) -> Self {
        self.sealed_tenants.remove(&tenant_id);
        self
    }

    /// Returns the metadata for a stream, if it exists.
    pub fn get_stream(&self, id: &StreamId) -> Option<&StreamMetadata> {
        self.streams.get(id)
    }

    /// Returns true if a stream with the given ID exists.
    pub fn stream_exists(&self, id: &StreamId) -> bool {
        self.streams.contains_key(id)
    }

    /// Adds a stream and returns the updated state.
    ///
    /// Internal to the kernel - external code should use `apply_committed`
    /// which handles validation and effects.
    ///
    /// Also advances `next_stream_id` past any explicit id >= the counter's
    /// current value. Without this, a later `with_new_stream` could pick
    /// the exact slot the explicit-id stream landed on — the two would
    /// share backing storage and every append would see the other's events.
    pub(crate) fn with_stream(mut self, meta: StreamMetadata) -> Self {
        let taken = meta.stream_id;
        self.streams.insert(taken, meta);
        if taken >= self.next_stream_id {
            self.next_stream_id = taken + StreamId::new(1);
        }
        self
    }

    /// Updates a stream's offset and returns the updated state.
    ///
    /// If the stream doesn't exist, returns self unchanged.
    ///
    /// Internal to the kernel - external code should use `apply_committed`
    /// which handles validation and effects.
    pub(crate) fn with_updated_offset(mut self, id: StreamId, new_offset: Offset) -> Self {
        if let Some(stream) = self.streams.get_mut(&id) {
            stream.current_offset = new_offset;
        }
        self
    }

    /// Returns the number of streams in the state.
    pub fn stream_count(&self) -> usize {
        self.streams.len()
    }

    /// Returns the next stream ID that will be allocated.
    pub fn next_stream_id(&self) -> StreamId {
        self.next_stream_id
    }

    /// Returns a reference to all streams.
    ///
    /// Made `pub` so observability callers outside the kernel (e.g. the
    /// chaos probe surface on `kimberlite-server`) can iterate streams
    /// without cloning the full state — reads are immutable and cheap.
    pub fn streams(&self) -> &BTreeMap<StreamId, StreamMetadata> {
        &self.streams
    }

    /// Creates a new stream with an auto-allocated ID.
    ///
    /// This is atomic - the ID allocation and stream insertion happen together,
    /// making it impossible to allocate an ID without creating the stream.
    ///
    /// The allocator walks `next_stream_id` forward until it lands on an
    /// unused slot. That walk is necessary because explicit-id streams
    /// (created via `Command::CreateStream` with a caller-chosen StreamId —
    /// for example, the `(tenant_id << 32) | local_id` scheme tenants use
    /// for application-level streams) do NOT advance `next_stream_id`. A
    /// naked `streams.insert(next_stream_id, …)` could otherwise clobber
    /// an explicit-id stream that happened to land on the same slot, and
    /// the two streams would then share backing storage — user events
    /// interleave with DML events and every subsequent append on either
    /// stream sees an offset view of events written for the other.
    pub(crate) fn with_new_stream(
        mut self,
        stream_name: StreamName,
        data_class: DataClass,
        placement: Placement,
    ) -> (Self, StreamMetadata) {
        while self.streams.contains_key(&self.next_stream_id) {
            self.next_stream_id = self.next_stream_id + StreamId::new(1);
        }
        let stream_id = self.next_stream_id;
        self.next_stream_id = self.next_stream_id + StreamId::new(1);

        // Invariant: we never overwrite an existing slot. The skip-forward
        // loop above is what makes that true in the presence of
        // explicit-id `CreateStream` commands.
        debug_assert!(
            !self.streams.contains_key(&stream_id),
            "with_new_stream must allocate a fresh slot"
        );

        let meta = StreamMetadata::new(stream_id, stream_name, data_class, placement);
        self.streams.insert(stream_id, meta.clone());

        (self, meta)
    }

    // ========================================================================
    // Table Management
    // ========================================================================

    /// Returns true if a table with the given ID exists.
    pub fn table_exists(&self, id: &TableId) -> bool {
        self.tables.contains_key(id)
    }

    /// Returns true if the given tenant owns a table with this name.
    ///
    /// A prior `table_name_exists(name)` accessor was global and let tenant
    /// A's catalog leak into tenant B's lookup. Callers must scope by
    /// tenant; the only callers without a tenant context are checkpoint /
    /// serialization paths that iterate `tables()` directly.
    pub fn table_name_exists_for_tenant(&self, tenant_id: TenantId, name: &str) -> bool {
        self.table_name_index
            .contains_key(&(tenant_id, name.to_string()))
    }

    /// Returns the metadata for a table, if it exists.
    ///
    /// Note: this does NOT verify tenant ownership. Use the command-level
    /// tenant check in `apply_committed` before acting on the result, or
    /// call [`Self::table_by_tenant_name`] when starting from a name.
    pub fn get_table(&self, id: &TableId) -> Option<&TableMetadata> {
        self.tables.get(id)
    }

    /// Returns the table metadata for `(tenant_id, name)` if present.
    ///
    /// This is the correct accessor for application-layer DDL/DML: never
    /// iterate `tables()` globally searching by name.
    pub fn table_by_tenant_name(&self, tenant_id: TenantId, name: &str) -> Option<&TableMetadata> {
        let table_id = self.table_name_index.get(&(tenant_id, name.to_string()))?;
        self.tables.get(table_id)
    }

    /// Returns an iterator over tables owned by a single tenant.
    ///
    /// Backed by a range scan on `table_name_index`, then a lookup per id.
    /// Prefer this over `tables().iter().filter(...)` to make the tenant
    /// filter part of the type/contract, not an easy-to-forget closure.
    pub fn tables_for_tenant(&self, tenant_id: TenantId) -> impl Iterator<Item = &TableMetadata> {
        let start = (tenant_id, String::new());
        let end = (
            TenantId::from(u64::from(tenant_id).saturating_add(1)),
            String::new(),
        );
        self.table_name_index
            .range(start..end)
            .filter_map(move |((t, _), table_id)| {
                debug_assert_eq!(*t, tenant_id);
                self.tables.get(table_id)
            })
    }

    /// Returns a reference to all tables.
    ///
    /// Reserved for checkpoint/restore and kernel-internal iteration.
    /// Application-layer code must use [`Self::tables_for_tenant`] or
    /// [`Self::table_by_tenant_name`] — iterating globally and filtering
    /// by `meta.table_name` is the shape of the isolation bug this module
    /// was hardened against.
    pub fn tables(&self) -> &std::collections::BTreeMap<TableId, TableMetadata> {
        &self.tables
    }

    /// Adds a table with pre-set metadata and returns the updated state.
    ///
    /// Internal to the kernel - external code should use `apply_committed`.
    pub(crate) fn with_table_metadata(mut self, meta: TableMetadata) -> Self {
        self.table_name_index
            .insert((meta.tenant_id, meta.table_name.clone()), meta.table_id);
        self.tables.insert(meta.table_id, meta);
        self
    }

    /// Removes a table and returns the updated state.
    pub(crate) fn without_table(mut self, id: TableId) -> Self {
        if let Some(meta) = self.tables.remove(&id) {
            self.table_name_index
                .remove(&(meta.tenant_id, meta.table_name));
        }
        self
    }

    /// Returns the number of tables.
    pub fn table_count(&self) -> usize {
        self.tables.len()
    }

    /// Returns the next table ID that will be allocated.
    pub fn next_table_id(&self) -> TableId {
        self.next_table_id
    }

    /// Returns a reference to the table name index.
    pub fn table_name_index(&self) -> &BTreeMap<(TenantId, String), TableId> {
        &self.table_name_index
    }

    /// Returns the number of entries in the table name index.
    pub fn table_name_index_len(&self) -> usize {
        self.table_name_index.len()
    }

    // ========================================================================
    // Index Management
    // ========================================================================

    /// Returns true if an index with the given ID exists.
    pub fn index_exists(&self, id: &IndexId) -> bool {
        self.indexes.contains_key(id)
    }

    /// Returns the metadata for an index, if it exists.
    pub fn get_index(&self, id: &IndexId) -> Option<&IndexMetadata> {
        self.indexes.get(id)
    }

    /// Returns a reference to all indexes.
    pub fn indexes(&self) -> &std::collections::BTreeMap<IndexId, IndexMetadata> {
        &self.indexes
    }

    /// Adds an index and returns the updated state.
    pub(crate) fn with_index(mut self, meta: IndexMetadata) -> Self {
        self.indexes.insert(meta.index_id, meta);
        self
    }

    /// Returns the number of indexes.
    pub fn index_count(&self) -> usize {
        self.indexes.len()
    }

    /// Returns the next index ID that will be allocated.
    pub fn next_index_id(&self) -> IndexId {
        self.next_index_id
    }

    // ========================================================================
    // Masking Policy Catalogue (v0.6.0 Tier 2 #7)
    // ========================================================================

    /// Returns `true` if a masking policy with this name exists for the tenant.
    pub fn masking_policy_exists(&self, tenant_id: TenantId, name: &str) -> bool {
        self.masking_policies
            .contains_key(&(tenant_id, name.to_string()))
    }

    /// Looks up a masking policy by `(tenant_id, name)`.
    pub fn masking_policy(
        &self,
        tenant_id: TenantId,
        name: &str,
    ) -> Option<&MaskingPolicyRecord> {
        self.masking_policies
            .get(&(tenant_id, name.to_string()))
    }

    /// Iterates over every masking policy owned by this tenant.
    pub fn masking_policies_for_tenant(
        &self,
        tenant_id: TenantId,
    ) -> impl Iterator<Item = &MaskingPolicyRecord> {
        let start = (tenant_id, String::new());
        let end = (
            TenantId::from(u64::from(tenant_id).saturating_add(1)),
            String::new(),
        );
        self.masking_policies
            .range(start..end)
            .map(|(_, rec)| rec)
    }

    /// Returns the number of masking policies across every tenant.
    pub fn masking_policy_count(&self) -> usize {
        self.masking_policies.len()
    }

    /// Returns the attached policy name for a column, if any.
    pub fn masking_attachment(
        &self,
        tenant_id: TenantId,
        table_id: TableId,
        column_name: &str,
    ) -> Option<&str> {
        self.masking_attachments
            .get(&(tenant_id, table_id, column_name.to_string()))
            .map(String::as_str)
    }

    /// Returns `true` if any column in any table currently references the named policy.
    pub fn masking_policy_has_attachments(&self, tenant_id: TenantId, name: &str) -> bool {
        // Walk attachments for this tenant and match policy name. The
        // attachment map is small (O(columns-attached)) and policies
        // change rarely, so a linear scan is fine and keeps the state
        // representation compact.
        self.masking_attachments
            .iter()
            .any(|((t, _, _), pname)| *t == tenant_id && pname == name)
    }

    /// Iterates over every `(table_id, column_name, policy_name)` attachment
    /// for a tenant.
    pub fn masking_attachments_for_tenant(
        &self,
        tenant_id: TenantId,
    ) -> impl Iterator<Item = (TableId, &str, &str)> {
        self.masking_attachments.iter().filter_map(move |((t, tid, col), pname)| {
            if *t == tenant_id {
                Some((*tid, col.as_str(), pname.as_str()))
            } else {
                None
            }
        })
    }

    /// Inserts or replaces a masking policy record. `pub(crate)` so
    /// external callers go through `apply_committed(Command::CreateMaskingPolicy)`.
    pub(crate) fn with_masking_policy(mut self, rec: MaskingPolicyRecord) -> Self {
        let key = (rec.tenant_id, rec.name.clone());
        self.masking_policies.insert(key, rec);
        self
    }

    /// Removes a masking policy. `pub(crate)` — the kernel handler
    /// verifies no attachments reference it first.
    pub(crate) fn without_masking_policy(mut self, tenant_id: TenantId, name: &str) -> Self {
        self.masking_policies.remove(&(tenant_id, name.to_string()));
        self
    }

    /// Attaches `policy_name` to the given column. `pub(crate)`.
    pub(crate) fn with_masking_attachment(
        mut self,
        tenant_id: TenantId,
        table_id: TableId,
        column_name: String,
        policy_name: String,
    ) -> Self {
        self.masking_attachments
            .insert((tenant_id, table_id, column_name), policy_name);
        self
    }

    /// Detaches any masking policy from the given column. `pub(crate)`.
    pub(crate) fn without_masking_attachment(
        mut self,
        tenant_id: TenantId,
        table_id: TableId,
        column_name: &str,
    ) -> Self {
        self.masking_attachments
            .remove(&(tenant_id, table_id, column_name.to_string()));
        self
    }

    /// Returns a snapshot reference to the masking policy map. Used by
    /// checkpoint / state-hash code paths.
    pub fn masking_policies_snapshot(
        &self,
    ) -> &BTreeMap<(TenantId, String), MaskingPolicyRecord> {
        &self.masking_policies
    }

    /// Returns a snapshot reference to the attachment map.
    pub fn masking_attachments_snapshot(
        &self,
    ) -> &BTreeMap<(TenantId, TableId, String), String> {
        &self.masking_attachments
    }
}

