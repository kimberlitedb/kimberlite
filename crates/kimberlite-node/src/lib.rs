//! Node.js N-API bindings for the Kimberlite client.
//!
//! This crate is the native backend for `@kimberlite/client` on npm. It wraps
//! the synchronous Rust `kimberlite-client` with napi-rs async handles so every
//! call from JavaScript returns a `Promise` and does not block the Node event loop.

#![allow(clippy::needless_pass_by_value)] // napi-derive expects owned params

use std::sync::{Arc, Mutex};
use std::time::Duration;

use napi::bindgen_prelude::*;
use napi_derive::napi;

use kimberlite_client::{
    Client, ClientConfig, ClientError, Pool, PoolConfig, PooledClient,
};
use kimberlite_types::{DataClass, Offset, Placement, Region, StreamId, TenantId};
use kimberlite_wire::{
    ClusterMode as WireClusterMode, ErrorCode, PushPayload, QueryParam as WireQueryParam,
    QueryValue as WireQueryValue, SubscriptionCloseReason,
};

// ============================================================================
// Public JS-facing types
// ============================================================================

/// Data classification for a stream. Mirrors `kimberlite_types::DataClass`.
#[napi(string_enum)]
pub enum JsDataClass {
    PHI,
    Deidentified,
    PII,
    Sensitive,
    PCI,
    Financial,
    Confidential,
    Public,
}

/// Placement policy for a stream.
#[napi(string_enum)]
pub enum JsPlacement {
    Global,
    UsEast1,
    ApSoutheast2,
}

/// Connection configuration.
#[napi(object)]
pub struct JsClientConfig {
    pub address: String,
    pub tenant_id: BigInt,
    pub auth_token: Option<String>,
    pub read_timeout_ms: Option<u32>,
    pub write_timeout_ms: Option<u32>,
    pub buffer_size_bytes: Option<u32>,
}

/// One SQL parameter value.
#[napi(object)]
pub struct JsQueryParam {
    /// Kind tag: "null" | "bigint" | "text" | "boolean" | "timestamp".
    pub kind: String,
    pub int_value: Option<BigInt>,
    pub text_value: Option<String>,
    pub bool_value: Option<bool>,
    pub timestamp_value: Option<BigInt>,
}

/// One SQL result cell.
#[napi(object)]
pub struct JsQueryValue {
    /// Kind tag: "null" | "bigint" | "text" | "boolean" | "timestamp".
    pub kind: String,
    pub int_value: Option<BigInt>,
    pub text_value: Option<String>,
    pub bool_value: Option<bool>,
    pub timestamp_value: Option<BigInt>,
}

/// Result of a SQL query.
#[napi(object)]
pub struct JsQueryResponse {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<JsQueryValue>>,
}

/// Result of a stream read.
#[napi(object)]
pub struct JsReadEventsResponse {
    pub events: Vec<Buffer>,
    pub next_offset: Option<BigInt>,
}

/// Result of a DML/DDL `execute()` call.
#[napi(object)]
pub struct JsExecuteResult {
    /// Number of rows inserted / updated / deleted (0 for DDL).
    pub rows_affected: BigInt,
    /// Log offset at which the change was committed.
    pub log_offset: BigInt,
}

/// Handshake result for a new subscription.
#[napi(object)]
pub struct JsSubscribeAck {
    pub subscription_id: BigInt,
    pub start_offset: BigInt,
    pub credits: u32,
}

/// A single event yielded from a subscription, or a close marker.
#[napi(object)]
pub struct JsSubscriptionEvent {
    pub offset: BigInt,
    pub data: Option<Buffer>,
    /// `true` once the subscription has closed; `data` will be `null` and
    /// further `nextEvent()` calls return the same closed marker.
    pub closed: bool,
    /// One of: "ClientCancelled" | "ServerShutdown" | "StreamDeleted"
    /// | "BackpressureTimeout" | "ProtocolError". Only meaningful when
    /// `closed` is true.
    pub close_reason: Option<String>,
}

fn close_reason_to_str(r: SubscriptionCloseReason) -> &'static str {
    match r {
        SubscriptionCloseReason::ClientCancelled => "ClientCancelled",
        SubscriptionCloseReason::ServerShutdown => "ServerShutdown",
        SubscriptionCloseReason::StreamDeleted => "StreamDeleted",
        SubscriptionCloseReason::BackpressureTimeout => "BackpressureTimeout",
        SubscriptionCloseReason::ProtocolError => "ProtocolError",
    }
}

// ============================================================================
// Phase 4 — admin + schema + server info (JS-facing types)
// ============================================================================

#[napi(object)]
pub struct JsTableInfo {
    pub name: String,
    pub column_count: u32,
}

#[napi(object)]
pub struct JsColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub primary_key: bool,
}

#[napi(object)]
pub struct JsIndexInfo {
    pub name: String,
    pub columns: Vec<String>,
}

#[napi(object)]
pub struct JsDescribeTable {
    pub table_name: String,
    pub columns: Vec<JsColumnInfo>,
}

#[napi(object)]
pub struct JsTenantInfo {
    pub tenant_id: BigInt,
    pub name: Option<String>,
    pub table_count: u32,
    pub created_at_nanos: Option<BigInt>,
}

#[napi(object)]
pub struct JsTenantCreateResult {
    pub tenant: JsTenantInfo,
    pub created: bool,
}

#[napi(object)]
pub struct JsTenantDeleteResult {
    pub deleted: bool,
    pub tables_dropped: u32,
}

#[napi(object)]
pub struct JsApiKeyInfo {
    pub key_id: String,
    pub subject: String,
    pub tenant_id: BigInt,
    pub roles: Vec<String>,
    pub expires_at_nanos: Option<BigInt>,
}

#[napi(object)]
pub struct JsApiKeyRegisterResult {
    pub key: String,
    pub info: JsApiKeyInfo,
}

#[napi(object)]
pub struct JsApiKeyRotateResult {
    pub new_key: String,
    pub info: JsApiKeyInfo,
}

#[napi(object)]
pub struct JsServerInfo {
    pub build_version: String,
    pub protocol_version: u32,
    pub capabilities: Vec<String>,
    pub uptime_secs: BigInt,
    /// `"Standalone"` or `"Clustered"`.
    pub cluster_mode: String,
    pub tenant_count: u32,
}

fn cluster_mode_to_str(m: WireClusterMode) -> &'static str {
    match m {
        WireClusterMode::Standalone => "Standalone",
        WireClusterMode::Clustered => "Clustered",
    }
}

fn tenant_info_to_js(info: kimberlite_wire::TenantInfo) -> JsTenantInfo {
    JsTenantInfo {
        tenant_id: BigInt::from(u64::from(info.tenant_id)),
        name: info.name,
        table_count: info.table_count,
        created_at_nanos: info.created_at_nanos.map(BigInt::from),
    }
}

fn api_key_info_to_js(info: kimberlite_wire::ApiKeyInfo) -> JsApiKeyInfo {
    JsApiKeyInfo {
        key_id: info.key_id,
        subject: info.subject,
        tenant_id: BigInt::from(u64::from(info.tenant_id)),
        roles: info.roles,
        expires_at_nanos: info.expires_at_nanos.map(BigInt::from),
    }
}

// ============================================================================
// Client wrapper
// ============================================================================

/// Async-safe wrapper around the synchronous `kimberlite-client` Client.
///
/// All methods offload I/O to a blocking tokio worker so the Node event loop
/// is never stalled by a socket read.
#[napi]
pub struct KimberliteClient {
    inner: Arc<Mutex<Client>>,
}

#[napi]
impl KimberliteClient {
    /// Connects to a Kimberlite server and performs the protocol handshake.
    #[napi(factory)]
    pub async fn connect(config: JsClientConfig) -> Result<Self> {
        let addr = config.address;
        let tenant = TenantId::new(config.tenant_id.get_u64().1);
        let cfg = ClientConfig {
            read_timeout: config.read_timeout_ms.map(|ms| Duration::from_millis(u64::from(ms))),
            write_timeout: config.write_timeout_ms.map(|ms| Duration::from_millis(u64::from(ms))),
            buffer_size: config
                .buffer_size_bytes
                .map_or(64 * 1024, |b| b as usize),
            auth_token: config.auth_token,
        };

        let client = spawn_blocking_client(move || Client::connect(addr, tenant, cfg)).await?;

        Ok(Self {
            inner: Arc::new(Mutex::new(client)),
        })
    }

    /// Creates a new stream with the given data classification.
    #[napi]
    pub async fn create_stream(
        &self,
        name: String,
        data_class: JsDataClass,
    ) -> Result<BigInt> {
        let client = self.inner.clone();
        let dc = map_data_class(data_class);
        let stream_id = spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.create_stream(&name, dc)
        })
        .await?;
        Ok(BigInt::from(u64::from(stream_id)))
    }

    /// Creates a new stream with a specific geographic placement policy.
    #[napi]
    pub async fn create_stream_with_placement(
        &self,
        name: String,
        data_class: JsDataClass,
        placement: JsPlacement,
    ) -> Result<BigInt> {
        let client = self.inner.clone();
        let dc = map_data_class(data_class);
        let p = map_placement(placement);
        let stream_id = spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.create_stream_with_placement(&name, dc, p)
        })
        .await?;
        Ok(BigInt::from(u64::from(stream_id)))
    }

    /// Appends events to a stream with optimistic concurrency.
    ///
    /// Returns the offset of the first appended event.
    #[napi]
    pub async fn append(
        &self,
        stream_id: BigInt,
        events: Vec<Buffer>,
        expected_offset: BigInt,
    ) -> Result<BigInt> {
        let client = self.inner.clone();
        let sid = StreamId::from(stream_id.get_u64().1);
        let offset = Offset::from(expected_offset.get_u64().1);
        let payload: Vec<Vec<u8>> = events.into_iter().map(|b| b.to_vec()).collect();

        let first = spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.append(sid, payload, offset)
        })
        .await?;
        Ok(BigInt::from(u64::from(first)))
    }

    /// Reads events from a stream starting at `from_offset`.
    #[napi]
    pub async fn read_events(
        &self,
        stream_id: BigInt,
        from_offset: BigInt,
        max_bytes: BigInt,
    ) -> Result<JsReadEventsResponse> {
        let client = self.inner.clone();
        let sid = StreamId::from(stream_id.get_u64().1);
        let from = Offset::from(from_offset.get_u64().1);
        let max = max_bytes.get_u64().1;

        let resp = spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.read_events(sid, from, max)
        })
        .await?;

        Ok(JsReadEventsResponse {
            events: resp.events.into_iter().map(Buffer::from).collect(),
            next_offset: resp.next_offset.map(|o| BigInt::from(u64::from(o))),
        })
    }

    /// Executes a SQL query against the server.
    #[napi]
    pub async fn query(
        &self,
        sql: String,
        params: Option<Vec<JsQueryParam>>,
    ) -> Result<JsQueryResponse> {
        let client = self.inner.clone();
        let wire_params: Vec<WireQueryParam> = params
            .unwrap_or_default()
            .into_iter()
            .map(map_query_param)
            .collect::<Result<Vec<_>>>()?;

        let resp = spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.query(&sql, &wire_params)
        })
        .await?;

        Ok(JsQueryResponse {
            columns: resp.columns,
            rows: resp
                .rows
                .into_iter()
                .map(|row| row.into_iter().map(map_query_value).collect())
                .collect(),
        })
    }

    /// Executes a SQL query at a specific log position (time travel).
    #[napi]
    pub async fn query_at(
        &self,
        sql: String,
        params: Option<Vec<JsQueryParam>>,
        position: BigInt,
    ) -> Result<JsQueryResponse> {
        let client = self.inner.clone();
        let wire_params: Vec<WireQueryParam> = params
            .unwrap_or_default()
            .into_iter()
            .map(map_query_param)
            .collect::<Result<Vec<_>>>()?;
        let pos = Offset::from(position.get_u64().1);

        let resp = spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.query_at(&sql, &wire_params, pos)
        })
        .await?;

        Ok(JsQueryResponse {
            columns: resp.columns,
            rows: resp
                .rows
                .into_iter()
                .map(|row| row.into_iter().map(map_query_value).collect())
                .collect(),
        })
    }

    /// Executes a DML or DDL SQL statement (INSERT / UPDATE / DELETE / CREATE / ALTER).
    ///
    /// Returns the row-affected count and the log offset at which the change
    /// committed. For DDL statements the row count is typically 0.
    #[napi]
    pub async fn execute(
        &self,
        sql: String,
        params: Option<Vec<JsQueryParam>>,
    ) -> Result<JsExecuteResult> {
        let client = self.inner.clone();
        let wire_params: Vec<WireQueryParam> = params
            .unwrap_or_default()
            .into_iter()
            .map(map_query_param)
            .collect::<Result<Vec<_>>>()?;

        let (rows, offset) = spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.execute(&sql, &wire_params)
        })
        .await?;

        Ok(JsExecuteResult {
            rows_affected: BigInt::from(rows),
            log_offset: BigInt::from(offset),
        })
    }

    /// Flushes pending data to disk on the server.
    #[napi]
    pub async fn sync(&self) -> Result<()> {
        let client = self.inner.clone();
        spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.sync()
        })
        .await
    }

    /// Returns the tenant ID this client is connected as.
    #[napi(getter)]
    pub fn tenant_id(&self) -> Result<BigInt> {
        let c = lock_client(&self.inner)?;
        Ok(BigInt::from(u64::from(c.tenant_id())))
    }

    /// Returns the wire request ID of the most recently sent request, or `null`
    /// if no request has been sent yet. Useful for correlating client-side
    /// behaviour with server-side tracing output.
    #[napi(getter)]
    pub fn last_request_id(&self) -> Result<Option<BigInt>> {
        let c = lock_client(&self.inner)?;
        Ok(c.last_request_id().map(BigInt::from))
    }

    /// Subscribe to real-time events on a stream. Returns the assigned
    /// subscription ID and initial credit balance. Drain events with
    /// [`next_subscription_event`](Self::next_subscription_event).
    #[napi]
    pub async fn subscribe(
        &self,
        stream_id: BigInt,
        from_offset: BigInt,
        initial_credits: u32,
        consumer_group: Option<String>,
    ) -> Result<JsSubscribeAck> {
        let client = self.inner.clone();
        let sid = StreamId::from(stream_id.get_u64().1);
        let off = Offset::from(from_offset.get_u64().1);
        let ack = spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.subscribe(sid, off, initial_credits, consumer_group)
        })
        .await?;
        Ok(JsSubscribeAck {
            subscription_id: BigInt::from(ack.subscription_id),
            start_offset: BigInt::from(u64::from(ack.start_offset)),
            credits: ack.credits,
        })
    }

    /// Grant additional credits to an active subscription. Returns the new
    /// server-side balance.
    #[napi]
    pub async fn grant_credits(
        &self,
        subscription_id: BigInt,
        additional: u32,
    ) -> Result<u32> {
        let client = self.inner.clone();
        let sid = subscription_id.get_u64().1;
        spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.grant_credits(sid, additional)
        })
        .await
    }

    /// Cancel an active subscription. The server emits a final closed event
    /// which `next_subscription_event` will surface.
    #[napi]
    pub async fn unsubscribe(&self, subscription_id: BigInt) -> Result<()> {
        let client = self.inner.clone();
        let sid = subscription_id.get_u64().1;
        spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.unsubscribe(sid)
        })
        .await
    }

    // --- Phase 4: admin + schema + server info ---------------------------

    #[napi]
    pub async fn list_tables(&self) -> Result<Vec<JsTableInfo>> {
        let client = self.inner.clone();
        let tables = spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.list_tables()
        })
        .await?;
        Ok(tables
            .into_iter()
            .map(|t| JsTableInfo {
                name: t.name,
                column_count: t.column_count,
            })
            .collect())
    }

    #[napi]
    pub async fn describe_table(&self, table_name: String) -> Result<JsDescribeTable> {
        let client = self.inner.clone();
        let resp = spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.describe_table(&table_name)
        })
        .await?;
        Ok(JsDescribeTable {
            table_name: resp.table_name,
            columns: resp
                .columns
                .into_iter()
                .map(|c| JsColumnInfo {
                    name: c.name,
                    data_type: c.data_type,
                    nullable: c.nullable,
                    primary_key: c.primary_key,
                })
                .collect(),
        })
    }

    #[napi]
    pub async fn list_indexes(&self, table_name: String) -> Result<Vec<JsIndexInfo>> {
        let client = self.inner.clone();
        let indexes = spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.list_indexes(&table_name)
        })
        .await?;
        Ok(indexes
            .into_iter()
            .map(|i| JsIndexInfo {
                name: i.name,
                columns: i.columns,
            })
            .collect())
    }

    #[napi]
    pub async fn tenant_create(
        &self,
        tenant_id: BigInt,
        name: Option<String>,
    ) -> Result<JsTenantCreateResult> {
        let client = self.inner.clone();
        let tid = TenantId::new(tenant_id.get_u64().1);
        let r = spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.tenant_create(tid, name)
        })
        .await?;
        Ok(JsTenantCreateResult {
            tenant: tenant_info_to_js(r.tenant),
            created: r.created,
        })
    }

    #[napi]
    pub async fn tenant_list(&self) -> Result<Vec<JsTenantInfo>> {
        let client = self.inner.clone();
        let tenants = spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.tenant_list()
        })
        .await?;
        Ok(tenants.into_iter().map(tenant_info_to_js).collect())
    }

    #[napi]
    pub async fn tenant_delete(&self, tenant_id: BigInt) -> Result<JsTenantDeleteResult> {
        let client = self.inner.clone();
        let tid = TenantId::new(tenant_id.get_u64().1);
        let r = spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.tenant_delete(tid)
        })
        .await?;
        Ok(JsTenantDeleteResult {
            deleted: r.deleted,
            tables_dropped: r.tables_dropped,
        })
    }

    #[napi]
    pub async fn tenant_get(&self, tenant_id: BigInt) -> Result<JsTenantInfo> {
        let client = self.inner.clone();
        let tid = TenantId::new(tenant_id.get_u64().1);
        let info = spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.tenant_get(tid)
        })
        .await?;
        Ok(tenant_info_to_js(info))
    }

    #[napi]
    pub async fn api_key_register(
        &self,
        subject: String,
        tenant_id: BigInt,
        roles: Vec<String>,
        expires_at_nanos: Option<BigInt>,
    ) -> Result<JsApiKeyRegisterResult> {
        let client = self.inner.clone();
        let tid = TenantId::new(tenant_id.get_u64().1);
        let exp = expires_at_nanos.map(|n| n.get_u64().1);
        let r = spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.api_key_register(subject, tid, roles, exp)
        })
        .await?;
        Ok(JsApiKeyRegisterResult {
            key: r.key,
            info: api_key_info_to_js(r.info),
        })
    }

    #[napi]
    pub async fn api_key_revoke(&self, key: String) -> Result<bool> {
        let client = self.inner.clone();
        spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.api_key_revoke(&key)
        })
        .await
    }

    #[napi]
    pub async fn api_key_list(
        &self,
        tenant_id: Option<BigInt>,
    ) -> Result<Vec<JsApiKeyInfo>> {
        let client = self.inner.clone();
        let tid = tenant_id.map(|n| TenantId::new(n.get_u64().1));
        let keys = spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.api_key_list(tid)
        })
        .await?;
        Ok(keys.into_iter().map(api_key_info_to_js).collect())
    }

    #[napi]
    pub async fn api_key_rotate(&self, old_key: String) -> Result<JsApiKeyRotateResult> {
        let client = self.inner.clone();
        let r = spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.api_key_rotate(&old_key)
        })
        .await?;
        Ok(JsApiKeyRotateResult {
            new_key: r.new_key,
            info: api_key_info_to_js(r.info),
        })
    }

    #[napi]
    pub async fn server_info(&self) -> Result<JsServerInfo> {
        let client = self.inner.clone();
        let info = spawn_blocking_client(move || {
            let mut c = client.lock().expect("client mutex poisoned");
            c.server_info()
        })
        .await?;
        Ok(JsServerInfo {
            build_version: info.build_version,
            protocol_version: u32::from(info.protocol_version),
            capabilities: info.capabilities,
            uptime_secs: BigInt::from(info.uptime_secs),
            cluster_mode: cluster_mode_to_str(info.cluster_mode).to_string(),
            tenant_count: info.tenant_count,
        })
    }

    /// Block (on a worker thread) until the next event for the given
    /// subscription ID arrives. Returns a close-marker event once the
    /// subscription has ended.
    #[napi]
    pub async fn next_subscription_event(
        &self,
        subscription_id: BigInt,
    ) -> Result<JsSubscriptionEvent> {
        let client = self.inner.clone();
        let sid = subscription_id.get_u64().1;
        tokio::task::spawn_blocking(move || -> std::result::Result<JsSubscriptionEvent, ClientError> {
            let mut c = client.lock().expect("client mutex poisoned");
            loop {
                match c.next_push()? {
                    Some(push) => match push.payload {
                        PushPayload::SubscriptionEvents {
                            subscription_id: sub,
                            start_offset,
                            mut events,
                            credits_remaining: _,
                        } if sub == sid => {
                            if let Some(first) = events.drain(..1).next() {
                                return Ok(JsSubscriptionEvent {
                                    offset: BigInt::from(u64::from(start_offset)),
                                    data: Some(Buffer::from(first)),
                                    closed: false,
                                    close_reason: None,
                                });
                            }
                        }
                        PushPayload::SubscriptionClosed {
                            subscription_id: sub,
                            reason,
                        } if sub == sid => {
                            return Ok(JsSubscriptionEvent {
                                offset: BigInt::from(0u64),
                                data: None,
                                closed: true,
                                close_reason: Some(close_reason_to_str(reason).to_string()),
                            });
                        }
                        _ => {} // Push for another subscription — keep reading.
                    },
                    None => {
                        return Err(ClientError::Connection(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "server closed connection",
                        )));
                    }
                }
            }
        })
        .await
        .map_err(|e| Error::from_reason(format!("blocking task join error: {e}")))?
        .map_err(client_error_to_napi)
    }
}

// ============================================================================
// Connection pool
// ============================================================================

/// Configuration for [`KimberlitePool`].
#[napi(object)]
pub struct JsPoolConfig {
    pub address: String,
    pub tenant_id: BigInt,
    pub auth_token: Option<String>,
    /// Maximum concurrent connections (default 10).
    pub max_size: Option<u32>,
    /// Milliseconds to wait on `acquire` before rejecting; 0 = wait forever.
    pub acquire_timeout_ms: Option<u32>,
    /// Milliseconds an idle connection lingers before eviction; 0 = never.
    pub idle_timeout_ms: Option<u32>,
    pub read_timeout_ms: Option<u32>,
    pub write_timeout_ms: Option<u32>,
    pub buffer_size_bytes: Option<u32>,
}

/// Snapshot of pool utilisation, returned from `pool.stats()`.
#[napi(object)]
pub struct JsPoolStats {
    pub max_size: u32,
    pub open: u32,
    pub idle: u32,
    pub in_use: u32,
    pub shutdown: bool,
}

/// Thread-safe connection pool.
///
/// ```ts
/// const pool = await KimberlitePool.create({
///   address: '127.0.0.1:5432',
///   tenantId: 1n,
///   maxSize: 8,
/// });
/// const client = await pool.acquire();
/// try {
///   await client.query('SELECT 1');
/// } finally {
///   client.release();
/// }
/// ```
#[napi]
pub struct KimberlitePool {
    inner: Pool,
}

#[napi]
impl KimberlitePool {
    /// Create a new pool. Connections are not opened eagerly; the first
    /// `acquire()` triggers a `Client::connect`. Returns a Promise for
    /// JS API symmetry with `KimberliteClient.connect`, though the pool
    /// is constructed synchronously.
    #[napi(factory)]
    #[allow(clippy::unused_async)]
    pub async fn create(config: JsPoolConfig) -> Result<Self> {
        let tenant_id = TenantId::new(config.tenant_id.get_u64().1);
        let client_config = ClientConfig {
            read_timeout: config
                .read_timeout_ms
                .map(|ms| Duration::from_millis(u64::from(ms))),
            write_timeout: config
                .write_timeout_ms
                .map(|ms| Duration::from_millis(u64::from(ms))),
            buffer_size: config
                .buffer_size_bytes
                .map_or(64 * 1024, |b| b as usize),
            auth_token: config.auth_token,
        };

        let pool_config = PoolConfig {
            max_size: config.max_size.map_or(10, |n| n as usize),
            acquire_timeout: match config.acquire_timeout_ms {
                Some(0) => None,
                Some(n) => Some(Duration::from_millis(u64::from(n))),
                None => Some(Duration::from_secs(30)),
            },
            idle_timeout: match config.idle_timeout_ms {
                Some(0) => None,
                Some(n) => Some(Duration::from_millis(u64::from(n))),
                None => Some(Duration::from_secs(300)),
            },
            client_config,
        };

        let inner = Pool::new(config.address.as_str(), tenant_id, pool_config)
            .map_err(client_error_to_napi)?;
        Ok(Self { inner })
    }

    /// Acquire a client from the pool. Blocks until one is available or the
    /// `acquireTimeoutMs` elapses.
    #[napi]
    pub async fn acquire(&self) -> Result<KimberlitePooledClient> {
        let pool = self.inner.clone();
        let guard = tokio::task::spawn_blocking(move || pool.acquire())
            .await
            .map_err(|e| Error::from_reason(format!("blocking task join error: {e}")))?
            .map_err(client_error_to_napi)?;
        Ok(KimberlitePooledClient {
            guard: Arc::new(Mutex::new(Some(guard))),
        })
    }

    /// Returns pool utilisation statistics.
    #[napi]
    pub fn stats(&self) -> JsPoolStats {
        let s = self.inner.stats();
        JsPoolStats {
            max_size: s.max_size as u32,
            open: s.open as u32,
            idle: s.idle as u32,
            in_use: s.in_use as u32,
            shutdown: s.shutdown,
        }
    }

    /// Shut the pool down. Subsequent acquires fail; in-flight clients close
    /// when released.
    #[napi]
    pub fn shutdown(&self) {
        self.inner.shutdown();
    }
}

/// Pool-borrowed client. Mirrors `KimberliteClient`'s surface but belongs to
/// a pool — call `release()` or `discard()` when done.
#[napi]
pub struct KimberlitePooledClient {
    guard: Arc<Mutex<Option<PooledClient>>>,
}

#[napi]
impl KimberlitePooledClient {
    /// Return the client to the pool. Idempotent.
    #[napi]
    pub fn release(&self) {
        // Dropping the PooledClient returns it to the pool.
        let mut slot = self.guard.lock().expect("pool guard mutex poisoned");
        slot.take();
    }

    /// Drop the underlying connection instead of returning it to the pool.
    /// Use after a fatal protocol error.
    #[napi]
    pub fn discard(&self) {
        let mut slot = self.guard.lock().expect("pool guard mutex poisoned");
        if let Some(guard) = slot.take() {
            guard.discard();
        }
    }

    #[napi(getter)]
    pub fn tenant_id(&self) -> Result<BigInt> {
        self.with_client(|c| Ok(BigInt::from(u64::from(c.tenant_id()))))
    }

    #[napi(getter)]
    pub fn last_request_id(&self) -> Result<Option<BigInt>> {
        self.with_client(|c| Ok(c.last_request_id().map(BigInt::from)))
    }

    #[napi]
    pub async fn create_stream(
        &self,
        name: String,
        data_class: JsDataClass,
    ) -> Result<BigInt> {
        let guard = self.guard.clone();
        let dc = map_data_class(data_class);
        let id = spawn_blocking_pooled(guard, move |c| c.create_stream(&name, dc)).await?;
        Ok(BigInt::from(u64::from(id)))
    }

    #[napi]
    pub async fn create_stream_with_placement(
        &self,
        name: String,
        data_class: JsDataClass,
        placement: JsPlacement,
    ) -> Result<BigInt> {
        let guard = self.guard.clone();
        let dc = map_data_class(data_class);
        let p = map_placement(placement);
        let id = spawn_blocking_pooled(guard, move |c| {
            c.create_stream_with_placement(&name, dc, p)
        })
        .await?;
        Ok(BigInt::from(u64::from(id)))
    }

    #[napi]
    pub async fn append(
        &self,
        stream_id: BigInt,
        events: Vec<Buffer>,
        expected_offset: BigInt,
    ) -> Result<BigInt> {
        let guard = self.guard.clone();
        let sid = StreamId::from(stream_id.get_u64().1);
        let offset = Offset::from(expected_offset.get_u64().1);
        let payload: Vec<Vec<u8>> = events.into_iter().map(|b| b.to_vec()).collect();
        let first = spawn_blocking_pooled(guard, move |c| c.append(sid, payload, offset)).await?;
        Ok(BigInt::from(u64::from(first)))
    }

    #[napi]
    pub async fn read_events(
        &self,
        stream_id: BigInt,
        from_offset: BigInt,
        max_bytes: BigInt,
    ) -> Result<JsReadEventsResponse> {
        let guard = self.guard.clone();
        let sid = StreamId::from(stream_id.get_u64().1);
        let from = Offset::from(from_offset.get_u64().1);
        let max = max_bytes.get_u64().1;
        let resp = spawn_blocking_pooled(guard, move |c| c.read_events(sid, from, max)).await?;
        Ok(JsReadEventsResponse {
            events: resp.events.into_iter().map(Buffer::from).collect(),
            next_offset: resp.next_offset.map(|o| BigInt::from(u64::from(o))),
        })
    }

    #[napi]
    pub async fn query(
        &self,
        sql: String,
        params: Option<Vec<JsQueryParam>>,
    ) -> Result<JsQueryResponse> {
        let guard = self.guard.clone();
        let wire_params: Vec<WireQueryParam> = params
            .unwrap_or_default()
            .into_iter()
            .map(map_query_param)
            .collect::<Result<Vec<_>>>()?;
        let resp = spawn_blocking_pooled(guard, move |c| c.query(&sql, &wire_params)).await?;
        Ok(JsQueryResponse {
            columns: resp.columns,
            rows: resp
                .rows
                .into_iter()
                .map(|row| row.into_iter().map(map_query_value).collect())
                .collect(),
        })
    }

    #[napi]
    pub async fn query_at(
        &self,
        sql: String,
        params: Option<Vec<JsQueryParam>>,
        position: BigInt,
    ) -> Result<JsQueryResponse> {
        let guard = self.guard.clone();
        let wire_params: Vec<WireQueryParam> = params
            .unwrap_or_default()
            .into_iter()
            .map(map_query_param)
            .collect::<Result<Vec<_>>>()?;
        let pos = Offset::from(position.get_u64().1);
        let resp =
            spawn_blocking_pooled(guard, move |c| c.query_at(&sql, &wire_params, pos)).await?;
        Ok(JsQueryResponse {
            columns: resp.columns,
            rows: resp
                .rows
                .into_iter()
                .map(|row| row.into_iter().map(map_query_value).collect())
                .collect(),
        })
    }

    #[napi]
    pub async fn execute(
        &self,
        sql: String,
        params: Option<Vec<JsQueryParam>>,
    ) -> Result<JsExecuteResult> {
        let guard = self.guard.clone();
        let wire_params: Vec<WireQueryParam> = params
            .unwrap_or_default()
            .into_iter()
            .map(map_query_param)
            .collect::<Result<Vec<_>>>()?;
        let (rows, offset) =
            spawn_blocking_pooled(guard, move |c| c.execute(&sql, &wire_params)).await?;
        Ok(JsExecuteResult {
            rows_affected: BigInt::from(rows),
            log_offset: BigInt::from(offset),
        })
    }

    #[napi]
    pub async fn sync(&self) -> Result<()> {
        let guard = self.guard.clone();
        spawn_blocking_pooled(guard, Client::sync).await
    }

    fn with_client<T>(&self, f: impl FnOnce(&Client) -> Result<T>) -> Result<T> {
        let slot = self.guard.lock().expect("pool guard mutex poisoned");
        match slot.as_ref() {
            Some(guard) => f(guard),
            None => Err(Error::from_reason(
                "[KMB_ERR_NotConnected] pooled client has been released",
            )),
        }
    }
}

async fn spawn_blocking_pooled<F, T>(
    guard: Arc<Mutex<Option<PooledClient>>>,
    f: F,
) -> Result<T>
where
    F: FnOnce(&mut Client) -> std::result::Result<T, ClientError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let mut slot = guard.lock().expect("pool guard mutex poisoned");
        let Some(pooled) = slot.as_mut() else {
            return Err(ClientError::NotConnected);
        };
        f(pooled)
    })
    .await
    .map_err(|e| Error::from_reason(format!("blocking task join error: {e}")))?
    .map_err(client_error_to_napi)
}

// ============================================================================
// Helpers
// ============================================================================

fn lock_client(inner: &Arc<Mutex<Client>>) -> Result<std::sync::MutexGuard<'_, Client>> {
    inner
        .lock()
        .map_err(|e| Error::from_reason(format!("client mutex poisoned: {e}")))
}

async fn spawn_blocking_client<F, T>(f: F) -> Result<T>
where
    F: FnOnce() -> std::result::Result<T, ClientError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| Error::from_reason(format!("blocking task join error: {e}")))?
        .map_err(client_error_to_napi)
}

fn client_error_to_napi(err: ClientError) -> Error {
    // Preserve the wire error code via a `[KMB_ERR_<code>]` prefix so the TS
    // wrapper can dispatch to a typed error subclass. The native `Status`
    // remains coarse for compatibility with generic JS consumers.
    let status = match &err {
        ClientError::Connection(_) | ClientError::Server { .. } => Status::GenericFailure,
        ClientError::NotConnected | ClientError::Timeout => Status::Cancelled,
        ClientError::Wire(_)
        | ClientError::ResponseMismatch { .. }
        | ClientError::UnexpectedResponse { .. }
        | ClientError::HandshakeFailed(_) => Status::InvalidArg,
    };

    let code_tag: &str = match &err {
        ClientError::Server { code, .. } => error_code_tag(*code),
        ClientError::Connection(_) => "Connection",
        ClientError::Timeout => "Timeout",
        ClientError::NotConnected => "NotConnected",
        ClientError::HandshakeFailed(_) => "HandshakeFailed",
        ClientError::Wire(_) => "Wire",
        ClientError::ResponseMismatch { .. } => "ResponseMismatch",
        ClientError::UnexpectedResponse { .. } => "UnexpectedResponse",
    };

    Error::new(status, format!("[KMB_ERR_{code_tag}] {err}"))
}

fn error_code_tag(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::Unknown => "Unknown",
        ErrorCode::InternalError => "InternalError",
        ErrorCode::InvalidRequest => "InvalidRequest",
        ErrorCode::AuthenticationFailed => "AuthenticationFailed",
        ErrorCode::TenantNotFound => "TenantNotFound",
        ErrorCode::StreamNotFound => "StreamNotFound",
        ErrorCode::TableNotFound => "TableNotFound",
        ErrorCode::QueryParseError => "QueryParseError",
        ErrorCode::QueryExecutionError => "QueryExecutionError",
        ErrorCode::PositionAhead => "PositionAhead",
        ErrorCode::StreamAlreadyExists => "StreamAlreadyExists",
        ErrorCode::InvalidOffset => "InvalidOffset",
        ErrorCode::StorageError => "StorageError",
        ErrorCode::ProjectionLag => "ProjectionLag",
        ErrorCode::RateLimited => "RateLimited",
        ErrorCode::NotLeader => "NotLeader",
        ErrorCode::OffsetMismatch => "OffsetMismatch",
        ErrorCode::SubscriptionNotFound => "SubscriptionNotFound",
        ErrorCode::SubscriptionClosed => "SubscriptionClosed",
        ErrorCode::SubscriptionBackpressure => "SubscriptionBackpressure",
        ErrorCode::ApiKeyNotFound => "ApiKeyNotFound",
        ErrorCode::TenantAlreadyExists => "TenantAlreadyExists",
    }
}

fn map_data_class(dc: JsDataClass) -> DataClass {
    match dc {
        JsDataClass::PHI => DataClass::PHI,
        JsDataClass::Deidentified => DataClass::Deidentified,
        JsDataClass::PII => DataClass::PII,
        JsDataClass::Sensitive => DataClass::Sensitive,
        JsDataClass::PCI => DataClass::PCI,
        JsDataClass::Financial => DataClass::Financial,
        JsDataClass::Confidential => DataClass::Confidential,
        JsDataClass::Public => DataClass::Public,
    }
}

fn map_placement(p: JsPlacement) -> Placement {
    match p {
        JsPlacement::Global => Placement::Global,
        JsPlacement::UsEast1 => Placement::Region(Region::USEast1),
        JsPlacement::ApSoutheast2 => Placement::Region(Region::APSoutheast2),
    }
}

fn map_query_param(p: JsQueryParam) -> Result<WireQueryParam> {
    match p.kind.as_str() {
        "null" => Ok(WireQueryParam::Null),
        "bigint" => {
            let v = p
                .int_value
                .ok_or_else(|| Error::from_reason("bigint param missing int_value"))?;
            Ok(WireQueryParam::BigInt(v.get_i64().0))
        }
        "text" => {
            let v = p
                .text_value
                .ok_or_else(|| Error::from_reason("text param missing text_value"))?;
            Ok(WireQueryParam::Text(v))
        }
        "boolean" => {
            let v = p
                .bool_value
                .ok_or_else(|| Error::from_reason("boolean param missing bool_value"))?;
            Ok(WireQueryParam::Boolean(v))
        }
        "timestamp" => {
            let v = p
                .timestamp_value
                .ok_or_else(|| Error::from_reason("timestamp param missing timestamp_value"))?;
            Ok(WireQueryParam::Timestamp(v.get_i64().0))
        }
        other => Err(Error::from_reason(format!("unknown param kind: {other}"))),
    }
}

fn map_query_value(v: WireQueryValue) -> JsQueryValue {
    match v {
        WireQueryValue::Null => JsQueryValue {
            kind: "null".into(),
            int_value: None,
            text_value: None,
            bool_value: None,
            timestamp_value: None,
        },
        WireQueryValue::BigInt(i) => JsQueryValue {
            kind: "bigint".into(),
            int_value: Some(BigInt::from(i)),
            text_value: None,
            bool_value: None,
            timestamp_value: None,
        },
        WireQueryValue::Text(s) => JsQueryValue {
            kind: "text".into(),
            int_value: None,
            text_value: Some(s),
            bool_value: None,
            timestamp_value: None,
        },
        WireQueryValue::Boolean(b) => JsQueryValue {
            kind: "boolean".into(),
            int_value: None,
            text_value: None,
            bool_value: Some(b),
            timestamp_value: None,
        },
        WireQueryValue::Timestamp(t) => JsQueryValue {
            kind: "timestamp".into(),
            int_value: None,
            text_value: None,
            bool_value: None,
            timestamp_value: Some(BigInt::from(t)),
        },
    }
}
