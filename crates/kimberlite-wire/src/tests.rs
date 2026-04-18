//! Integration tests for the wire protocol.

use bytes::BytesMut;
use kimberlite_types::{DataClass, Offset, Placement, StreamId, TenantId};

use crate::frame::{FRAME_HEADER_SIZE, Frame, PROTOCOL_VERSION};
use crate::message::{
    AppendEventsRequest, CreateStreamRequest, ErrorCode, Message, Push, PushPayload, QueryParam,
    QueryRequest, ReadEventsRequest, Request, RequestId, RequestPayload, Response, ResponsePayload,
    SubscribeCreditRequest, SubscriptionAckResponse, SubscriptionCloseReason, UnsubscribeRequest,
};

#[test]
fn test_full_request_response_cycle() {
    // Create a request
    let request = Request::new(
        RequestId::new(1),
        TenantId::new(42),
        RequestPayload::CreateStream(CreateStreamRequest {
            name: "test-stream".to_string(),
            data_class: DataClass::PHI,
            placement: Placement::Global,
        }),
    );

    // Encode to frame
    let frame = request.to_frame().unwrap();

    // Encode to wire format
    let wire_bytes = frame.encode_to_bytes();
    assert!(wire_bytes.len() > FRAME_HEADER_SIZE);

    // Decode from wire format
    let mut buf = BytesMut::from(&wire_bytes[..]);
    let decoded_frame = Frame::decode(&mut buf).unwrap().unwrap();

    // Decode request from frame
    let decoded_request = Request::from_frame(&decoded_frame).unwrap();

    // Verify
    assert_eq!(decoded_request.id, request.id);
    assert_eq!(u64::from(decoded_request.tenant_id), 42);

    if let RequestPayload::CreateStream(cs) = decoded_request.payload {
        assert_eq!(cs.name, "test-stream");
        assert_eq!(cs.data_class, DataClass::PHI);
    } else {
        panic!("expected CreateStream payload");
    }
}

#[test]
fn test_append_events_request() {
    let request = Request::new(
        RequestId::new(100),
        TenantId::new(1),
        RequestPayload::AppendEvents(AppendEventsRequest {
            stream_id: StreamId::new(1000),
            events: vec![b"event1".to_vec(), b"event2".to_vec(), b"event3".to_vec()],
            expected_offset: Offset::ZERO,
        }),
    );

    let frame = request.to_frame().unwrap();
    let decoded = Request::from_frame(&frame).unwrap();

    if let RequestPayload::AppendEvents(ae) = decoded.payload {
        assert_eq!(u64::from(ae.stream_id), 1000);
        assert_eq!(ae.events.len(), 3);
        assert_eq!(ae.events[0], b"event1");
    } else {
        panic!("expected AppendEvents payload");
    }
}

#[test]
fn test_query_with_params() {
    let request = Request::new(
        RequestId::new(200),
        TenantId::new(1),
        RequestPayload::Query(QueryRequest {
            sql: "SELECT * FROM users WHERE id = $1 AND active = $2".to_string(),
            params: vec![QueryParam::BigInt(42), QueryParam::Boolean(true)],
        }),
    );

    let frame = request.to_frame().unwrap();
    let decoded = Request::from_frame(&frame).unwrap();

    if let RequestPayload::Query(q) = decoded.payload {
        assert_eq!(q.sql, "SELECT * FROM users WHERE id = $1 AND active = $2");
        assert_eq!(q.params.len(), 2);
    } else {
        panic!("expected Query payload");
    }
}

#[test]
fn test_read_events_request() {
    let request = Request::new(
        RequestId::new(300),
        TenantId::new(1),
        RequestPayload::ReadEvents(ReadEventsRequest {
            stream_id: StreamId::new(500),
            from_offset: Offset::new(100),
            max_bytes: 1024 * 1024,
        }),
    );

    let frame = request.to_frame().unwrap();
    let decoded = Request::from_frame(&frame).unwrap();

    if let RequestPayload::ReadEvents(re) = decoded.payload {
        assert_eq!(u64::from(re.stream_id), 500);
        assert_eq!(re.from_offset.as_u64(), 100);
        assert_eq!(re.max_bytes, 1024 * 1024);
    } else {
        panic!("expected ReadEvents payload");
    }
}

#[test]
fn test_error_codes() {
    // Test all error codes can be serialized/deserialized
    let error_codes = [
        ErrorCode::Unknown,
        ErrorCode::InternalError,
        ErrorCode::InvalidRequest,
        ErrorCode::AuthenticationFailed,
        ErrorCode::TenantNotFound,
        ErrorCode::StreamNotFound,
        ErrorCode::TableNotFound,
        ErrorCode::QueryParseError,
        ErrorCode::QueryExecutionError,
        ErrorCode::PositionAhead,
        ErrorCode::StreamAlreadyExists,
        ErrorCode::InvalidOffset,
        ErrorCode::StorageError,
        ErrorCode::ProjectionLag,
        ErrorCode::RateLimited,
    ];

    for code in error_codes {
        let response = Response::error(RequestId::new(1), code, format!("test error: {code:?}"));

        let frame = response.to_frame().unwrap();
        let decoded = Response::from_frame(&frame).unwrap();

        if let ResponsePayload::Error(err) = decoded.payload {
            assert_eq!(err.code, code);
        } else {
            panic!("expected Error payload");
        }
    }
}

#[test]
fn test_streaming_decode() {
    // Simulate receiving bytes in chunks
    let request = Request::new(
        RequestId::new(1),
        TenantId::new(1),
        RequestPayload::CreateStream(CreateStreamRequest {
            name: "test".to_string(),
            data_class: DataClass::Public,
            placement: Placement::Global,
        }),
    );

    let wire_bytes = request.to_frame().unwrap().encode_to_bytes();
    let mut buf = BytesMut::new();

    // Feed bytes one at a time
    for &byte in &wire_bytes {
        buf.extend_from_slice(&[byte]);
        let result = Frame::decode(&mut buf);

        // Should only succeed on the last byte
        if buf.is_empty() {
            // Frame was decoded and buffer consumed
            assert!(result.is_ok());
            assert!(result.unwrap().is_some());
        } else if result.is_ok() && result.as_ref().unwrap().is_some() {
            // Frame decoded before end - this is also valid
            break;
        }
    }
}

#[test]
fn test_large_payload() {
    // Test with a reasonably large payload
    let large_event = vec![0u8; 100_000]; // 100KB event

    let request = Request::new(
        RequestId::new(1),
        TenantId::new(1),
        RequestPayload::AppendEvents(AppendEventsRequest {
            stream_id: StreamId::new(1),
            events: vec![large_event.clone()],
            expected_offset: Offset::ZERO,
        }),
    );

    let frame = request.to_frame().unwrap();
    assert!(frame.payload.len() > 100_000);

    let decoded = Request::from_frame(&frame).unwrap();

    if let RequestPayload::AppendEvents(ae) = decoded.payload {
        assert_eq!(ae.events[0].len(), 100_000);
    } else {
        panic!("expected AppendEvents payload");
    }
}

// ============================================================================
// Protocol v2 — push frames and Message enum
// ============================================================================

#[test]
fn protocol_version_is_two() {
    // Phase 3 bump — v1 clients must fail the handshake cleanly.
    assert_eq!(PROTOCOL_VERSION, 2);
}

#[test]
fn push_subscription_events_roundtrip() {
    let push = Push::new(PushPayload::SubscriptionEvents {
        subscription_id: 42,
        start_offset: Offset::new(100),
        events: vec![b"event1".to_vec(), b"event2".to_vec()],
        credits_remaining: 8,
    });
    let msg = Message::Push(push);

    let frame = msg.to_frame().unwrap();
    let decoded = Message::from_frame(&frame).unwrap();

    match decoded {
        Message::Push(p) => match p.payload {
            PushPayload::SubscriptionEvents {
                subscription_id,
                start_offset,
                events,
                credits_remaining,
            } => {
                assert_eq!(subscription_id, 42);
                assert_eq!(start_offset.as_u64(), 100);
                assert_eq!(events.len(), 2);
                assert_eq!(events[0], b"event1");
                assert_eq!(credits_remaining, 8);
            }
            other => panic!("expected SubscriptionEvents, got {other:?}"),
        },
        other => panic!("expected Message::Push, got {other:?}"),
    }
}

#[test]
fn push_subscription_closed_roundtrip() {
    for reason in [
        SubscriptionCloseReason::ClientCancelled,
        SubscriptionCloseReason::ServerShutdown,
        SubscriptionCloseReason::StreamDeleted,
        SubscriptionCloseReason::BackpressureTimeout,
        SubscriptionCloseReason::ProtocolError,
    ] {
        let msg = Message::Push(Push::new(PushPayload::SubscriptionClosed {
            subscription_id: 7,
            reason,
        }));
        let frame = msg.to_frame().unwrap();
        let decoded = Message::from_frame(&frame).unwrap();
        match decoded {
            Message::Push(p) => match p.payload {
                PushPayload::SubscriptionClosed {
                    subscription_id,
                    reason: r,
                } => {
                    assert_eq!(subscription_id, 7);
                    assert_eq!(r, reason);
                }
                other => panic!("expected SubscriptionClosed, got {other:?}"),
            },
            other => panic!("expected Message::Push, got {other:?}"),
        }
    }
}

#[test]
fn subscribe_credit_request_roundtrip() {
    let request = Request::new(
        RequestId::new(99),
        TenantId::new(1),
        RequestPayload::SubscribeCredit(SubscribeCreditRequest {
            subscription_id: 12345,
            additional_credits: 64,
        }),
    );
    let msg = Message::Request(request);
    let frame = msg.to_frame().unwrap();
    let decoded = Message::from_frame(&frame).unwrap();

    match decoded {
        Message::Request(r) => match r.payload {
            RequestPayload::SubscribeCredit(req) => {
                assert_eq!(req.subscription_id, 12345);
                assert_eq!(req.additional_credits, 64);
            }
            other => panic!("expected SubscribeCredit, got {other:?}"),
        },
        other => panic!("expected Message::Request, got {other:?}"),
    }
}

#[test]
fn unsubscribe_request_roundtrip() {
    let request = Request::new(
        RequestId::new(100),
        TenantId::new(1),
        RequestPayload::Unsubscribe(UnsubscribeRequest { subscription_id: 9 }),
    );
    let frame = Message::Request(request).to_frame().unwrap();
    let decoded = Message::from_frame(&frame).unwrap();
    match decoded {
        Message::Request(r) => match r.payload {
            RequestPayload::Unsubscribe(u) => assert_eq!(u.subscription_id, 9),
            other => panic!("expected Unsubscribe, got {other:?}"),
        },
        other => panic!("expected Message::Request, got {other:?}"),
    }
}

#[test]
fn subscription_ack_response_roundtrip() {
    let response = Response::new(
        RequestId::new(50),
        ResponsePayload::SubscriptionAck(SubscriptionAckResponse {
            subscription_id: 1,
            credits_remaining: 16,
        }),
    );
    let frame = Message::Response(response).to_frame().unwrap();
    let decoded = Message::from_frame(&frame).unwrap();
    match decoded {
        Message::Response(r) => match r.payload {
            ResponsePayload::SubscriptionAck(ack) => {
                assert_eq!(ack.subscription_id, 1);
                assert_eq!(ack.credits_remaining, 16);
            }
            other => panic!("expected SubscriptionAck, got {other:?}"),
        },
        other => panic!("expected Message::Response, got {other:?}"),
    }
}

#[test]
fn new_error_codes_round_trip() {
    // These three codes were added in protocol v2 for subscription flow.
    let codes = [
        ErrorCode::SubscriptionNotFound,
        ErrorCode::SubscriptionClosed,
        ErrorCode::SubscriptionBackpressure,
    ];
    for code in codes {
        let response = Response::error(RequestId::new(1), code, format!("{code:?}"));
        let frame = Message::Response(response).to_frame().unwrap();
        let decoded = Message::from_frame(&frame).unwrap();
        match decoded {
            Message::Response(r) => match r.payload {
                ResponsePayload::Error(e) => assert_eq!(e.code, code),
                other => panic!("expected Error, got {other:?}"),
            },
            other => panic!("expected Message::Response, got {other:?}"),
        }
    }
}

#[test]
fn legacy_request_to_frame_still_works_via_message() {
    // Existing code paths that call `Request::to_frame` directly continue
    // working as long as the decoder treats bare Request bytes as
    // `Message::Request` once v2 introduces the outer enum. Here we verify
    // the v2 shape specifically: a `Request::to_frame` output is NOT a valid
    // `Message::from_frame` input — clients must go through Message from now
    // on. This test locks in that contract so a regression is loud.
    let req = Request::new(
        RequestId::new(1),
        TenantId::new(1),
        RequestPayload::Sync(crate::message::SyncRequest {}),
    );
    let bare_frame = req.to_frame().unwrap();
    // Decoding the bare Request bytes as a Message should fail — callers
    // must wrap in Message::Request before framing under v2.
    assert!(Message::from_frame(&bare_frame).is_err());
}
