//! Request and response message types for the wire protocol.
//!
//! Messages are serialized using postcard for efficient binary encoding.

use bytes::Bytes;
use kimberlite_types::{DataClass, Offset, Placement, StreamId, TenantId};
use serde::{Deserialize, Serialize};

use crate::error::{WireError, WireResult};
use crate::frame::Frame;

/// Unique identifier for a request, used to match responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RequestId(pub u64);

impl RequestId {
    /// Creates a new request ID.
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

// ============================================================================
// Request Types
// ============================================================================

/// A client request to the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    /// Unique request identifier.
    pub id: RequestId,
    /// Tenant context for the request.
    pub tenant_id: TenantId,
    /// The request payload.
    pub payload: RequestPayload,
}

impl Request {
    /// Creates a new request.
    pub fn new(id: RequestId, tenant_id: TenantId, payload: RequestPayload) -> Self {
        Self {
            id,
            tenant_id,
            payload,
        }
    }

    /// Encodes the request to a frame.
    pub fn to_frame(&self) -> WireResult<Frame> {
        let payload =
            postcard::to_allocvec(self).map_err(|e| WireError::Serialization(e.to_string()))?;
        Ok(Frame::new(Bytes::from(payload)))
    }

    /// Decodes a request from a frame.
    pub fn from_frame(frame: &Frame) -> WireResult<Self> {
        postcard::from_bytes(&frame.payload).map_err(WireError::from)
    }
}

/// Request payload variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RequestPayload {
    /// Handshake to establish connection.
    Handshake(HandshakeRequest),
    /// Create a new stream.
    CreateStream(CreateStreamRequest),
    /// Append events to a stream.
    AppendEvents(AppendEventsRequest),
    /// Execute a SQL query.
    Query(QueryRequest),
    /// Execute a SQL query at a specific position.
    QueryAt(QueryAtRequest),
    /// Read events from a stream.
    ReadEvents(ReadEventsRequest),
    /// Subscribe to real-time events on a stream.
    Subscribe(SubscribeRequest),
    /// Sync all data to disk.
    Sync(SyncRequest),
    /// Grant additional flow-control credits to an existing subscription.
    SubscribeCredit(SubscribeCreditRequest),
    /// Cancel an existing subscription.
    Unsubscribe(UnsubscribeRequest),

    // ---- Phase 4: schema introspection (admin.v1) -------------------
    /// List all tables in the caller's tenant.
    ListTables(ListTablesRequest),
    /// Describe a table's columns + primary key.
    DescribeTable(DescribeTableRequest),
    /// List indexes on a table.
    ListIndexes(ListIndexesRequest),

    // ---- Phase 4: tenant management (admin-only) --------------------
    /// Register a new tenant in the server's registry.
    TenantCreate(TenantCreateRequest),
    /// List every tenant the server knows about.
    TenantList(TenantListRequest),
    /// Delete a tenant — drops all tables and removes from registry.
    TenantDelete(TenantDeleteRequest),
    /// Return summary info for a tenant.
    TenantGet(TenantGetRequest),

    // ---- Phase 4: API-key lifecycle (admin-only) --------------------
    /// Issue a new API key (returns plaintext once).
    ApiKeyRegister(ApiKeyRegisterRequest),
    /// Revoke a plaintext API key.
    ApiKeyRevoke(ApiKeyRevokeRequest),
    /// List API keys' metadata (no plaintext).
    ApiKeyList(ApiKeyListRequest),
    /// Atomic rotate — issue new, revoke old, return the new plaintext.
    ApiKeyRotate(ApiKeyRotateRequest),

    // ---- Phase 4: server info ---------------------------------------
    /// Get server version + capabilities + uptime.
    GetServerInfo(GetServerInfoRequest),
}

/// Handshake request to establish connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeRequest {
    /// Client protocol version.
    pub client_version: u16,
    /// Optional authentication token.
    pub auth_token: Option<String>,
}

/// Create stream request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateStreamRequest {
    /// Stream name.
    pub name: String,
    /// Data classification.
    pub data_class: DataClass,
    /// Placement policy.
    pub placement: Placement,
}

/// Append events request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendEventsRequest {
    /// Target stream.
    pub stream_id: StreamId,
    /// Events to append.
    pub events: Vec<Vec<u8>>,
    /// Expected stream offset for optimistic concurrency control.
    pub expected_offset: Offset,
}

/// SQL query request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRequest {
    /// SQL query string.
    pub sql: String,
    /// Query parameters.
    pub params: Vec<QueryParam>,
}

/// SQL query at specific position request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryAtRequest {
    /// SQL query string.
    pub sql: String,
    /// Query parameters.
    pub params: Vec<QueryParam>,
    /// Log position to query at.
    pub position: Offset,
}

/// Query parameter value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueryParam {
    /// Null value.
    Null,
    /// 64-bit integer.
    BigInt(i64),
    /// Text string.
    Text(String),
    /// Boolean.
    Boolean(bool),
    /// Timestamp (nanoseconds since epoch).
    Timestamp(i64),
}

/// Read events request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadEventsRequest {
    /// Source stream.
    pub stream_id: StreamId,
    /// Starting offset (inclusive).
    pub from_offset: Offset,
    /// Maximum bytes to read.
    pub max_bytes: u64,
}

/// Subscribe to real-time events on a stream.
///
/// The server will push events as they are appended to the stream,
/// starting from `from_offset`. The client controls flow with a credit
/// system: the server sends up to `initial_credits` events before waiting
/// for the client to grant more credits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeRequest {
    /// Stream to subscribe to.
    pub stream_id: StreamId,
    /// Starting offset (inclusive).
    pub from_offset: Offset,
    /// Maximum events the server may send before needing more credits.
    pub initial_credits: u32,
    /// Optional consumer group name for coordinated consumption.
    pub consumer_group: Option<String>,
}

/// Sync request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRequest {}

/// Grant additional flow-control credits to an active subscription.
///
/// The server stops sending push frames for a subscription once its credit
/// balance reaches zero; this request replenishes the balance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeCreditRequest {
    /// Subscription returned from the original `Subscribe` response.
    pub subscription_id: u64,
    /// Number of additional events the server may push before waiting again.
    pub additional_credits: u32,
}

/// Cancel a subscription. The server closes its send queue and emits a
/// single `SubscriptionClosed` push before forgetting the subscription.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsubscribeRequest {
    /// Subscription to cancel.
    pub subscription_id: u64,
}

// ============================================================================
// Phase 4 — schema introspection
// ============================================================================

/// Request to list every table in the caller's tenant.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListTablesRequest {}

/// Summary metadata for a single table (from `ListTables` / `TenantGet`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableInfo {
    pub name: String,
    /// Number of columns in the table (useful for CLI / dashboard summaries).
    pub column_count: u32,
}

/// Response for [`ListTablesRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListTablesResponse {
    pub tables: Vec<TableInfo>,
}

/// Request to describe a single table's columns + primary key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescribeTableRequest {
    pub table_name: String,
}

/// Column metadata returned by [`DescribeTableResponse`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    /// Column type rendered as a SQL type string (e.g. `"BIGINT"`, `"TEXT"`).
    pub data_type: String,
    pub nullable: bool,
    pub primary_key: bool,
}

/// Response for [`DescribeTableRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescribeTableResponse {
    pub table_name: String,
    pub columns: Vec<ColumnInfo>,
}

/// Request to list indexes defined on a table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListIndexesRequest {
    pub table_name: String,
}

/// Index metadata returned by [`ListIndexesResponse`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexInfo {
    pub name: String,
    pub columns: Vec<String>,
}

/// Response for [`ListIndexesRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListIndexesResponse {
    pub indexes: Vec<IndexInfo>,
}

// ============================================================================
// Phase 4 — tenant management
// ============================================================================

/// Tenant summary. Populated by `TenantList` and `TenantGet`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantInfo {
    pub tenant_id: TenantId,
    /// Optional human-readable name assigned at create time.
    pub name: Option<String>,
    pub table_count: u32,
    /// Unix-nanosecond timestamp when the tenant was first registered,
    /// or `None` if the server cannot determine it.
    pub created_at_nanos: Option<u64>,
}

/// Request to register a tenant.
///
/// `tenant_id` is required; `name` is an optional label stored in the server's
/// in-memory registry. If the tenant already exists the response carries the
/// existing registration (idempotent).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantCreateRequest {
    pub tenant_id: TenantId,
    pub name: Option<String>,
}

/// Response for [`TenantCreateRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantCreateResponse {
    pub tenant: TenantInfo,
    /// `true` if this call created the registration; `false` if it was
    /// already present (idempotent re-registration).
    pub created: bool,
}

/// Request to list every tenant on the server.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TenantListRequest {}

/// Response for [`TenantListRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantListResponse {
    pub tenants: Vec<TenantInfo>,
}

/// Request to delete a tenant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantDeleteRequest {
    pub tenant_id: TenantId,
}

/// Response for [`TenantDeleteRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantDeleteResponse {
    pub deleted: bool,
    pub tables_dropped: u32,
}

/// Request for a tenant summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantGetRequest {
    pub tenant_id: TenantId,
}

/// Response for [`TenantGetRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantGetResponse {
    pub tenant: TenantInfo,
}

// ============================================================================
// Phase 4 — API-key lifecycle
// ============================================================================

/// API-key metadata (never includes plaintext or hash).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyInfo {
    /// Short identifier of the key (first 8 chars of the hash, hex-encoded).
    pub key_id: String,
    pub subject: String,
    pub tenant_id: TenantId,
    pub roles: Vec<String>,
    pub expires_at_nanos: Option<u64>,
}

/// Request to issue a new API key.
///
/// The server generates a cryptographically random key, stores its hash, and
/// returns the plaintext exactly once. Callers must persist the plaintext —
/// it cannot be recovered later.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyRegisterRequest {
    pub subject: String,
    pub tenant_id: TenantId,
    pub roles: Vec<String>,
    /// Optional expiry as Unix nanoseconds. `None` = non-expiring.
    pub expires_at_nanos: Option<u64>,
}

/// Response for [`ApiKeyRegisterRequest`].
///
/// `key` is returned in plaintext exactly once. Store it immediately — the
/// server retains only a hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyRegisterResponse {
    pub key: String,
    pub info: ApiKeyInfo,
}

/// Request to revoke an existing API key by plaintext.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyRevokeRequest {
    pub key: String,
}

/// Response for [`ApiKeyRevokeRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyRevokeResponse {
    pub revoked: bool,
}

/// Request to list API keys, optionally filtered by tenant.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApiKeyListRequest {
    pub tenant_id: Option<TenantId>,
}

/// Response for [`ApiKeyListRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyListResponse {
    pub keys: Vec<ApiKeyInfo>,
}

/// Request to rotate an existing API key.
///
/// Semantically: issue a new key with identical subject/tenant/roles/expiry,
/// revoke the old plaintext, and return the new plaintext. The two steps
/// are performed atomically with respect to `AuthService`'s internal lock
/// so concurrent callers cannot observe an intermediate state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyRotateRequest {
    pub old_key: String,
}

/// Response for [`ApiKeyRotateRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyRotateResponse {
    pub new_key: String,
    pub info: ApiKeyInfo,
}

// ============================================================================
// Phase 4 — server info
// ============================================================================

/// Request the server's canonical info block.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GetServerInfoRequest {}

/// Replication / cluster mode the server is running in.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClusterMode {
    /// Single-node direct mode (no replication).
    Standalone,
    /// Multi-node cluster with VSR consensus.
    Clustered,
}

/// Response for [`GetServerInfoRequest`].
///
/// Returns the authoritative view of what the server supports — replaces
/// the fixed `HandshakeResponse.capabilities` in v1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfoResponse {
    /// Semantic version of the server binary (e.g. `"0.5.0"`).
    pub build_version: String,
    /// Wire protocol version (currently `2`).
    pub protocol_version: u16,
    /// Capability strings the server advertises (e.g. `"subscribe.v2"`).
    pub capabilities: Vec<String>,
    /// Seconds since the server started.
    pub uptime_secs: u64,
    pub cluster_mode: ClusterMode,
    /// Number of registered tenants.
    pub tenant_count: u32,
}

// ============================================================================
// Response Types
// ============================================================================

/// A server response to a client request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    /// Request ID this is responding to.
    pub request_id: RequestId,
    /// The response payload.
    pub payload: ResponsePayload,
}

impl Response {
    /// Creates a new response.
    pub fn new(request_id: RequestId, payload: ResponsePayload) -> Self {
        Self {
            request_id,
            payload,
        }
    }

    /// Creates an error response.
    pub fn error(request_id: RequestId, code: ErrorCode, message: String) -> Self {
        Self {
            request_id,
            payload: ResponsePayload::Error(ErrorResponse { code, message }),
        }
    }

    /// Encodes the response to a frame.
    pub fn to_frame(&self) -> WireResult<Frame> {
        let payload =
            postcard::to_allocvec(self).map_err(|e| WireError::Serialization(e.to_string()))?;
        Ok(Frame::new(Bytes::from(payload)))
    }

    /// Decodes a response from a frame.
    pub fn from_frame(frame: &Frame) -> WireResult<Self> {
        postcard::from_bytes(&frame.payload).map_err(WireError::from)
    }
}

// ============================================================================
// Push (server-initiated) messages
// ============================================================================

/// A server-initiated push frame (no `RequestId`; correlated by
/// `subscription_id` inside the payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Push {
    pub payload: PushPayload,
}

impl Push {
    pub fn new(payload: PushPayload) -> Self {
        Self { payload }
    }
}

/// Payload of a server-initiated [`Push`] frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PushPayload {
    /// A batch of stream events for an active subscription. The server sends
    /// these as events arrive, bounded by the subscription's credit balance.
    SubscriptionEvents {
        subscription_id: u64,
        /// Starting offset of the first event in `events`.
        start_offset: Offset,
        /// Event payloads, in stream order.
        events: Vec<Vec<u8>>,
        /// Remaining server-side credit balance after this batch.
        credits_remaining: u32,
    },
    /// Subscription has been closed. No further push frames will arrive.
    SubscriptionClosed {
        subscription_id: u64,
        reason: SubscriptionCloseReason,
    },
}

/// Why a subscription ended.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SubscriptionCloseReason {
    /// Client explicitly called `Unsubscribe`.
    ClientCancelled,
    /// Server shutting down / losing leadership.
    ServerShutdown,
    /// Stream was deleted.
    StreamDeleted,
    /// Client failed to keep up and hit the backpressure hard-limit.
    BackpressureTimeout,
    /// Protocol error on the subscription (e.g. unknown subscription ID).
    ProtocolError,
}

// ============================================================================
// Message enum (top-level wire multiplexer, v2)
// ============================================================================

/// Top-level wire message — discriminates client requests, server responses,
/// and server-initiated push frames.
///
/// Added in protocol v2. Use [`Message::from_frame`] / [`Message::to_frame`]
/// to encode and decode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    Request(Request),
    Response(Response),
    Push(Push),
}

impl Message {
    /// Encodes this message to a wire [`Frame`].
    pub fn to_frame(&self) -> WireResult<Frame> {
        let payload =
            postcard::to_allocvec(self).map_err(|e| WireError::Serialization(e.to_string()))?;
        Ok(Frame::new(Bytes::from(payload)))
    }

    /// Decodes a [`Frame`] into a message.
    pub fn from_frame(frame: &Frame) -> WireResult<Self> {
        postcard::from_bytes(&frame.payload).map_err(WireError::from)
    }
}

/// Response payload variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponsePayload {
    /// Error response.
    Error(ErrorResponse),
    /// Handshake response.
    Handshake(HandshakeResponse),
    /// Create stream response.
    CreateStream(CreateStreamResponse),
    /// Append events response.
    AppendEvents(AppendEventsResponse),
    /// Query response.
    Query(QueryResponse),
    /// Query at response.
    QueryAt(QueryResponse),
    /// Read events response.
    ReadEvents(ReadEventsResponse),
    /// Subscribe response (initial acknowledgment).
    Subscribe(SubscribeResponse),
    /// Sync response.
    Sync(SyncResponse),
    /// Acknowledgement for `SubscribeCredit` / `Unsubscribe`.
    SubscriptionAck(SubscriptionAckResponse),

    // ---- Phase 4 ----
    ListTables(ListTablesResponse),
    DescribeTable(DescribeTableResponse),
    ListIndexes(ListIndexesResponse),
    TenantCreate(TenantCreateResponse),
    TenantList(TenantListResponse),
    TenantDelete(TenantDeleteResponse),
    TenantGet(TenantGetResponse),
    ApiKeyRegister(ApiKeyRegisterResponse),
    ApiKeyRevoke(ApiKeyRevokeResponse),
    ApiKeyList(ApiKeyListResponse),
    ApiKeyRotate(ApiKeyRotateResponse),
    ServerInfo(ServerInfoResponse),
}

/// Generic ack for subscription lifecycle requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionAckResponse {
    pub subscription_id: u64,
    /// Remaining credit balance (after grant) or `0` for Unsubscribe.
    pub credits_remaining: u32,
}

/// Error response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// Error code.
    pub code: ErrorCode,
    /// Human-readable error message.
    pub message: String,
}

/// Error codes for wire protocol errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u16)]
pub enum ErrorCode {
    /// Unknown error.
    Unknown = 0,
    /// Internal server error.
    InternalError = 1,
    /// Invalid request format.
    InvalidRequest = 2,
    /// Authentication failed.
    AuthenticationFailed = 3,
    /// Tenant not found.
    TenantNotFound = 4,
    /// Stream not found.
    StreamNotFound = 5,
    /// Table not found.
    TableNotFound = 6,
    /// Query parse error.
    QueryParseError = 7,
    /// Query execution error.
    QueryExecutionError = 8,
    /// Position ahead of current.
    PositionAhead = 9,
    /// Stream already exists.
    StreamAlreadyExists = 10,
    /// Invalid stream offset.
    InvalidOffset = 11,
    /// Storage error.
    StorageError = 12,
    /// Projection lag.
    ProjectionLag = 13,
    /// Rate limit exceeded.
    RateLimited = 14,
    /// Not the leader - client should retry on another node.
    ///
    /// This error is returned in cluster mode when a write request
    /// is sent to a follower replica. The error message may include
    /// a leader hint to help the client redirect.
    NotLeader = 15,
    /// Offset mismatch — optimistic concurrency conflict.
    ///
    /// The client's expected offset doesn't match the stream's current
    /// offset. This is a retryable conflict: re-read the stream position
    /// and retry the append.
    OffsetMismatch = 16,
    /// Subscription ID not found on the server.
    ///
    /// The subscription was never created or has already been closed.
    SubscriptionNotFound = 17,
    /// Subscription has been closed (by the server or via `Unsubscribe`).
    ///
    /// Any further requests targeting the subscription ID are rejected.
    SubscriptionClosed = 18,
    /// Subscription backpressure — the client owes credits before more
    /// events can be pushed.
    SubscriptionBackpressure = 19,
    /// API key not found in the server's registry. Returned by revoke / rotate
    /// / list when the plaintext key doesn't match any stored hash.
    ApiKeyNotFound = 20,
    /// `TenantCreate` received an ID that already has a registration with
    /// a *different* human-readable name. Idempotent registrations (same
    /// tenant_id, same name or no name) do not produce this error.
    TenantAlreadyExists = 21,
}

/// Handshake response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeResponse {
    /// Server protocol version.
    pub server_version: u16,
    /// Whether authentication succeeded.
    pub authenticated: bool,
    /// Server capabilities.
    pub capabilities: Vec<String>,
}

/// Create stream response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateStreamResponse {
    /// The created stream ID.
    pub stream_id: StreamId,
}

/// Append events response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendEventsResponse {
    /// Offset of the first appended event.
    pub first_offset: Offset,
    /// Number of events appended.
    pub count: u32,
}

/// Query response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResponse {
    /// Column names.
    pub columns: Vec<String>,
    /// Rows of data.
    pub rows: Vec<Vec<QueryValue>>,
}

/// Query result value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueryValue {
    /// Null value.
    Null,
    /// 64-bit integer.
    BigInt(i64),
    /// Text string.
    Text(String),
    /// Boolean.
    Boolean(bool),
    /// Timestamp (nanoseconds since epoch).
    Timestamp(i64),
}

/// Read events response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadEventsResponse {
    /// The events.
    pub events: Vec<Vec<u8>>,
    /// Next offset to read from (for pagination).
    pub next_offset: Option<Offset>,
}

/// Subscribe response (initial acknowledgment).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeResponse {
    /// Subscription ID for this subscription.
    pub subscription_id: u64,
    /// The offset the server will start streaming from.
    pub start_offset: Offset,
    /// Initial credits acknowledged by the server.
    pub credits: u32,
}

/// Sync response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResponse {
    /// Whether sync completed successfully.
    pub success: bool,
}

#[cfg(test)]
mod message_tests {
    use super::*;

    #[test]
    fn test_request_roundtrip() {
        let request = Request::new(
            RequestId::new(1),
            TenantId::new(42),
            RequestPayload::CreateStream(CreateStreamRequest {
                name: "test-stream".to_string(),
                data_class: DataClass::Public,
                placement: Placement::Global,
            }),
        );

        // Encode to frame
        let frame = request.to_frame().unwrap();

        // Decode from frame
        let decoded = Request::from_frame(&frame).unwrap();

        assert_eq!(decoded.id, request.id);
        assert_eq!(u64::from(decoded.tenant_id), 42);
    }

    #[test]
    fn test_response_roundtrip() {
        let response = Response::new(
            RequestId::new(1),
            ResponsePayload::CreateStream(CreateStreamResponse {
                stream_id: StreamId::new(100),
            }),
        );

        // Encode to frame
        let frame = response.to_frame().unwrap();

        // Decode from frame
        let decoded = Response::from_frame(&frame).unwrap();

        assert_eq!(decoded.request_id, response.request_id);
    }

    #[test]
    fn test_error_response() {
        let response = Response::error(
            RequestId::new(1),
            ErrorCode::StreamNotFound,
            "stream 123 not found".to_string(),
        );

        let frame = response.to_frame().unwrap();
        let decoded = Response::from_frame(&frame).unwrap();

        if let ResponsePayload::Error(err) = decoded.payload {
            assert_eq!(err.code, ErrorCode::StreamNotFound);
            assert_eq!(err.message, "stream 123 not found");
        } else {
            panic!("expected error payload");
        }
    }

    #[test]
    fn test_query_params() {
        let request = Request::new(
            RequestId::new(2),
            TenantId::new(1),
            RequestPayload::Query(QueryRequest {
                sql: "SELECT * FROM events WHERE id = $1".to_string(),
                params: vec![
                    QueryParam::BigInt(42),
                    QueryParam::Text("hello".to_string()),
                    QueryParam::Boolean(true),
                    QueryParam::Null,
                ],
            }),
        );

        let frame = request.to_frame().unwrap();
        let decoded = Request::from_frame(&frame).unwrap();

        if let RequestPayload::Query(q) = decoded.payload {
            assert_eq!(q.params.len(), 4);
        } else {
            panic!("expected query payload");
        }
    }
}
