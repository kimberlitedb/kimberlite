//! Grouped admin namespace for the Rust SDK.
//!
//! AUDIT-2026-04 S2.5 — mirrors the TypeScript SDK's
//! `client.admin.xxx` and the Python SDK's `client.admin.xxx`
//! grouping. The same methods remain available as flat
//! `Client::xxx` for back-compat — the grouped form is additive.
//!
//! # Example
//!
//! ```no_run
//! use kimberlite_client::{Client, ClientConfig};
//! use kimberlite_types::TenantId;
//! # fn main() -> kimberlite_client::ClientResult<()> {
//! let mut client = Client::connect(
//!     "127.0.0.1:5432",
//!     TenantId::new(1),
//!     ClientConfig::default(),
//! )?;
//!
//! let tables = client.admin().list_tables()?;
//! let info = client.admin().server_info()?;
//! # Ok(()) }
//! ```

use kimberlite_types::TenantId;
use kimberlite_wire::{
    ApiKeyInfo, ApiKeyRegisterResponse, ApiKeyRotateResponse, DescribeTableResponse,
    IndexInfo, ServerInfoResponse, TableInfo, TenantCreateResponse, TenantDeleteResponse,
    TenantInfo,
};

use crate::client::Client;
use crate::error::ClientResult;

/// Admin operations — schema introspection, tenant lifecycle,
/// API-key lifecycle, server info.
///
/// Borrowed from a `&mut Client` via [`Client::admin`]. Each
/// method delegates to the underlying flat method so behaviour
/// is identical to the back-compat path.
pub struct AdminApi<'a> {
    client: &'a mut Client,
}

impl<'a> AdminApi<'a> {
    pub(crate) fn new(client: &'a mut Client) -> Self {
        Self { client }
    }

    // -- Schema introspection -------------------------------------------

    pub fn list_tables(&mut self) -> ClientResult<Vec<TableInfo>> {
        self.client.list_tables()
    }

    pub fn describe_table(
        &mut self,
        table_name: &str,
    ) -> ClientResult<DescribeTableResponse> {
        self.client.describe_table(table_name)
    }

    pub fn list_indexes(&mut self, table_name: &str) -> ClientResult<Vec<IndexInfo>> {
        self.client.list_indexes(table_name)
    }

    // -- Tenant lifecycle ----------------------------------------------

    pub fn tenant_create(
        &mut self,
        tenant_id: TenantId,
        name: Option<String>,
    ) -> ClientResult<TenantCreateResponse> {
        self.client.tenant_create(tenant_id, name)
    }

    pub fn tenant_list(&mut self) -> ClientResult<Vec<TenantInfo>> {
        self.client.tenant_list()
    }

    pub fn tenant_delete(
        &mut self,
        tenant_id: TenantId,
    ) -> ClientResult<TenantDeleteResponse> {
        self.client.tenant_delete(tenant_id)
    }

    pub fn tenant_get(&mut self, tenant_id: TenantId) -> ClientResult<TenantInfo> {
        self.client.tenant_get(tenant_id)
    }

    // -- API-key lifecycle ---------------------------------------------

    pub fn api_key_register(
        &mut self,
        subject: &str,
        tenant_id: TenantId,
        roles: Vec<String>,
        expires_at_nanos: Option<u64>,
    ) -> ClientResult<ApiKeyRegisterResponse> {
        self.client
            .api_key_register(subject, tenant_id, roles, expires_at_nanos)
    }

    pub fn api_key_revoke(&mut self, key: &str) -> ClientResult<bool> {
        self.client.api_key_revoke(key)
    }

    pub fn api_key_list(&mut self, tenant_id: Option<TenantId>) -> ClientResult<Vec<ApiKeyInfo>> {
        self.client.api_key_list(tenant_id)
    }

    pub fn api_key_rotate(&mut self, old_key: &str) -> ClientResult<ApiKeyRotateResponse> {
        self.client.api_key_rotate(old_key)
    }

    // -- Server info ---------------------------------------------------

    pub fn server_info(&mut self) -> ClientResult<ServerInfoResponse> {
        self.client.server_info()
    }
}

#[cfg(test)]
mod tests {
    //! Compile-time smoke tests — the grouped namespace has the
    //! right method names and returns the right types.
    //! End-to-end behaviour is exercised by the server-level
    //! integration suite (same FFI methods the flat API uses).

    use super::*;

    /// If the method signatures drift from the `Client` flat
    /// methods, this will fail to compile. It never runs — the
    /// never-returning `unreachable!()` keeps the trybuild-style
    /// type check alive without needing a live server.
    #[allow(dead_code)]
    fn _signature_trybuild(client: &mut Client) {
        let mut admin = client.admin();
        let _: ClientResult<Vec<TableInfo>> = admin.list_tables();
        let _: ClientResult<DescribeTableResponse> = admin.describe_table("t");
        let _: ClientResult<Vec<IndexInfo>> = admin.list_indexes("t");
        let _: ClientResult<TenantCreateResponse> =
            admin.tenant_create(TenantId::new(1), None);
        let _: ClientResult<Vec<TenantInfo>> = admin.tenant_list();
        let _: ClientResult<TenantDeleteResponse> = admin.tenant_delete(TenantId::new(1));
        let _: ClientResult<TenantInfo> = admin.tenant_get(TenantId::new(1));
        let _: ClientResult<ApiKeyRegisterResponse> =
            admin.api_key_register("alice", TenantId::new(1), vec![], None);
        let _: ClientResult<bool> = admin.api_key_revoke("key");
        let _: ClientResult<Vec<ApiKeyInfo>> = admin.api_key_list(None);
        let _: ClientResult<ApiKeyRotateResponse> = admin.api_key_rotate("old");
        let _: ClientResult<ServerInfoResponse> = admin.server_info();
    }
}
