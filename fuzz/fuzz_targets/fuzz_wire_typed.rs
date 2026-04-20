#![no_main]
//! Structure-aware fuzzing of the wire request path.
//!
//! `fuzz_wire_deserialize` feeds raw bytes through `Frame::decode`; ~99% of
//! mutations are rejected at framing and never reach request handlers. This
//! target mutates at the typed level: `Arbitrary` generates a structurally
//! valid `Request` and exercises its serialisation round-trip + handler-free
//! invariant checks. Coverage lands inside the postcard codec and wire-type
//! validation paths that the byte-level fuzzer rarely hits.

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;

// Typed request generator. Kept in the fuzz crate (rather than propagating
// an `arbitrary` feature through `kimberlite-types` / `kimberlite-wire`) so
// that the production crates' dependency footprint stays minimal.
#[derive(Debug, Arbitrary)]
struct FuzzableRequest {
    request_id: u64,
    tenant_id: u64,
    payload: FuzzablePayload,
}

#[derive(Debug, Arbitrary)]
enum FuzzablePayload {
    Handshake {
        client_version: u16,
        auth_token: Option<String>,
    },
    CreateStream {
        name: String,
        // Data classification and placement are left as defaults to keep the
        // Arbitrary surface small; the parser/handler paths don't panic on
        // defaults, so this keeps focus on name / string-handling bugs.
    },
    AppendEvents {
        stream_id: u64,
        events: Vec<Vec<u8>>,
        expected_offset: u64,
    },
    Query {
        sql: String,
        params: Vec<FuzzableQueryParam>,
    },
    QueryAt {
        sql: String,
        params: Vec<FuzzableQueryParam>,
        position: u64,
    },
    ReadEvents {
        stream_id: u64,
        from_offset: u64,
        max_bytes: u64,
    },
    Subscribe {
        stream_id: u64,
        from_offset: u64,
        initial_credits: u32,
        consumer_group: Option<String>,
    },
    Sync,
}

#[derive(Debug, Arbitrary)]
enum FuzzableQueryParam {
    Null,
    BigInt(i64),
    Text(String),
    Boolean(bool),
    Timestamp(i64),
}

impl From<FuzzableQueryParam> for kmb_wire::QueryParam {
    fn from(p: FuzzableQueryParam) -> Self {
        match p {
            FuzzableQueryParam::Null => Self::Null,
            FuzzableQueryParam::BigInt(v) => Self::BigInt(v),
            FuzzableQueryParam::Text(s) => Self::Text(s),
            FuzzableQueryParam::Boolean(b) => Self::Boolean(b),
            FuzzableQueryParam::Timestamp(t) => Self::Timestamp(t),
        }
    }
}

impl From<FuzzableRequest> for kmb_wire::Request {
    fn from(f: FuzzableRequest) -> Self {
        use kimberlite_types::{DataClass, Placement, StreamId, TenantId};
        use kmb_wire::{
            AppendEventsRequest, CreateStreamRequest, HandshakeRequest, QueryAtRequest,
            QueryRequest, ReadEventsRequest, RequestId, RequestPayload, SubscribeRequest,
            SyncRequest,
        };

        let payload = match f.payload {
            FuzzablePayload::Handshake {
                client_version,
                auth_token,
            } => RequestPayload::Handshake(HandshakeRequest {
                client_version,
                auth_token,
            }),
            FuzzablePayload::CreateStream { name } => {
                RequestPayload::CreateStream(CreateStreamRequest {
                    name,
                    data_class: DataClass::Public,
                    placement: Placement::Global,
                })
            }
            FuzzablePayload::AppendEvents {
                stream_id,
                events,
                expected_offset,
            } => RequestPayload::AppendEvents(AppendEventsRequest {
                stream_id: StreamId::new(stream_id),
                events,
                expected_offset: kimberlite_types::Offset::from(expected_offset),
            }),
            FuzzablePayload::Query { sql, params } => RequestPayload::Query(QueryRequest {
                sql,
                params: params.into_iter().map(Into::into).collect(),
                break_glass_reason: None,
            }),
            FuzzablePayload::QueryAt {
                sql,
                params,
                position,
            } => RequestPayload::QueryAt(QueryAtRequest {
                sql,
                params: params.into_iter().map(Into::into).collect(),
                position: kimberlite_types::Offset::from(position),
                break_glass_reason: None,
            }),
            FuzzablePayload::ReadEvents {
                stream_id,
                from_offset,
                max_bytes,
            } => RequestPayload::ReadEvents(ReadEventsRequest {
                stream_id: StreamId::new(stream_id),
                from_offset: kimberlite_types::Offset::from(from_offset),
                max_bytes,
            }),
            FuzzablePayload::Subscribe {
                stream_id,
                from_offset,
                initial_credits,
                consumer_group,
            } => RequestPayload::Subscribe(SubscribeRequest {
                stream_id: StreamId::new(stream_id),
                from_offset: kimberlite_types::Offset::from(from_offset),
                initial_credits,
                consumer_group,
            }),
            FuzzablePayload::Sync => RequestPayload::Sync(SyncRequest {}),
        };

        Self {
            id: RequestId::new(f.request_id),
            tenant_id: TenantId::new(f.tenant_id),
            audit: None,
            payload,
        }
    }
}

fuzz_target!(|data: &[u8]| {
    // Drive the Arbitrary generator with the input bytes.
    let mut u = Unstructured::new(data);
    let Ok(fuzzable) = FuzzableRequest::arbitrary(&mut u) else {
        return;
    };
    let request: kmb_wire::Request = fuzzable.into();

    // 1. Serialisation must never panic.
    let Ok(frame) = request.to_frame() else {
        return;
    };

    // 2. Round-trip: decode what we just encoded, must match.
    let decoded =
        kmb_wire::Request::from_frame(&frame).expect("typed request must round-trip through frame");

    // 3. Deterministic re-encoding.
    let frame2 = decoded
        .to_frame()
        .expect("re-encoding a decoded request must succeed");
    assert_eq!(
        frame.payload.as_ref(),
        frame2.payload.as_ref(),
        "postcard encoding must be deterministic"
    );
});
