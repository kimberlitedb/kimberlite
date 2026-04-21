//! Async (tokio) Kimberlite RPC client — AUDIT-2026-04 S2.1.
//!
//! Mirrors [`crate::Client`] but every method is `async fn`. A
//! background task owns the TCP socket; `AsyncClient` handles
//! request/response correlation via per-request `oneshot` channels
//! keyed on `request_id`. Server-initiated `Push` frames are
//! delivered to a per-subscription `mpsc::UnboundedSender<Push>`
//! registered when [`AsyncClient::subscribe`] is called.
//!
//! # Architecture
//!
//! ```text
//!     caller                         reader task
//!        │                                  │
//!        │ send Request                     │
//!        ├──► request_tx ──► writer half ───┤ writes frames to socket
//!        │   + oneshot::Sender<Response>    │
//!        │                                  │ reads frames from socket
//!        │                                  ├── Response → match request_id
//!        │   awaits oneshot                 │   → fire matching oneshot
//!        │ ◄────────────────────────────────┤
//!        │                                  ├── Push → fire per-subscription mpsc
//! ```
//!
//! The reader task keeps a `HashMap<u64, oneshot::Sender<Response>>`
//! of in-flight requests; if the socket closes, every pending
//! responder is fired with [`ClientError::NotConnected`] so callers
//! see a clean failure rather than hanging forever.
//!
//! # Feature flag
//!
//! Gated behind the `tokio` cargo feature (default-on). Building
//! without `tokio` keeps `kimberlite-client` runtime-agnostic.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::BytesMut;
use kimberlite_types::{DataClass, Offset, Placement, StreamId, TenantId};
use kimberlite_wire::{
    AppendEventsRequest, CreateStreamRequest, ErrorCode, HandshakeRequest, Message,
    PROTOCOL_VERSION, Push, QueryAtRequest, QueryParam, QueryRequest, QueryResponse,
    ReadEventsRequest, ReadEventsResponse, Request, RequestId, RequestPayload, Response,
    ResponsePayload, SubscribeRequest, SubscribeResponse, SyncRequest,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::error::{ClientError, ClientResult};
use crate::framing::{decode_frame, decode_message, encode_request};

/// The in-flight request table: a per-request-id slot holding the
/// oneshot sender that fulfils the caller's awaited response. Shared
/// across the writer, reader, and fail-pending paths.
type PendingResponders = Arc<Mutex<HashMap<u64, oneshot::Sender<ClientResult<Response>>>>>;

/// Match [`crate::ClientConfig`] field names so callers can reuse the
/// same struct for both clients (different defaults though — async
/// has no socket-level read/write timeouts since reads are driven
/// by the reader task).
#[derive(Debug, Clone)]
pub struct AsyncClientConfig {
    /// Read buffer size for the reader task. 64 KiB is enough for any
    /// single response under the current wire protocol.
    pub buffer_size: usize,
    /// Optional auth token forwarded in the handshake.
    pub auth_token: Option<String>,
    /// Maximum in-flight requests waiting on a oneshot. The mpsc
    /// channel sized at `request_capacity` provides backpressure
    /// when the writer task can't keep up.
    pub request_capacity: usize,
}

impl Default for AsyncClientConfig {
    fn default() -> Self {
        Self {
            buffer_size: 64 * 1024,
            auth_token: None,
            request_capacity: 1024,
        }
    }
}

/// Async RPC client. Cheaply cloneable — each clone shares the same
/// background reader/writer tasks and request id counter.
#[derive(Clone)]
pub struct AsyncClient {
    inner: Arc<AsyncClientInner>,
}

struct AsyncClientInner {
    tenant_id: TenantId,
    next_request_id: AtomicU64,
    request_tx: mpsc::Sender<OutboundRequest>,
    /// Subscription dispatch table. Keyed by `subscription_id`
    /// returned from a successful [`AsyncClient::subscribe`].
    push_routes: Mutex<HashMap<u64, mpsc::UnboundedSender<Push>>>,
    /// Reader task handle — kept alive so dropping the client also
    /// stops the background task. Wrapped in `Mutex<Option<...>>`
    /// so `disconnect()` can take ownership and await the join.
    #[allow(dead_code)] // held to keep the reader task alive
    reader_task: Mutex<Option<JoinHandle<()>>>,
    /// Writer task handle. Same shape as reader_task.
    #[allow(dead_code)] // held to keep the writer task alive
    writer_task: Mutex<Option<JoinHandle<()>>>,
}

/// Internal envelope sent from caller → writer task. Carries the
/// request bytes plus the oneshot the reader task should fire when
/// the matching response arrives.
struct OutboundRequest {
    request: Request,
    responder: oneshot::Sender<ClientResult<Response>>,
}

impl AsyncClient {
    /// Connect to a Kimberlite server, perform the handshake, spawn
    /// the reader + writer tasks, and return a ready client.
    ///
    /// # Errors
    ///
    /// - [`ClientError::Connection`] — TCP connect failed.
    /// - [`ClientError::HandshakeFailed`] — server rejected the
    ///   protocol version or auth token.
    pub async fn connect(
        addr: impl tokio::net::ToSocketAddrs,
        tenant_id: TenantId,
        config: AsyncClientConfig,
    ) -> ClientResult<Self> {
        let stream = TcpStream::connect(addr).await?;
        let (read_half, write_half) = stream.into_split();

        let (request_tx, request_rx) = mpsc::channel::<OutboundRequest>(config.request_capacity);
        let pending: PendingResponders = Arc::new(Mutex::new(HashMap::new()));
        let push_routes: Arc<Mutex<HashMap<u64, mpsc::UnboundedSender<Push>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let writer_task = tokio::spawn(writer_loop(write_half, request_rx, pending.clone()));
        let reader_task = tokio::spawn(reader_loop(
            read_half,
            pending.clone(),
            push_routes.clone(),
            config.buffer_size,
        ));

        let inner = Arc::new(AsyncClientInner {
            tenant_id,
            next_request_id: AtomicU64::new(1),
            request_tx,
            push_routes: Mutex::new(HashMap::new()),
            reader_task: Mutex::new(Some(reader_task)),
            writer_task: Mutex::new(Some(writer_task)),
        });
        // Replace the empty placeholder with the shared map the
        // reader task is actually using. Done after `Arc::new` so
        // the inner struct can hold the same routing table.
        *inner.push_routes.lock().await = std::mem::take(&mut *push_routes.lock().await);

        let client = Self { inner };
        // Handshake before returning — matches sync Client::connect.
        client.do_handshake(config.auth_token).await?;
        Ok(client)
    }

    /// Tenant id this client was opened for.
    pub fn tenant_id(&self) -> TenantId {
        self.inner.tenant_id
    }

    /// Run the protocol-version handshake.
    async fn do_handshake(&self, auth_token: Option<String>) -> ClientResult<()> {
        let response = self
            .send_request(RequestPayload::Handshake(HandshakeRequest {
                client_version: PROTOCOL_VERSION,
                auth_token,
            }))
            .await?;
        match response.payload {
            ResponsePayload::Handshake(h) if h.server_version == PROTOCOL_VERSION => Ok(()),
            ResponsePayload::Handshake(h) => Err(ClientError::HandshakeFailed(format!(
                "protocol version mismatch: client {}, server {}",
                PROTOCOL_VERSION, h.server_version
            ))),
            ResponsePayload::Error(e) => Err(ClientError::server(e.code, e.message)),
            other => Err(ClientError::UnexpectedResponse {
                expected: "Handshake".into(),
                actual: format!("{other:?}"),
            }),
        }
    }

    /// Issue a request and await its response. Single internal
    /// dispatch path — every public `async fn` calls this.
    async fn send_request(&self, payload: RequestPayload) -> ClientResult<Response> {
        let request_id = RequestId::new(self.inner.next_request_id.fetch_add(1, Ordering::SeqCst));
        let audit = crate::audit_context::current_audit().map(|c| c.to_wire());
        let request = Request::with_audit(request_id, self.inner.tenant_id, audit, payload);

        let (responder, response_rx) = oneshot::channel();
        self.inner
            .request_tx
            .send(OutboundRequest { request, responder })
            .await
            .map_err(|_| ClientError::NotConnected)?;
        match response_rx.await {
            Ok(result) => result,
            Err(_) => Err(ClientError::NotConnected),
        }
    }

    /// Async port of [`crate::Client::create_stream`].
    pub async fn create_stream(&self, name: &str, data_class: DataClass) -> ClientResult<StreamId> {
        let response = self
            .send_request(RequestPayload::CreateStream(CreateStreamRequest {
                name: name.to_string(),
                data_class,
                placement: Placement::Global,
            }))
            .await?;
        match response.payload {
            ResponsePayload::CreateStream(r) => Ok(r.stream_id),
            ResponsePayload::Error(e) => Err(ClientError::server(e.code, e.message)),
            other => Err(ClientError::UnexpectedResponse {
                expected: "CreateStream".into(),
                actual: format!("{other:?}"),
            }),
        }
    }

    /// Async port of [`crate::Client::append`].
    pub async fn append(
        &self,
        stream_id: StreamId,
        events: Vec<Vec<u8>>,
        expected_offset: Offset,
    ) -> ClientResult<Offset> {
        let response = self
            .send_request(RequestPayload::AppendEvents(AppendEventsRequest {
                stream_id,
                events,
                expected_offset,
            }))
            .await?;
        match response.payload {
            ResponsePayload::AppendEvents(r) => Ok(r.first_offset),
            ResponsePayload::Error(e) => Err(ClientError::server(e.code, e.message)),
            other => Err(ClientError::UnexpectedResponse {
                expected: "AppendEvents".into(),
                actual: format!("{other:?}"),
            }),
        }
    }

    /// Async port of [`crate::Client::query`].
    pub async fn query(&self, sql: &str, params: &[QueryParam]) -> ClientResult<QueryResponse> {
        let response = self
            .send_request(RequestPayload::Query(QueryRequest {
                sql: sql.to_string(),
                params: params.to_vec(),
                break_glass_reason: None,
            }))
            .await?;
        match response.payload {
            ResponsePayload::Query(r) => Ok(r),
            ResponsePayload::Error(e) => Err(ClientError::server(e.code, e.message)),
            other => Err(ClientError::UnexpectedResponse {
                expected: "Query".into(),
                actual: format!("{other:?}"),
            }),
        }
    }

    /// Async port of [`crate::Client::query_at`].
    pub async fn query_at(
        &self,
        sql: &str,
        params: &[QueryParam],
        position: Offset,
    ) -> ClientResult<QueryResponse> {
        let response = self
            .send_request(RequestPayload::QueryAt(QueryAtRequest {
                sql: sql.to_string(),
                params: params.to_vec(),
                position,
                break_glass_reason: None,
            }))
            .await?;
        match response.payload {
            ResponsePayload::QueryAt(r) => Ok(r),
            ResponsePayload::Error(e) => Err(ClientError::server(e.code, e.message)),
            other => Err(ClientError::UnexpectedResponse {
                expected: "QueryAt".into(),
                actual: format!("{other:?}"),
            }),
        }
    }

    /// Async port of [`crate::Client::query_at_clause`].
    ///
    /// v0.6.0 Tier 2 #6 — accepts any of the three [`crate::AtClause`]
    /// forms (offset, nanos, `DateTime<Utc>`). Timestamp forms are
    /// carried by rewriting the SQL with an inline `AS OF TIMESTAMP
    /// '<iso>'` suffix, preserving the v4 wire protocol unchanged.
    pub async fn query_at_clause(
        &self,
        sql: &str,
        params: &[QueryParam],
        at: crate::AtClause,
    ) -> ClientResult<QueryResponse> {
        match at {
            crate::AtClause::Offset(position) => self.query_at(sql, params, position).await,
            crate::AtClause::TimestampNs(ns) => {
                let ts = chrono::DateTime::<chrono::Utc>::from_timestamp_nanos(ns);
                let with_clause = format!(
                    "{} AS OF TIMESTAMP '{}'",
                    sql.trim_end_matches(';'),
                    ts.to_rfc3339()
                );
                let response = self
                    .send_request(RequestPayload::Query(QueryRequest {
                        sql: with_clause,
                        params: params.to_vec(),
                        break_glass_reason: None,
                    }))
                    .await?;
                match response.payload {
                    ResponsePayload::Query(r) | ResponsePayload::QueryAt(r) => Ok(r),
                    ResponsePayload::Error(e) => Err(ClientError::server(e.code, e.message)),
                    other => Err(ClientError::UnexpectedResponse {
                        expected: "Query or QueryAt".into(),
                        actual: format!("{other:?}"),
                    }),
                }
            }
            crate::AtClause::Timestamp(dt) => {
                let ns = dt.timestamp_nanos_opt().ok_or_else(|| {
                    ClientError::UnexpectedResponse {
                        expected: "timestamp in representable range".into(),
                        actual: format!("{dt}"),
                    }
                })?;
                Box::pin(self.query_at_clause(sql, params, crate::AtClause::TimestampNs(ns))).await
            }
        }
    }

    /// Async port of [`crate::Client::execute`]. Returns
    /// `(rows_affected, log_offset)`.
    pub async fn execute(&self, sql: &str, params: &[QueryParam]) -> ClientResult<(u64, u64)> {
        let response = self.query(sql, params).await?;
        crate::client::extract_execute_result_for_async(&response).ok_or_else(|| {
            ClientError::server(
                ErrorCode::InternalError,
                format!(
                    "execute() called on non-DML statement; got columns {:?}",
                    response.columns
                ),
            )
        })
    }

    /// Async port of [`crate::Client::read_events`].
    pub async fn read_events(
        &self,
        stream_id: StreamId,
        from_offset: Offset,
        max_bytes: u64,
    ) -> ClientResult<ReadEventsResponse> {
        let response = self
            .send_request(RequestPayload::ReadEvents(ReadEventsRequest {
                stream_id,
                from_offset,
                max_bytes,
            }))
            .await?;
        match response.payload {
            ResponsePayload::ReadEvents(r) => Ok(r),
            ResponsePayload::Error(e) => Err(ClientError::server(e.code, e.message)),
            other => Err(ClientError::UnexpectedResponse {
                expected: "ReadEvents".into(),
                actual: format!("{other:?}"),
            }),
        }
    }

    /// Async port of [`crate::Client::sync`].
    pub async fn sync(&self) -> ClientResult<()> {
        let response = self
            .send_request(RequestPayload::Sync(SyncRequest {}))
            .await?;
        match response.payload {
            ResponsePayload::Sync(r) if r.success => Ok(()),
            ResponsePayload::Sync(_) => {
                Err(ClientError::server(ErrorCode::InternalError, "sync failed"))
            }
            ResponsePayload::Error(e) => Err(ClientError::server(e.code, e.message)),
            other => Err(ClientError::UnexpectedResponse {
                expected: "Sync".into(),
                actual: format!("{other:?}"),
            }),
        }
    }

    /// Async port of [`crate::Client::subscribe`].
    ///
    /// Returns the [`SubscribeResponse`] **and** an
    /// [`AsyncSubscription`] that yields server-pushed events as a
    /// stream of [`Push`] frames. The subscription's `Drop` impl
    /// removes the route from the dispatch table; a subsequent
    /// reconnect must re-issue the subscription.
    pub async fn subscribe(
        &self,
        stream_id: StreamId,
        from_offset: Offset,
        initial_credits: u32,
        consumer_group: Option<String>,
    ) -> ClientResult<(SubscribeResponse, AsyncSubscription)> {
        let response = self
            .send_request(RequestPayload::Subscribe(SubscribeRequest {
                stream_id,
                from_offset,
                initial_credits,
                consumer_group,
            }))
            .await?;
        let info = match response.payload {
            ResponsePayload::Subscribe(r) => r,
            ResponsePayload::Error(e) => return Err(ClientError::server(e.code, e.message)),
            other => {
                return Err(ClientError::UnexpectedResponse {
                    expected: "Subscribe".into(),
                    actual: format!("{other:?}"),
                });
            }
        };

        let (push_tx, push_rx) = mpsc::unbounded_channel();
        self.inner
            .push_routes
            .lock()
            .await
            .insert(info.subscription_id, push_tx);
        let sub = AsyncSubscription {
            subscription_id: info.subscription_id,
            push_rx,
            client: self.clone(),
        };
        Ok((info, sub))
    }
}

/// Async equivalent of [`crate::Subscription`]. Drains server-pushed
/// frames as they arrive on the underlying socket.
///
/// Implements the `tokio_stream::Stream` contract indirectly via
/// [`AsyncSubscription::recv`]; callers can wrap with
/// `tokio_stream::wrappers::UnboundedReceiverStream` if they want
/// the `Stream` trait.
pub struct AsyncSubscription {
    subscription_id: u64,
    push_rx: mpsc::UnboundedReceiver<Push>,
    client: AsyncClient,
}

impl AsyncSubscription {
    /// Server-assigned subscription id.
    pub fn id(&self) -> u64 {
        self.subscription_id
    }

    /// Await the next push frame. Returns `None` once the underlying
    /// socket closes or the subscription is dropped server-side.
    pub async fn recv(&mut self) -> Option<Push> {
        self.push_rx.recv().await
    }
}

impl Drop for AsyncSubscription {
    fn drop(&mut self) {
        // Synchronous best-effort cleanup: spawn a short task to
        // remove the route. We don't issue an Unsubscribe RPC here —
        // the server's connection-tear-down handles that — but we
        // must drop the sender so the reader task stops trying to
        // route here.
        let id = self.subscription_id;
        let client = self.client.clone();
        tokio::spawn(async move {
            client.inner.push_routes.lock().await.remove(&id);
        });
    }
}

// ============================================================================
// Background task loops
// ============================================================================

async fn writer_loop(
    mut write_half: OwnedWriteHalf,
    mut request_rx: mpsc::Receiver<OutboundRequest>,
    pending: PendingResponders,
) {
    let mut buf = BytesMut::new();
    while let Some(OutboundRequest { request, responder }) = request_rx.recv().await {
        buf.clear();
        if let Err(e) = encode_request(&request, &mut buf) {
            let _ = responder.send(Err(e));
            continue;
        }
        // Register the responder before flushing — if the response
        // races back faster than the unlock here, the reader task
        // would otherwise drop it on the floor.
        pending.lock().await.insert(request.id.0, responder);
        if let Err(e) = write_half.write_all(&buf).await {
            // Pull the responder back out so we can fire the
            // ConnectionError instead of leaking it.
            if let Some(r) = pending.lock().await.remove(&request.id.0) {
                let _ = r.send(Err(ClientError::Connection(e)));
            }
            break;
        }
        if let Err(e) = write_half.flush().await {
            if let Some(r) = pending.lock().await.remove(&request.id.0) {
                let _ = r.send(Err(ClientError::Connection(e)));
            }
            break;
        }
    }
    // Drain any remaining pending responders with NotConnected so
    // callers don't hang on a dropped writer task.
    let mut pending_guard = pending.lock().await;
    for (_, responder) in pending_guard.drain() {
        let _ = responder.send(Err(ClientError::NotConnected));
    }
}

async fn reader_loop(
    mut read_half: OwnedReadHalf,
    pending: PendingResponders,
    push_routes: Arc<Mutex<HashMap<u64, mpsc::UnboundedSender<Push>>>>,
    buffer_size: usize,
) {
    let mut buf = BytesMut::with_capacity(buffer_size);
    let mut chunk = vec![0u8; 4096];
    loop {
        // Try to decode any complete frames already in the buffer.
        loop {
            let frame = match decode_frame(&mut buf) {
                Ok(Some(f)) => f,
                Ok(None) => break,
                Err(e) => {
                    fail_all_pending(&pending, e).await;
                    return;
                }
            };
            let msg = match decode_message(&frame) {
                Ok(m) => m,
                Err(e) => {
                    fail_all_pending(&pending, e).await;
                    return;
                }
            };
            match msg {
                Message::Response(response) => {
                    let id = response.request_id.0;
                    if let Some(responder) = pending.lock().await.remove(&id) {
                        // AUDIT-2026-04 S3.8 — map server Error payloads to
                        // request-id-tagged client errors before handing
                        // back to the caller. Call sites still pattern-match
                        // `ResponsePayload::Error` for defence in depth but
                        // the request_id is lost at that layer; centralising
                        // here means every async code path gets correlation
                        // for free (mirrors the sync `Client::send_request`
                        // behaviour).
                        let result = match response.payload {
                            ResponsePayload::Error(e) => {
                                Err(ClientError::server_with_request(e.code, e.message, id))
                            }
                            payload => Ok(Response {
                                request_id: response.request_id,
                                payload,
                            }),
                        };
                        let _ = responder.send(result);
                    }
                }
                Message::Push(push) => {
                    let sub_id = match &push.payload {
                        kimberlite_wire::PushPayload::SubscriptionEvents {
                            subscription_id,
                            ..
                        }
                        | kimberlite_wire::PushPayload::SubscriptionClosed {
                            subscription_id,
                            ..
                        } => *subscription_id,
                    };
                    let routes = push_routes.lock().await;
                    if let Some(tx) = routes.get(&sub_id) {
                        let _ = tx.send(push);
                    }
                }
                Message::Request(_) => {
                    // Server should never send a Request to a client.
                    fail_all_pending(
                        &pending,
                        ClientError::server(
                            ErrorCode::InvalidRequest,
                            "server sent a Request frame",
                        ),
                    )
                    .await;
                    return;
                }
            }
        }
        // Need more bytes.
        let n = match read_half.read(&mut chunk).await {
            Ok(0) => {
                fail_all_pending(&pending, ClientError::NotConnected).await;
                return;
            }
            Ok(n) => n,
            Err(e) => {
                fail_all_pending(&pending, ClientError::Connection(e)).await;
                return;
            }
        };
        buf.extend_from_slice(&chunk[..n]);
    }
}

async fn fail_all_pending(pending: &PendingResponders, err: ClientError) {
    // ClientError doesn't impl Clone (the io::Error variant has no
    // Clone). Synthesise a Display-equivalent NotConnected for each
    // pending responder; the original error is logged here so the
    // root cause survives.
    tracing::warn!(error = %err, "async client failing all pending requests");
    let mut guard = pending.lock().await;
    for (_, responder) in guard.drain() {
        let _ = responder.send(Err(ClientError::NotConnected));
    }
}
