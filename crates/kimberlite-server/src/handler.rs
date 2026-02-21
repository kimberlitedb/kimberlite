//! Request handler that routes requests to Kimberlite.

use kimberlite::{Kimberlite, Offset};
use kimberlite_query::Value;
use kimberlite_types::Timestamp;
use kimberlite_wire::{
    AppendEventsResponse, CreateStreamResponse, ErrorCode, ErrorResponse, HandshakeResponse,
    PROTOCOL_VERSION, QueryParam, QueryResponse, QueryValue, ReadEventsResponse, Request,
    RequestPayload, Response, ResponsePayload, SubscribeResponse, SyncResponse,
};
use tracing::instrument;

use crate::auth::AuthService;
use crate::error::{ServerError, ServerResult};
use crate::metrics;
use crate::replication::CommandSubmitter;

/// Handles requests by routing them to the appropriate Kimberlite operations.
pub struct RequestHandler {
    /// The command submitter (wraps Kimberlite with optional replication).
    submitter: CommandSubmitter,
    /// Authentication service for validating handshake tokens.
    auth_service: AuthService,
}

impl RequestHandler {
    /// Creates a new request handler with a command submitter and auth service.
    pub fn new(submitter: CommandSubmitter, auth_service: AuthService) -> Self {
        Self {
            submitter,
            auth_service,
        }
    }

    /// Creates a new request handler with direct Kimberlite access (no replication).
    #[allow(dead_code)] // Available for direct testing without replication
    pub fn new_direct(db: Kimberlite) -> Self {
        Self {
            submitter: CommandSubmitter::Direct { db },
            auth_service: AuthService::new(crate::auth::AuthMode::None),
        }
    }

    /// Returns a reference to the underlying Kimberlite instance.
    pub fn kimberlite(&self) -> &Kimberlite {
        self.submitter.kimberlite()
    }

    /// Returns a reference to the authentication service.
    pub fn auth_service(&self) -> &AuthService {
        &self.auth_service
    }

    /// Handles a request and returns a response.
    #[instrument(skip_all, fields(request_id))]
    pub fn handle(&self, request: Request) -> Response {
        let request_id = request.id;
        tracing::Span::current().record("request_id", request_id.0);

        match self.handle_inner(request) {
            Ok(payload) => Response::new(request_id, payload),
            Err(e) => {
                let (code, message) = error_to_wire(&e);
                Response::error(request_id, code, message)
            }
        }
    }

    #[instrument(skip_all, fields(op))]
    fn handle_inner(&self, request: Request) -> ServerResult<ResponsePayload> {
        let tenant = self.kimberlite().tenant(request.tenant_id);

        match request.payload {
            RequestPayload::Handshake(req) => {
                tracing::Span::current().record("op", "handshake");
                // Version check
                if req.client_version != PROTOCOL_VERSION {
                    return Ok(ResponsePayload::Error(ErrorResponse {
                        code: ErrorCode::InvalidRequest,
                        message: format!(
                            "unsupported client version: {}, server is {}",
                            req.client_version, PROTOCOL_VERSION
                        ),
                    }));
                }

                // Authenticate using the real auth service
                let auth_result = self.auth_service.authenticate(req.auth_token.as_deref());
                let authenticated = auth_result.is_ok();

                // Record auth metrics
                let method = if req.auth_token.as_deref().is_some_and(|t| t.contains('.')) {
                    "jwt"
                } else if req.auth_token.is_some() {
                    "api_key"
                } else {
                    "none"
                };
                metrics::record_auth_attempt(method, authenticated);

                if let Err(e) = &auth_result {
                    // If auth is required (not AuthMode::None) and fails, reject
                    if !matches!(e, ServerError::Unauthorized(_)) || req.auth_token.is_some() {
                        // Only reject if a token was provided and failed validation
                        if req.auth_token.is_some() && !authenticated {
                            return Err(ServerError::Unauthorized(
                                "authentication failed".to_string(),
                            ));
                        }
                    }
                }

                Ok(ResponsePayload::Handshake(HandshakeResponse {
                    server_version: PROTOCOL_VERSION,
                    authenticated,
                    capabilities: vec![
                        "query".to_string(),
                        "append".to_string(),
                        "subscribe".to_string(),
                    ],
                }))
            }

            RequestPayload::CreateStream(req) => {
                tracing::Span::current().record("op", "create_stream");
                let stream_id = tenant.create_stream(&req.name, req.data_class)?;
                Ok(ResponsePayload::CreateStream(CreateStreamResponse {
                    stream_id,
                }))
            }

            RequestPayload::AppendEvents(req) => {
                tracing::Span::current().record("op", "append_events");
                let first_offset =
                    tenant.append(req.stream_id, req.events.clone(), req.expected_offset)?;
                Ok(ResponsePayload::AppendEvents(AppendEventsResponse {
                    first_offset,
                    count: req.events.len() as u32,
                }))
            }

            RequestPayload::Query(req) => {
                tracing::Span::current().record("op", "query");
                let params = convert_params(&req.params);

                // Check if this is a SELECT query or a DDL/DML statement
                // Use a simple heuristic: if it starts with SELECT (case-insensitive), route to query
                let trimmed_sql = req.sql.trim_start();
                let is_select =
                    trimmed_sql.len() >= 6 && trimmed_sql[..6].eq_ignore_ascii_case("SELECT");

                if is_select {
                    // Route to query engine (read path)
                    let result = tenant.query(&req.sql, &params)?;
                    Ok(ResponsePayload::Query(convert_query_result(&result)))
                } else {
                    // Route to execute (write path for DDL/DML)
                    let exec_result = tenant.execute(&req.sql, &params)?;

                    // Return empty result set with metadata
                    Ok(ResponsePayload::Query(QueryResponse {
                        columns: vec!["rows_affected".to_string(), "log_offset".to_string()],
                        rows: vec![vec![
                            QueryValue::BigInt(exec_result.rows_affected() as i64),
                            QueryValue::BigInt(exec_result.log_offset().as_u64() as i64),
                        ]],
                    }))
                }
            }

            RequestPayload::QueryAt(req) => {
                tracing::Span::current().record("op", "query_at");
                let params = convert_params(&req.params);
                let result = tenant.query_at(&req.sql, &params, req.position)?;

                Ok(ResponsePayload::QueryAt(convert_query_result(&result)))
            }

            RequestPayload::ReadEvents(req) => {
                tracing::Span::current().record("op", "read_events");
                let events = tenant.read_events(req.stream_id, req.from_offset, req.max_bytes)?;

                // Calculate next offset for pagination
                let next_offset = if events.is_empty() {
                    None
                } else {
                    Some(Offset::new(req.from_offset.as_u64() + events.len() as u64))
                };

                Ok(ResponsePayload::ReadEvents(ReadEventsResponse {
                    events: events.into_iter().map(|b| b.to_vec()).collect(),
                    next_offset,
                }))
            }

            RequestPayload::Subscribe(req) => {
                tracing::Span::current().record("op", "subscribe");

                // Validate stream exists by reading zero events
                let _events = tenant.read_events(req.stream_id, req.from_offset, 0)?;

                // Generate a subscription ID from stream + tenant
                let subscription_id = u64::from(request.tenant_id)
                    .wrapping_mul(0x517cc1b727220a95)
                    .wrapping_add(u64::from(req.stream_id));

                Ok(ResponsePayload::Subscribe(SubscribeResponse {
                    subscription_id,
                    start_offset: req.from_offset,
                    credits: req.initial_credits,
                }))
            }

            RequestPayload::Sync(_) => {
                tracing::Span::current().record("op", "sync");
                self.kimberlite().sync()?;
                Ok(ResponsePayload::Sync(SyncResponse { success: true }))
            }
        }
    }
}

/// Converts wire query parameters to Kimberlite query values.
fn convert_params(params: &[QueryParam]) -> Vec<Value> {
    params
        .iter()
        .map(|p| match p {
            QueryParam::Null => Value::Null,
            QueryParam::BigInt(v) => Value::BigInt(*v),
            QueryParam::Text(v) => Value::Text(v.clone()),
            QueryParam::Boolean(v) => Value::Boolean(*v),
            // Negative timestamps are treated as 0 (epoch)
            QueryParam::Timestamp(v) => {
                #[allow(clippy::cast_sign_loss)]
                let nanos = if *v < 0 { 0 } else { *v as u64 };
                Value::Timestamp(Timestamp::from_nanos(nanos))
            }
        })
        .collect()
}

/// Converts a Kimberlite query result to a wire response.
fn convert_query_result(result: &kimberlite_query::QueryResult) -> QueryResponse {
    let columns = result.columns.iter().map(ToString::to_string).collect();

    let rows = result
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|v| match v {
                    Value::Null => QueryValue::Null,
                    Value::TinyInt(n) => QueryValue::BigInt(i64::from(*n)),
                    Value::SmallInt(n) => QueryValue::BigInt(i64::from(*n)),
                    Value::Integer(n) => QueryValue::BigInt(i64::from(*n)),
                    Value::BigInt(n) => QueryValue::BigInt(*n),
                    Value::Real(f) => {
                        // Transmit as text to preserve precision
                        QueryValue::Text(f.to_string())
                    }
                    Value::Decimal(val, scale) => {
                        let divisor = 10_i128.pow(u32::from(*scale));
                        let float_val = *val as f64 / divisor as f64;
                        QueryValue::Text(float_val.to_string())
                    }
                    Value::Text(s) => QueryValue::Text(s.clone()),
                    Value::Bytes(b) => {
                        // Encode bytes as base64 text for wire transmission
                        use base64::Engine;
                        let encoded = base64::engine::general_purpose::STANDARD.encode(b);
                        QueryValue::Text(encoded)
                    }
                    Value::Boolean(b) => QueryValue::Boolean(*b),
                    Value::Date(days) => QueryValue::Text(format!("Date({days})")),
                    Value::Time(nanos) => QueryValue::Text(format!("Time({nanos})")),
                    #[allow(clippy::cast_possible_wrap)]
                    Value::Timestamp(t) => QueryValue::Timestamp(t.as_nanos() as i64),
                    Value::Uuid(bytes) => {
                        let uuid_str = format!(
                            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                            bytes[0], bytes[1], bytes[2], bytes[3],
                            bytes[4], bytes[5],
                            bytes[6], bytes[7],
                            bytes[8], bytes[9],
                            bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
                        );
                        QueryValue::Text(uuid_str)
                    }
                    Value::Json(j) => QueryValue::Text(j.to_string()),
                    Value::Placeholder(idx) => {
                        panic!("Cannot convert unbound placeholder ${idx} - bind parameters first")
                    }
                })
                .collect()
        })
        .collect();

    QueryResponse { columns, rows }
}

/// Converts a server error to a wire error code and message.
fn error_to_wire(error: &ServerError) -> (ErrorCode, String) {
    match error {
        ServerError::Wire(e) => (ErrorCode::InvalidRequest, e.to_string()),
        ServerError::Database(e) => match e {
            kimberlite::KimberliteError::TenantNotFound(_) => {
                (ErrorCode::TenantNotFound, e.to_string())
            }
            kimberlite::KimberliteError::StreamNotFound(_) => {
                (ErrorCode::StreamNotFound, e.to_string())
            }
            kimberlite::KimberliteError::TableNotFound(_) => {
                (ErrorCode::TableNotFound, e.to_string())
            }
            kimberlite::KimberliteError::PositionAhead { .. } => {
                (ErrorCode::PositionAhead, e.to_string())
            }
            kimberlite::KimberliteError::ProjectionLag { .. } => {
                (ErrorCode::ProjectionLag, e.to_string())
            }
            kimberlite::KimberliteError::Query(qe) => (ErrorCode::QueryParseError, qe.to_string()),
            kimberlite::KimberliteError::Storage(_) | kimberlite::KimberliteError::Store(_) => {
                (ErrorCode::StorageError, e.to_string())
            }
            kimberlite::KimberliteError::Kernel(ke) => {
                if let kimberlite::KernelError::UnexpectedStreamOffset { .. } = &ke {
                    (ErrorCode::OffsetMismatch, ke.to_string())
                } else {
                    let msg = ke.to_string();
                    if msg.contains("not found") {
                        (ErrorCode::StreamNotFound, msg)
                    } else if msg.contains("already exists") || msg.contains("unique") {
                        (ErrorCode::StreamAlreadyExists, msg)
                    } else {
                        (ErrorCode::InternalError, msg)
                    }
                }
            }
            _ => (ErrorCode::InternalError, e.to_string()),
        },
        ServerError::Io(e) => (ErrorCode::InternalError, e.to_string()),
        ServerError::ConnectionClosed => {
            (ErrorCode::InternalError, "connection closed".to_string())
        }
        ServerError::MaxConnectionsReached(n) => (
            ErrorCode::InternalError,
            format!("max connections reached: {n}"),
        ),
        ServerError::InvalidTenant => (ErrorCode::TenantNotFound, "invalid tenant".to_string()),
        ServerError::BindFailed { addr, source } => (
            ErrorCode::InternalError,
            format!("bind failed on {addr}: {source}"),
        ),
        ServerError::Tls(msg) => (ErrorCode::InternalError, format!("TLS error: {msg}")),
        ServerError::Unauthorized(msg) => (ErrorCode::AuthenticationFailed, msg.clone()),
        ServerError::Shutdown => (ErrorCode::InternalError, "server shutdown".to_string()),
        ServerError::Replication(msg) => (ErrorCode::InternalError, format!("replication: {msg}")),
        ServerError::NotLeader { view, leader_hint } => {
            let hint = leader_hint.map_or_else(|| "unknown".to_string(), |addr| addr.to_string());
            (
                ErrorCode::NotLeader,
                format!("not the leader (view: {view}, leader hint: {hint})"),
            )
        }
        ServerError::ClusterConfig(msg) => (
            ErrorCode::InternalError,
            format!("cluster configuration error: {msg}"),
        ),
        ServerError::ServerBusy => (
            ErrorCode::RateLimited,
            "server busy, try again later".to_string(),
        ),
    }
}
