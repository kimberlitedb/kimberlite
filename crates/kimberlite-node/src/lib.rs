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

use kimberlite_client::{Client, ClientConfig, ClientError};
use kimberlite_types::{DataClass, Offset, Placement, Region, StreamId, TenantId};
use kimberlite_wire::{QueryParam as WireQueryParam, QueryValue as WireQueryValue};

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
    // Preserve error type via a coarse status code; JS side can pattern-match
    // on the rendered message for finer detail.
    let status = match &err {
        ClientError::Connection(_) | ClientError::Server { .. } => Status::GenericFailure,
        ClientError::NotConnected | ClientError::Timeout => Status::Cancelled,
        ClientError::Wire(_)
        | ClientError::ResponseMismatch { .. }
        | ClientError::UnexpectedResponse { .. }
        | ClientError::HandshakeFailed(_) => Status::InvalidArg,
    };
    Error::new(status, err.to_string())
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
