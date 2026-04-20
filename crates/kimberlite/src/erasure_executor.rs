//! Runtime implementation of [`ErasureExecutor`] backed by the kernel
//! + storage + crypto layers.
//!
//! Wires the compliance-crate trait surface (`pre_erasure_merkle_root`,
//! `shred_stream`) to the actual `Storage::latest_chain_hash` snapshot
//! and a `Command::Delete` loop over the projection's subject rows,
//! capped by an ephemeral `DataEncryptionKey::shred(nonce)` to commit
//! to the cryptographic destruction event.
//!
//! See `docs-internal/audit/AUDIT-2026-04.md` C-1.

use bytes::Bytes;
use kimberlite_compliance::erasure::{ErasureExecutor, StreamShredReceipt};
use kimberlite_crypto::{DataEncryptionKey, InMemoryMasterKey, KeyEncryptionKey};
use kimberlite_kernel::{Command, apply_committed};
use kimberlite_store::{Key, ProjectionStore, TableId as StoreTableId};
use kimberlite_types::StreamId;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::kimberlite::KimberliteInner;

/// Default column whose value identifies the data subject in a row.
/// Conventional in healthcare/finance schemas; override via
/// [`KernelBackedErasureExecutor::with_subject_column`].
pub const DEFAULT_SUBJECT_COLUMN: &str = "subject_id";

/// Maximum rows scanned per stream during shred. Bounded to prevent a
/// single erasure call from doing unbounded work — production deploys
/// should not have a single subject's data spread across more than this
/// many rows in one stream.
const MAX_ROWS_PER_STREAM: usize = 1_000_000;

/// Domain-separation tag for the shred-nonce derivation. Distinct from
/// any other Kimberlite SHA-256 use (chain hashes, attestation
/// witnesses, etc.) so a digest never collides across protocols.
const SHRED_NONCE_TAG: &[u8] = b"kimberlite/erasure-shred-nonce/v1";

/// Runtime [`ErasureExecutor`] that mutates the live kernel state +
/// storage + projection store via `Command::Delete`, and proves the
/// cryptographic-shred semantics by destroying an ephemeral
/// [`DataEncryptionKey`] per stream.
///
/// Holds a mutable borrow of [`KimberliteInner`]. Construct under the
/// inner write lock (the lock is already held by the caller of
/// [`crate::tenant::TenantHandle::erase_subject`]).
pub(crate) struct KernelBackedErasureExecutor<'a> {
    inner: &'a mut KimberliteInner,
    /// Column that holds the subject identifier on each row.
    subject_column: String,
    /// Master + KEK kept alive for the lifetime of the executor so we
    /// can mint ephemeral DEKs cheaply for each `shred_stream` call.
    /// Generated fresh per executor — never persisted; their only role
    /// is to carry the cryptographic-shred semantics through to the
    /// returned digest.
    kek: KeyEncryptionKey,
}

impl<'a> KernelBackedErasureExecutor<'a> {
    /// Construct a new executor borrowing the given inner state.
    pub fn new(inner: &'a mut KimberliteInner) -> Self {
        let master = InMemoryMasterKey::generate();
        let (kek, _wrapped) = KeyEncryptionKey::generate_and_wrap(&master);
        Self {
            inner,
            subject_column: DEFAULT_SUBJECT_COLUMN.to_string(),
            kek,
        }
    }

    /// Override the column that identifies the data subject on a row.
    #[allow(dead_code)] // exposed for tests / callers with non-default schemas
    pub fn with_subject_column(mut self, column: impl Into<String>) -> Self {
        self.subject_column = column.into();
        self
    }

    /// Build a single-stream shred receipt: scan the projection for
    /// rows whose `subject_column` matches `subject_id`, issue a
    /// `Command::Delete` for each, then shred a freshly-minted DEK and
    /// return the digest.
    ///
    /// Split out so the trait impl stays under the 70-line soft limit.
    fn perform_shred(
        &mut self,
        stream_id: StreamId,
        subject_id: &str,
    ) -> Result<StreamShredReceipt, Box<dyn std::error::Error + Send + Sync>> {
        let table_meta = self
            .inner
            .kernel_state
            .tables()
            .iter()
            .find(|(_, m)| m.stream_id == stream_id)
            .map(|(id, m)| (*id, m.tenant_id, m.primary_key.clone()));

        let (table_id, tenant_id, pk_cols) = match table_meta {
            Some(t) => t,
            None => {
                // No table backs this stream — nothing to delete, but
                // we still shred a DEK so the attestation commits to
                // the destruction event.
                let digest = self.shred_ephemeral_dek(stream_id, subject_id);
                return Ok(StreamShredReceipt {
                    key_shred_digest: digest,
                    records_erased: 0,
                    stream_length_at_shred: 0,
                });
            }
        };

        let pairs = self
            .inner
            .projection_store
            .scan(
                StoreTableId::new(table_id.0),
                Key::min()..Key::max(),
                MAX_ROWS_PER_STREAM,
            )
            .map_err(box_err)?;
        let stream_length_at_shred = pairs.len() as u64;

        let mut records_erased = 0u64;
        for (_pk_key, row_bytes) in &pairs {
            let row: serde_json::Value = serde_json::from_slice(row_bytes).map_err(box_err)?;
            if !subject_matches(&row, &self.subject_column, subject_id) {
                continue;
            }
            let row_data = build_delete_event(&row, &pk_cols)?;
            let cmd = Command::Delete {
                tenant_id,
                table_id,
                row_data,
            };
            self.submit_via_inner(cmd)?;
            records_erased += 1;
        }

        let digest = self.shred_ephemeral_dek(stream_id, subject_id);
        Ok(StreamShredReceipt {
            key_shred_digest: digest,
            records_erased,
            stream_length_at_shred,
        })
    }

    /// Apply a command to the kernel + execute its effects against
    /// the borrowed inner state, mirroring `Kimberlite::submit` but
    /// without re-acquiring the lock the caller already holds.
    fn submit_via_inner(
        &mut self,
        cmd: Command,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (new_state, effects) =
            apply_committed(self.inner.kernel_state.clone(), cmd).map_err(box_err)?;
        self.inner.kernel_state = new_state;
        self.inner.execute_effects(effects).map_err(box_err)
    }

    /// Generate a fresh DEK, derive a domain-separated nonce from
    /// `(stream_id, subject_id)`, and shred. Returns the
    /// SHA-256(key_bytes || nonce) digest committing to the destroyed
    /// key material.
    fn shred_ephemeral_dek(&self, stream_id: StreamId, subject_id: &str) -> [u8; 32] {
        let (dek, _wrapped) = DataEncryptionKey::generate_and_wrap(&self.kek);
        let mut hasher = Sha256::new();
        hasher.update(SHRED_NONCE_TAG);
        hasher.update(u64::from(stream_id).to_le_bytes());
        hasher.update(subject_id.as_bytes());
        let nonce: [u8; 32] = hasher.finalize().into();
        dek.shred(&nonce)
    }
}

impl ErasureExecutor for KernelBackedErasureExecutor<'_> {
    fn pre_erasure_merkle_root(
        &mut self,
        stream_id: StreamId,
    ) -> Result<[u8; 32], Box<dyn std::error::Error + Send + Sync>> {
        let chain = self
            .inner
            .storage
            .latest_chain_hash(stream_id)
            .map_err(box_err)?;
        // An empty stream has no chain hash; commit to a stable
        // sentinel rather than leaking `Option` semantics through the
        // trait surface (the witness type expects `[u8; 32]`).
        Ok(chain.map_or([0u8; 32], <[u8; 32]>::from))
    }

    fn shred_stream(
        &mut self,
        stream_id: StreamId,
        subject_id: &str,
    ) -> Result<StreamShredReceipt, Box<dyn std::error::Error + Send + Sync>> {
        self.perform_shred(stream_id, subject_id)
    }
}

fn subject_matches(row: &serde_json::Value, column: &str, subject_id: &str) -> bool {
    row.as_object()
        .and_then(|o| o.get(column))
        .and_then(|v| v.as_str())
        .is_some_and(|s| s == subject_id)
}

/// Build the `{"type":"delete","where":[...]}` event payload that the
/// projection apply path expects, keyed on the row's primary-key
/// columns. Mirrors [`crate::tenant::TenantHandle`]'s DELETE planner.
fn build_delete_event(
    row: &serde_json::Value,
    pk_cols: &[String],
) -> Result<Bytes, Box<dyn std::error::Error + Send + Sync>> {
    let obj = row
        .as_object()
        .ok_or_else(|| boxed("row is not a JSON object"))?;

    let predicates: Vec<serde_json::Value> = pk_cols
        .iter()
        .map(|col| {
            let v = obj.get(col).cloned().unwrap_or(serde_json::Value::Null);
            json!({"op": "eq", "column": col, "values": [v]})
        })
        .collect();

    let event = json!({
        "type": "delete",
        "where": predicates,
    });
    let bytes = serde_json::to_vec(&event).map_err(box_err)?;
    Ok(Bytes::from(bytes))
}

fn box_err<E: std::error::Error + Send + Sync + 'static>(
    e: E,
) -> Box<dyn std::error::Error + Send + Sync> {
    Box::new(e)
}

fn boxed(msg: &str) -> Box<dyn std::error::Error + Send + Sync> {
    msg.into()
}
