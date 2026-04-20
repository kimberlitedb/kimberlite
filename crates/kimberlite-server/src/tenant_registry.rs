//! In-memory tenant registry for the admin API.
//!
//! Kimberlite's kernel doesn't (yet) persist tenant metadata — tenants are
//! identified solely by their `TenantId` and are auto-created on first
//! access. To support `TenantCreate` / `TenantList` / `TenantGet` /
//! `TenantDelete` over the wire we keep a lightweight registry in the
//! server process. Restart semantics are intentionally simple: tenants
//! that had tables before a restart reappear in the registry as soon as
//! they're accessed again (lazy re-registration in `handler.rs`).
//!
//! A persistent registry is tracked in ROADMAP v0.6 as a separate
//! engineering item — the wire protocol is already shaped to accept it.

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

use kimberlite_types::TenantId;

#[derive(Debug, Clone)]
pub struct TenantEntry {
    pub name: Option<String>,
    pub created_at_nanos: u64,
}

#[derive(Debug, Default)]
pub struct TenantRegistry {
    entries: RwLock<HashMap<TenantId, TenantEntry>>,
}

impl TenantRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ensure an entry for `tenant_id` exists. Returns `(entry, created)`
    /// where `created` is `true` when this call created the registration.
    ///
    /// If the tenant already exists with a name, re-registering with a
    /// *different* name is rejected — that would silently rewrite the
    /// label. Same-name (or `None` name) is idempotent.
    pub fn register(
        &self,
        tenant_id: TenantId,
        name: Option<String>,
    ) -> Result<(TenantEntry, bool), RegistryError> {
        let mut entries = self.entries.write().map_err(|_| RegistryError::Poisoned)?;

        if let Some(existing) = entries.get(&tenant_id) {
            if let (Some(existing_name), Some(incoming)) = (&existing.name, &name) {
                if existing_name != incoming {
                    return Err(RegistryError::AlreadyExistsDifferentName {
                        existing: existing_name.clone(),
                        incoming: incoming.clone(),
                    });
                }
            }
            return Ok((existing.clone(), false));
        }

        let entry = TenantEntry {
            name,
            created_at_nanos: now_nanos(),
        };
        entries.insert(tenant_id, entry.clone());
        Ok((entry, true))
    }

    /// Ensure the given tenant is present without failing on name mismatch —
    /// used for lazy re-registration during handshake / ListTables.
    pub fn touch(&self, tenant_id: TenantId) {
        let mut entries = match self.entries.write() {
            Ok(e) => e,
            Err(_) => return,
        };
        entries.entry(tenant_id).or_insert_with(|| TenantEntry {
            name: None,
            created_at_nanos: now_nanos(),
        });
    }

    pub fn get(&self, tenant_id: TenantId) -> Option<TenantEntry> {
        self.entries
            .read()
            .ok()
            .and_then(|e| e.get(&tenant_id).cloned())
    }

    pub fn list(&self) -> Vec<(TenantId, TenantEntry)> {
        let entries = match self.entries.read() {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };
        let mut out: Vec<(TenantId, TenantEntry)> =
            entries.iter().map(|(k, v)| (*k, v.clone())).collect();
        out.sort_by_key(|(k, _)| u64::from(*k));
        out
    }

    /// Remove a tenant from the registry. Returns `true` if it was present.
    pub fn remove(&self, tenant_id: TenantId) -> bool {
        self.entries
            .write()
            .ok()
            .and_then(|mut e| e.remove(&tenant_id))
            .is_some()
    }

    pub fn len(&self) -> usize {
        self.entries.read().map(|e| e.len()).unwrap_or(0)
    }
}

fn now_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_nanos()).ok())
        .unwrap_or(0)
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("tenant registry lock poisoned")]
    Poisoned,
    #[error(
        "tenant already registered with a different name (existing: {existing:?}, incoming: {incoming:?})"
    )]
    AlreadyExistsDifferentName { existing: String, incoming: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_then_get_returns_entry() {
        let reg = TenantRegistry::new();
        let (entry, created) = reg.register(TenantId::new(1), Some("acme".into())).unwrap();
        assert!(created);
        assert_eq!(entry.name.as_deref(), Some("acme"));

        let fetched = reg.get(TenantId::new(1)).unwrap();
        assert_eq!(fetched.name.as_deref(), Some("acme"));
    }

    #[test]
    fn register_is_idempotent_for_same_or_no_name() {
        let reg = TenantRegistry::new();
        let (_, c1) = reg.register(TenantId::new(1), Some("acme".into())).unwrap();
        let (_, c2) = reg.register(TenantId::new(1), Some("acme".into())).unwrap();
        // Re-register with no name — should also be idempotent.
        let (_, c3) = reg.register(TenantId::new(1), None).unwrap();
        assert!(c1);
        assert!(!c2);
        assert!(!c3);
    }

    #[test]
    fn register_rejects_different_name() {
        let reg = TenantRegistry::new();
        reg.register(TenantId::new(1), Some("acme".into())).unwrap();
        let err = reg
            .register(TenantId::new(1), Some("other".into()))
            .unwrap_err();
        assert!(matches!(
            err,
            RegistryError::AlreadyExistsDifferentName { .. }
        ));
    }

    #[test]
    fn list_returns_sorted() {
        let reg = TenantRegistry::new();
        reg.register(TenantId::new(3), None).unwrap();
        reg.register(TenantId::new(1), None).unwrap();
        reg.register(TenantId::new(2), None).unwrap();
        let listed: Vec<u64> = reg
            .list()
            .into_iter()
            .map(|(id, _)| u64::from(id))
            .collect();
        assert_eq!(listed, vec![1, 2, 3]);
    }

    #[test]
    fn touch_adds_without_overwriting() {
        let reg = TenantRegistry::new();
        reg.register(TenantId::new(1), Some("acme".into())).unwrap();
        reg.touch(TenantId::new(1));
        reg.touch(TenantId::new(2));
        assert_eq!(reg.len(), 2);
        assert_eq!(
            reg.get(TenantId::new(1)).unwrap().name.as_deref(),
            Some("acme")
        );
        assert_eq!(reg.get(TenantId::new(2)).unwrap().name, None);
    }

    #[test]
    fn remove_returns_presence() {
        let reg = TenantRegistry::new();
        reg.register(TenantId::new(1), None).unwrap();
        assert!(reg.remove(TenantId::new(1)));
        assert!(!reg.remove(TenantId::new(1)));
    }
}
