//! RPC client for `Kimberlite`.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use bytes::BytesMut;
use kimberlite_types::{DataClass, Offset, Placement, StreamId, TenantId};
use kimberlite_wire::{
    AppendEventsRequest, CreateStreamRequest, ErrorCode, Frame, HandshakeRequest, Message,
    PROTOCOL_VERSION, Push, QueryAtRequest, QueryParam, QueryRequest, QueryResponse,
    ReadEventsRequest, ReadEventsResponse, Request, RequestId, RequestPayload, Response,
    ResponsePayload, SubscribeCreditRequest, SubscribeRequest, SubscribeResponse, SyncRequest,
    UnsubscribeRequest,
};

use crate::error::{ClientError, ClientResult};

/// Configuration for the client.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Read timeout.
    pub read_timeout: Option<Duration>,
    /// Write timeout.
    pub write_timeout: Option<Duration>,
    /// Buffer size for reads.
    pub buffer_size: usize,
    /// Authentication token.
    pub auth_token: Option<String>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            read_timeout: Some(Duration::from_secs(30)),
            write_timeout: Some(Duration::from_secs(30)),
            buffer_size: 64 * 1024,
            auth_token: None,
        }
    }
}

/// RPC client for `Kimberlite`.
///
/// This client uses synchronous I/O to communicate with a `Kimberlite` server
/// using the binary wire protocol.
///
/// # Example
///
/// ```ignore
/// use kimberlite_client::{Client, ClientConfig};
/// use kimberlite_types::{DataClass, Offset, TenantId};
///
/// let mut client = Client::connect("127.0.0.1:5432", TenantId::new(1), ClientConfig::default())?;
///
/// // Create a stream
/// let stream_id = client.create_stream("events", DataClass::Public)?;
///
/// // Append events
/// let offset = client.append(stream_id, vec![b"event1".to_vec(), b"event2".to_vec()], Offset::ZERO)?;
/// ```
pub struct Client {
    stream: TcpStream,
    tenant_id: TenantId,
    next_request_id: u64,
    last_request_id: Option<u64>,
    read_buf: BytesMut,
    config: ClientConfig,
    /// Push frames buffered out-of-band while waiting for a response.
    ///
    /// Protocol v2 interleaves server-initiated `Push` frames on the same
    /// socket as normal responses. If a push arrives during
    /// [`Client::send_request`] we stash it here so subscriptions can drain
    /// it later via [`Client::next_push`].
    push_buffer: VecDeque<Push>,
}

impl Client {
    /// Connects to a `Kimberlite` server.
    pub fn connect(
        addr: impl ToSocketAddrs,
        tenant_id: TenantId,
        config: ClientConfig,
    ) -> ClientResult<Self> {
        let stream = TcpStream::connect(addr)?;
        stream.set_read_timeout(config.read_timeout)?;
        stream.set_write_timeout(config.write_timeout)?;

        let mut client = Self {
            stream,
            tenant_id,
            next_request_id: 1,
            last_request_id: None,
            read_buf: BytesMut::with_capacity(config.buffer_size),
            config,
            push_buffer: VecDeque::new(),
        };

        // Perform handshake
        client.handshake()?;

        Ok(client)
    }

    /// Performs the handshake with the server.
    fn handshake(&mut self) -> ClientResult<()> {
        let response = self.send_request(RequestPayload::Handshake(HandshakeRequest {
            client_version: PROTOCOL_VERSION,
            auth_token: self.config.auth_token.clone(),
        }))?;

        match response.payload {
            ResponsePayload::Handshake(h) => {
                if h.server_version != PROTOCOL_VERSION {
                    return Err(ClientError::HandshakeFailed(format!(
                        "protocol version mismatch: client {}, server {}",
                        PROTOCOL_VERSION, h.server_version
                    )));
                }
                Ok(())
            }
            ResponsePayload::Error(e) => Err(ClientError::server(e.code, e.message)),
            _ => Err(ClientError::UnexpectedResponse {
                expected: "Handshake".to_string(),
                actual: format!("{:?}", response.payload),
            }),
        }
    }

    /// Creates a new stream.
    pub fn create_stream(&mut self, name: &str, data_class: DataClass) -> ClientResult<StreamId> {
        self.create_stream_with_placement(name, data_class, Placement::Global)
    }

    /// Creates a new stream with a specific placement policy.
    pub fn create_stream_with_placement(
        &mut self,
        name: &str,
        data_class: DataClass,
        placement: Placement,
    ) -> ClientResult<StreamId> {
        let response = self.send_request(RequestPayload::CreateStream(CreateStreamRequest {
            name: name.to_string(),
            data_class,
            placement,
        }))?;

        match response.payload {
            ResponsePayload::CreateStream(r) => Ok(r.stream_id),
            ResponsePayload::Error(e) => Err(ClientError::server(e.code, e.message)),
            _ => Err(ClientError::UnexpectedResponse {
                expected: "CreateStream".to_string(),
                actual: format!("{:?}", response.payload),
            }),
        }
    }

    /// Appends events to a stream with optimistic concurrency control.
    ///
    /// The caller must provide the expected current offset of the stream.
    /// If another writer has appended since the caller last read the offset,
    /// the server returns `ErrorCode::OffsetMismatch`.
    ///
    /// Returns the offset of the first appended event.
    pub fn append(
        &mut self,
        stream_id: StreamId,
        events: Vec<Vec<u8>>,
        expected_offset: Offset,
    ) -> ClientResult<Offset> {
        let response = self.send_request(RequestPayload::AppendEvents(AppendEventsRequest {
            stream_id,
            events,
            expected_offset,
        }))?;

        match response.payload {
            ResponsePayload::AppendEvents(r) => Ok(r.first_offset),
            ResponsePayload::Error(e) => Err(ClientError::server(e.code, e.message)),
            _ => Err(ClientError::UnexpectedResponse {
                expected: "AppendEvents".to_string(),
                actual: format!("{:?}", response.payload),
            }),
        }
    }

    /// Executes a SQL query.
    pub fn query(&mut self, sql: &str, params: &[QueryParam]) -> ClientResult<QueryResponse> {
        let response = self.send_request(RequestPayload::Query(QueryRequest {
            sql: sql.to_string(),
            params: params.to_vec(),
        }))?;

        match response.payload {
            ResponsePayload::Query(r) => Ok(r),
            ResponsePayload::Error(e) => Err(ClientError::server(e.code, e.message)),
            _ => Err(ClientError::UnexpectedResponse {
                expected: "Query".to_string(),
                actual: format!("{:?}", response.payload),
            }),
        }
    }

    /// Executes a SQL query at a specific position.
    pub fn query_at(
        &mut self,
        sql: &str,
        params: &[QueryParam],
        position: Offset,
    ) -> ClientResult<QueryResponse> {
        let response = self.send_request(RequestPayload::QueryAt(QueryAtRequest {
            sql: sql.to_string(),
            params: params.to_vec(),
            position,
        }))?;

        match response.payload {
            ResponsePayload::QueryAt(r) => Ok(r),
            ResponsePayload::Error(e) => Err(ClientError::server(e.code, e.message)),
            _ => Err(ClientError::UnexpectedResponse {
                expected: "QueryAt".to_string(),
                actual: format!("{:?}", response.payload),
            }),
        }
    }

    /// Reads events from a stream.
    pub fn read_events(
        &mut self,
        stream_id: StreamId,
        from_offset: Offset,
        max_bytes: u64,
    ) -> ClientResult<ReadEventsResponse> {
        let response = self.send_request(RequestPayload::ReadEvents(ReadEventsRequest {
            stream_id,
            from_offset,
            max_bytes,
        }))?;

        match response.payload {
            ResponsePayload::ReadEvents(r) => Ok(r),
            ResponsePayload::Error(e) => Err(ClientError::server(e.code, e.message)),
            _ => Err(ClientError::UnexpectedResponse {
                expected: "ReadEvents".to_string(),
                actual: format!("{:?}", response.payload),
            }),
        }
    }

    /// Syncs all data to disk.
    pub fn sync(&mut self) -> ClientResult<()> {
        let response = self.send_request(RequestPayload::Sync(SyncRequest {}))?;

        match response.payload {
            ResponsePayload::Sync(r) => {
                if r.success {
                    Ok(())
                } else {
                    Err(ClientError::server(ErrorCode::InternalError, "sync failed"))
                }
            }
            ResponsePayload::Error(e) => Err(ClientError::server(e.code, e.message)),
            _ => Err(ClientError::UnexpectedResponse {
                expected: "Sync".to_string(),
                actual: format!("{:?}", response.payload),
            }),
        }
    }

    /// Returns the tenant ID for this client.
    pub fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }

    /// Returns the wire request ID of the most recently sent request.
    ///
    /// Useful for correlating client operations with server-side tracing
    /// output. Returns `None` before any request has been sent.
    pub fn last_request_id(&self) -> Option<u64> {
        self.last_request_id
    }

    /// Executes a SQL statement that modifies state (INSERT, UPDATE, DELETE,
    /// CREATE TABLE, ALTER TABLE, DROP TABLE, ...).
    ///
    /// Returns `(rows_affected, log_offset)`. For DDL statements the
    /// rows-affected count is typically 0.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Server`] if the server rejects the statement
    /// (bad SQL, insufficient privileges, constraint violation, ...).
    ///
    /// # Example
    ///
    /// ```ignore
    /// let (rows, _offset) = client.execute(
    ///     "INSERT INTO users (id, name) VALUES ($1, $2)",
    ///     &[QueryParam::BigInt(1), QueryParam::Text("alice".into())],
    /// )?;
    /// assert_eq!(rows, 1);
    /// ```
    pub fn execute(&mut self, sql: &str, params: &[QueryParam]) -> ClientResult<(u64, u64)> {
        let response = self.query(sql, params)?;
        extract_execute_result(&response).ok_or_else(|| {
            ClientError::server(
                ErrorCode::InternalError,
                format!(
                    "execute() called on non-DML statement; got columns {:?}",
                    response.columns
                ),
            )
        })
    }

    /// Subscribes to real-time events on a stream.
    ///
    /// Returns the server-assigned subscription ID, the starting offset, and
    /// the initial credit balance. Use [`Client::next_push`] to drain push
    /// frames (or the higher-level [`Subscription`](crate::Subscription)
    /// helper for iterator ergonomics).
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Server`] if the stream doesn't exist or the
    /// caller's role lacks read permission.
    pub fn subscribe(
        &mut self,
        stream_id: StreamId,
        from_offset: Offset,
        initial_credits: u32,
        consumer_group: Option<String>,
    ) -> ClientResult<SubscribeResponse> {
        let response = self.send_request(RequestPayload::Subscribe(SubscribeRequest {
            stream_id,
            from_offset,
            initial_credits,
            consumer_group,
        }))?;

        match response.payload {
            ResponsePayload::Subscribe(r) => Ok(r),
            ResponsePayload::Error(e) => Err(ClientError::server(e.code, e.message)),
            _ => Err(ClientError::UnexpectedResponse {
                expected: "Subscribe".to_string(),
                actual: format!("{:?}", response.payload),
            }),
        }
    }

    /// Grants additional flow-control credits to an existing subscription.
    ///
    /// Returns the server's new credit balance.
    pub fn grant_credits(
        &mut self,
        subscription_id: u64,
        additional_credits: u32,
    ) -> ClientResult<u32> {
        let response =
            self.send_request(RequestPayload::SubscribeCredit(SubscribeCreditRequest {
                subscription_id,
                additional_credits,
            }))?;
        match response.payload {
            ResponsePayload::SubscriptionAck(ack) => Ok(ack.credits_remaining),
            ResponsePayload::Error(e) => Err(ClientError::server(e.code, e.message)),
            _ => Err(ClientError::UnexpectedResponse {
                expected: "SubscriptionAck".to_string(),
                actual: format!("{:?}", response.payload),
            }),
        }
    }

    /// Cancels a subscription. The server emits a final `SubscriptionClosed`
    /// push before forgetting the subscription.
    pub fn unsubscribe(&mut self, subscription_id: u64) -> ClientResult<()> {
        let response = self.send_request(RequestPayload::Unsubscribe(UnsubscribeRequest {
            subscription_id,
        }))?;
        match response.payload {
            ResponsePayload::SubscriptionAck(_) => Ok(()),
            ResponsePayload::Error(e) => Err(ClientError::server(e.code, e.message)),
            _ => Err(ClientError::UnexpectedResponse {
                expected: "SubscriptionAck".to_string(),
                actual: format!("{:?}", response.payload),
            }),
        }
    }

    /// Reads the next server-pushed frame. Blocks until a push arrives,
    /// EOF, or the read timeout expires.
    ///
    /// Push frames that arrive during a normal request/response exchange
    /// are buffered — this method drains that buffer first before reading
    /// from the socket.
    pub fn next_push(&mut self) -> ClientResult<Option<Push>> {
        if let Some(push) = self.push_buffer.pop_front() {
            return Ok(Some(push));
        }

        loop {
            match self.read_message()? {
                Message::Push(p) => return Ok(Some(p)),
                Message::Response(r) => {
                    tracing::warn!(
                        request_id = r.request_id.0,
                        "next_push: discarding out-of-band Response frame"
                    );
                }
                Message::Request(_) => {
                    return Err(ClientError::server(
                        ErrorCode::InvalidRequest,
                        "server sent a Request frame",
                    ));
                }
            }
        }
    }

    /// Sends a request and waits for the response.
    ///
    /// Protocol v2 uses multiplexed framing: the server may interleave
    /// server-initiated `Push` frames between requests and responses. Any
    /// pushes received while waiting for the response are buffered in
    /// `push_buffer` and surfaced via [`Client::next_push`].
    #[tracing::instrument(
        skip_all,
        fields(tenant_id = u64::from(self.tenant_id), request_id)
    )]
    fn send_request(&mut self, payload: RequestPayload) -> ClientResult<Response> {
        let request_id = RequestId::new(self.next_request_id);
        self.next_request_id += 1;
        self.last_request_id = Some(request_id.0);
        tracing::Span::current().record("request_id", request_id.0);

        let request = Request::new(request_id, self.tenant_id, payload);

        // Encode and send the request (wire v2: wrapped in Message::Request).
        let frame = Message::Request(request).to_frame()?;
        let mut write_buf = BytesMut::new();
        frame.encode(&mut write_buf);
        self.stream.write_all(&write_buf)?;
        self.stream.flush()?;

        // Read until we see the response for this request_id, buffering any
        // push frames that arrive in the meantime.
        loop {
            match self.read_message()? {
                Message::Response(response) => {
                    if response.request_id.0 != request_id.0 {
                        return Err(ClientError::ResponseMismatch {
                            expected: request_id.0,
                            received: response.request_id.0,
                        });
                    }
                    return Ok(response);
                }
                Message::Push(push) => self.push_buffer.push_back(push),
                Message::Request(_) => {
                    return Err(ClientError::server(
                        ErrorCode::InvalidRequest,
                        "server sent a Request frame",
                    ));
                }
            }
        }
    }

    /// Reads a single [`Message`] frame from the socket, pulling more bytes
    /// from the stream as needed.
    fn read_message(&mut self) -> ClientResult<Message> {
        loop {
            if let Some(frame) = Frame::decode(&mut self.read_buf)? {
                return Ok(Message::from_frame(&frame)?);
            }

            let mut temp_buf = [0u8; 4096];
            let n = self.stream.read(&mut temp_buf)?;
            if n == 0 {
                return Err(ClientError::Connection(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "server closed connection",
                )));
            }
            self.read_buf.extend_from_slice(&temp_buf[..n]);

            if self.read_buf.len() > self.config.buffer_size * 2 {
                return Err(ClientError::Connection(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "response too large",
                )));
            }
        }
    }
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("tenant_id", &self.tenant_id)
            .field("next_request_id", &self.next_request_id)
            .field("last_request_id", &self.last_request_id)
            .finish_non_exhaustive()
    }
}

/// Extracts `(rows_affected, log_offset)` from a server response to a DML
/// statement. The server returns these as two BigInt columns named
/// `rows_affected` and `log_offset` — see `kimberlite-server` handler.
fn extract_execute_result(response: &QueryResponse) -> Option<(u64, u64)> {
    use kimberlite_wire::QueryValue;
    if response.columns.len() != 2 || response.rows.len() != 1 {
        return None;
    }
    if response.columns[0] != "rows_affected" || response.columns[1] != "log_offset" {
        return None;
    }
    let row = &response.rows[0];
    match (&row[0], &row[1]) {
        (QueryValue::BigInt(rows), QueryValue::BigInt(offset)) => {
            // Clamp to non-negative since wire uses i64 but these counters are unsigned.
            let rows = u64::try_from(*rows).unwrap_or(0);
            let offset = u64::try_from(*offset).unwrap_or(0);
            Some((rows, offset))
        }
        _ => None,
    }
}

#[cfg(test)]
mod client_tests {
    use super::*;
    use kimberlite_wire::QueryValue;

    #[test]
    fn extract_execute_result_matches_server_shape() {
        let response = QueryResponse {
            columns: vec!["rows_affected".to_string(), "log_offset".to_string()],
            rows: vec![vec![QueryValue::BigInt(3), QueryValue::BigInt(1024)]],
        };
        assert_eq!(extract_execute_result(&response), Some((3, 1024)));
    }

    #[test]
    fn extract_execute_result_rejects_select_shape() {
        let response = QueryResponse {
            columns: vec!["id".to_string(), "name".to_string()],
            rows: vec![vec![QueryValue::BigInt(1), QueryValue::Text("alice".into())]],
        };
        assert_eq!(extract_execute_result(&response), None);
    }

    #[test]
    fn extract_execute_result_rejects_empty_response() {
        let response = QueryResponse {
            columns: vec!["rows_affected".to_string(), "log_offset".to_string()],
            rows: vec![],
        };
        assert_eq!(extract_execute_result(&response), None);
    }
}
