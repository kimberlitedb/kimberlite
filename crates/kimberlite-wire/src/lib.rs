//! # kmb-wire: Binary wire protocol for `Kimberlite`
//!
//! This crate opts in to strict PRESSURECRAFT clippy lints — see
//! `docs-internal/contributing/constructor-audit-2026-04.md` for policy.
//!
//! This crate defines the binary wire protocol used for client-server
//! communication in `Kimberlite`.
//!
//! ## Frame Format
//!
//! ```text
//! ┌─────────┬─────────┬──────────┬──────────┬──────────────────┐
//! │ Magic   │ Version │ Length   │ Checksum │     Payload      │
//! │ (4 B)   │ (2 B)   │ (4 B)    │ (4 B)    │     (var)        │
//! └─────────┴─────────┴──────────┴──────────┴──────────────────┘
//! ```
//!
//! - **Magic**: `0x56444220` ("VDB ")
//! - **Version**: Protocol version (currently 1)
//! - **Length**: Payload length in bytes (max 16 MiB)
//! - **Checksum**: CRC32 of payload
//! - **Payload**: Bincode-encoded message
//!
//! ## Message Types
//!
//! Messages are either requests (client → server) or responses (server → client).
//! Each request has a corresponding response type.

#![warn(
    clippy::unwrap_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::too_many_lines
)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::too_many_lines
    )
)]

mod error;
mod frame;
mod message;

pub use error::{WireError, WireResult};
pub use frame::{FRAME_HEADER_SIZE, Frame, FrameHeader, MAGIC, MAX_PAYLOAD_SIZE, PROTOCOL_VERSION};
pub use message::{
    AppendEventsRequest, AppendEventsResponse, ApiKeyInfo, ApiKeyListRequest, ApiKeyListResponse,
    ApiKeyRegisterRequest, ApiKeyRegisterResponse, ApiKeyRevokeRequest, ApiKeyRevokeResponse,
    ApiKeyRotateRequest, ApiKeyRotateResponse, AuditEventInfo, AuditMetadata, AuditQueryRequest,
    AuditQueryResponse, BreachConfirmRequest, BreachConfirmResponse, BreachEventInfo,
    BreachIndicatorPayload, BreachQueryStatusRequest, BreachQueryStatusResponse, BreachReportInfo,
    BreachReportIndicatorRequest, BreachReportIndicatorResponse, BreachResolveRequest,
    BreachResolveResponse, BreachSeverity, BreachStatusTag, ClusterMode, ColumnInfo,
    ConsentCheckRequest, ConsentCheckResponse, ConsentGrantRequest, ConsentGrantResponse,
    ConsentListRequest, ConsentListResponse, ConsentPurpose, ConsentRecord, ConsentScope,
    ConsentWithdrawRequest, ConsentWithdrawResponse, CreateStreamRequest, CreateStreamResponse,
    DescribeTableRequest, DescribeTableResponse, ErasureAuditInfo, ErasureCompleteRequest,
    ErasureCompleteResponse, ErasureExemptRequest, ErasureExemptResponse, ErasureExemptionBasis,
    ErasureListRequest, ErasureListResponse, ErasureMarkProgressRequest,
    ErasureMarkProgressResponse, ErasureMarkStreamErasedRequest, ErasureMarkStreamErasedResponse,
    ErasureRequestInfo, ErasureRequestRequest, ErasureRequestResponse, ErasureStatusRequest,
    ErasureStatusResponse, ErasureStatusTag, ErrorCode, ErrorResponse, ExportFormat,
    ExportSubjectRequest, ExportSubjectResponse, GetServerInfoRequest, HandshakeRequest,
    HandshakeResponse, IndexInfo, ListIndexesRequest, ListIndexesResponse, ListTablesRequest,
    ListTablesResponse, Message, PortabilityExportInfo, Push, PushPayload, QueryAtRequest,
    QueryParam, QueryRequest, QueryResponse, QueryValue, ReadEventsRequest, ReadEventsResponse,
    Request, RequestId, RequestPayload, Response, ResponsePayload, ServerInfoResponse,
    SubscribeCreditRequest, SubscribeRequest, SubscribeResponse, SubscriptionAckResponse,
    SubscriptionCloseReason, SyncRequest, SyncResponse, TableInfo, TenantCreateRequest,
    TenantCreateResponse, TenantDeleteRequest, TenantDeleteResponse, TenantGetRequest,
    TenantGetResponse, TenantInfo, TenantListRequest, TenantListResponse, UnsubscribeRequest,
    VerifyExportRequest, VerifyExportResponse,
};

#[cfg(test)]
mod tests;
