//! Integration tests for the wire protocol.

#![allow(clippy::match_wildcard_for_single_variants)]

use bytes::BytesMut;
use kimberlite_types::{DataClass, Offset, Placement, StreamId, TenantId};

use crate::error::WireError;
use crate::frame::{FRAME_HEADER_SIZE, Frame, FrameHeader, PROTOCOL_VERSION};
use crate::message::{
    AppendEventsRequest, AuditMetadata, CreateStreamRequest, ErrorCode, Message, Push, PushPayload,
    QueryParam, QueryRequest, ReadEventsRequest, Request, RequestId, RequestPayload, Response,
    ResponsePayload, SubscribeCreditRequest, SubscriptionAckResponse, SubscriptionCloseReason,
    UnsubscribeRequest,
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
            break_glass_reason: None,
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
fn protocol_version_is_four() {
    // v4 bump — `ConsentGrantRequest` and `ConsentRecord` carry an
    // optional GDPR Article 6(1) `basis`. v2 and earlier clients
    // must fail the handshake cleanly; v3 clients remain bytewise
    // wire-compatible with v4 servers for payloads that don't
    // populate `basis` — see `v3_v4_compat_*` tests below.
    assert_eq!(PROTOCOL_VERSION, 4);
}

#[test]
fn request_without_audit_roundtrips() {
    let request = Request::new(
        RequestId::new(7),
        TenantId::new(3),
        RequestPayload::Query(QueryRequest {
            sql: "SELECT 1".into(),
            params: vec![],
            break_glass_reason: None,
        }),
    );
    let frame = request.to_frame().unwrap();
    let decoded = Request::from_frame(&frame).unwrap();
    assert_eq!(decoded.id, RequestId::new(7));
    assert!(decoded.audit.is_none());
}

#[test]
fn request_with_full_audit_metadata_roundtrips() {
    let audit = AuditMetadata {
        actor: Some("dr.smith@example.com".into()),
        reason: Some("patient-chart-view".into()),
        correlation_id: Some("trace-8f3a".into()),
        idempotency_key: Some("chart-view:42:2026-04-20T12:00".into()),
    };
    let request = Request::with_audit(
        RequestId::new(8),
        TenantId::new(3),
        Some(audit.clone()),
        RequestPayload::Query(QueryRequest {
            sql: "SELECT * FROM patients WHERE id = ?".into(),
            params: vec![QueryParam::BigInt(42)],
            break_glass_reason: None,
        }),
    );
    let frame = request.to_frame().unwrap();
    let decoded = Request::from_frame(&frame).unwrap();
    assert_eq!(decoded.audit.as_ref().unwrap(), &audit);
}

#[test]
fn request_with_partial_audit_metadata_roundtrips() {
    // Actor + reason but no correlation/idempotency — the common
    // React Router loader shape.
    let audit = AuditMetadata {
        actor: Some("user-42".into()),
        reason: Some("scheduled-export".into()),
        ..Default::default()
    };
    let request = Request::with_audit(
        RequestId::new(9),
        TenantId::new(4),
        Some(audit.clone()),
        RequestPayload::Sync(crate::message::SyncRequest {}),
    );
    let frame = request.to_frame().unwrap();
    let decoded = Request::from_frame(&frame).unwrap();
    assert_eq!(
        decoded.audit.as_ref().unwrap().actor.as_deref(),
        Some("user-42")
    );
    assert!(decoded.audit.as_ref().unwrap().correlation_id.is_none());
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
    // Codes added in protocol v2 for subscription flow + Phase 4 admin ops.
    let codes = [
        ErrorCode::SubscriptionNotFound,
        ErrorCode::SubscriptionClosed,
        ErrorCode::SubscriptionBackpressure,
        ErrorCode::ApiKeyNotFound,
        ErrorCode::TenantAlreadyExists,
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

// ============================================================================
// Phase 4 — admin + schema + server info roundtrips
// ============================================================================

use crate::message::{
    ApiKeyInfo, ApiKeyListResponse, ApiKeyRegisterRequest, ApiKeyRegisterResponse,
    ApiKeyRotateRequest, ApiKeyRotateResponse, ClusterMode, ColumnInfo, DescribeTableRequest,
    DescribeTableResponse, GetServerInfoRequest, IndexInfo, ListIndexesRequest,
    ListIndexesResponse, ListTablesRequest, ListTablesResponse, ServerInfoResponse, TableInfo,
    TenantCreateRequest, TenantCreateResponse, TenantGetRequest, TenantInfo, TenantListResponse,
};

#[test]
fn list_tables_roundtrip() {
    let req = Request::new(
        RequestId::new(1),
        TenantId::new(1),
        RequestPayload::ListTables(ListTablesRequest::default()),
    );
    let frame = Message::Request(req).to_frame().unwrap();
    match Message::from_frame(&frame).unwrap() {
        Message::Request(r) => assert!(matches!(r.payload, RequestPayload::ListTables(_))),
        other => panic!("expected Request, got {other:?}"),
    }

    let resp = Response::new(
        RequestId::new(1),
        ResponsePayload::ListTables(ListTablesResponse {
            tables: vec![TableInfo {
                name: "patients".into(),
                column_count: 5,
            }],
        }),
    );
    let frame = Message::Response(resp).to_frame().unwrap();
    match Message::from_frame(&frame).unwrap() {
        Message::Response(r) => match r.payload {
            ResponsePayload::ListTables(l) => {
                assert_eq!(l.tables.len(), 1);
                assert_eq!(l.tables[0].name, "patients");
                assert_eq!(l.tables[0].column_count, 5);
            }
            other => panic!("expected ListTables, got {other:?}"),
        },
        other => panic!("expected Response, got {other:?}"),
    }
}

#[test]
fn describe_table_roundtrip() {
    let resp = Response::new(
        RequestId::new(1),
        ResponsePayload::DescribeTable(DescribeTableResponse {
            table_name: "users".into(),
            columns: vec![
                ColumnInfo {
                    name: "id".into(),
                    data_type: "BIGINT".into(),
                    nullable: false,
                    primary_key: true,
                },
                ColumnInfo {
                    name: "name".into(),
                    data_type: "TEXT".into(),
                    nullable: true,
                    primary_key: false,
                },
            ],
        }),
    );
    let frame = Message::Response(resp).to_frame().unwrap();
    match Message::from_frame(&frame).unwrap() {
        Message::Response(r) => match r.payload {
            ResponsePayload::DescribeTable(d) => {
                assert_eq!(d.table_name, "users");
                assert_eq!(d.columns.len(), 2);
                assert!(d.columns[0].primary_key);
                assert!(!d.columns[0].nullable);
                assert!(d.columns[1].nullable);
            }
            other => panic!("expected DescribeTable, got {other:?}"),
        },
        other => panic!("expected Response, got {other:?}"),
    }

    // Request side
    let req = Request::new(
        RequestId::new(1),
        TenantId::new(1),
        RequestPayload::DescribeTable(DescribeTableRequest {
            table_name: "users".into(),
        }),
    );
    let frame = Message::Request(req).to_frame().unwrap();
    match Message::from_frame(&frame).unwrap() {
        Message::Request(r) => match r.payload {
            RequestPayload::DescribeTable(d) => assert_eq!(d.table_name, "users"),
            other => panic!("expected DescribeTable, got {other:?}"),
        },
        other => panic!("expected Request, got {other:?}"),
    }
}

#[test]
fn list_indexes_roundtrip() {
    let resp = Response::new(
        RequestId::new(1),
        ResponsePayload::ListIndexes(ListIndexesResponse {
            indexes: vec![IndexInfo {
                name: "users_email_idx".into(),
                columns: vec!["email".into()],
            }],
        }),
    );
    let frame = Message::Response(resp).to_frame().unwrap();
    match Message::from_frame(&frame).unwrap() {
        Message::Response(r) => match r.payload {
            ResponsePayload::ListIndexes(l) => assert_eq!(l.indexes[0].columns[0], "email"),
            other => panic!("expected ListIndexes, got {other:?}"),
        },
        other => panic!("expected Response, got {other:?}"),
    }

    let req = Request::new(
        RequestId::new(1),
        TenantId::new(1),
        RequestPayload::ListIndexes(ListIndexesRequest {
            table_name: "users".into(),
        }),
    );
    let frame = Message::Request(req).to_frame().unwrap();
    assert!(matches!(
        Message::from_frame(&frame).unwrap(),
        Message::Request(r) if matches!(r.payload, RequestPayload::ListIndexes(_))
    ));
}

#[test]
fn tenant_crud_roundtrip() {
    // Create
    let req = Request::new(
        RequestId::new(1),
        TenantId::new(1),
        RequestPayload::TenantCreate(TenantCreateRequest {
            tenant_id: TenantId::new(42),
            name: Some("acme-corp".into()),
        }),
    );
    let frame = Message::Request(req).to_frame().unwrap();
    match Message::from_frame(&frame).unwrap() {
        Message::Request(r) => match r.payload {
            RequestPayload::TenantCreate(c) => {
                assert_eq!(u64::from(c.tenant_id), 42);
                assert_eq!(c.name.as_deref(), Some("acme-corp"));
            }
            other => panic!("expected TenantCreate, got {other:?}"),
        },
        other => panic!("expected Request, got {other:?}"),
    }

    // Create response
    let resp = Response::new(
        RequestId::new(1),
        ResponsePayload::TenantCreate(TenantCreateResponse {
            tenant: TenantInfo {
                tenant_id: TenantId::new(42),
                name: Some("acme-corp".into()),
                table_count: 0,
                created_at_nanos: Some(1_700_000_000_000_000_000),
            },
            created: true,
        }),
    );
    let frame = Message::Response(resp).to_frame().unwrap();
    match Message::from_frame(&frame).unwrap() {
        Message::Response(r) => match r.payload {
            ResponsePayload::TenantCreate(c) => {
                assert!(c.created);
                assert_eq!(u64::from(c.tenant.tenant_id), 42);
            }
            other => panic!("expected TenantCreate, got {other:?}"),
        },
        other => panic!("expected Response, got {other:?}"),
    }

    // List
    let resp = Response::new(
        RequestId::new(1),
        ResponsePayload::TenantList(TenantListResponse {
            tenants: vec![TenantInfo {
                tenant_id: TenantId::new(42),
                name: None,
                table_count: 3,
                created_at_nanos: None,
            }],
        }),
    );
    let frame = Message::Response(resp).to_frame().unwrap();
    match Message::from_frame(&frame).unwrap() {
        Message::Response(r) => match r.payload {
            ResponsePayload::TenantList(l) => assert_eq!(l.tenants.len(), 1),
            other => panic!("expected TenantList, got {other:?}"),
        },
        other => panic!("expected Response, got {other:?}"),
    }

    // Get — verify request shape
    let req = Request::new(
        RequestId::new(2),
        TenantId::new(1),
        RequestPayload::TenantGet(TenantGetRequest {
            tenant_id: TenantId::new(42),
        }),
    );
    let frame = Message::Request(req).to_frame().unwrap();
    assert!(matches!(
        Message::from_frame(&frame).unwrap(),
        Message::Request(r) if matches!(r.payload, RequestPayload::TenantGet(_))
    ));
}

#[test]
fn api_key_lifecycle_roundtrip() {
    // Register
    let req = Request::new(
        RequestId::new(1),
        TenantId::new(1),
        RequestPayload::ApiKeyRegister(ApiKeyRegisterRequest {
            subject: "alice".into(),
            tenant_id: TenantId::new(1),
            roles: vec!["User".into()],
            expires_at_nanos: None,
        }),
    );
    let frame = Message::Request(req).to_frame().unwrap();
    match Message::from_frame(&frame).unwrap() {
        Message::Request(r) => match r.payload {
            RequestPayload::ApiKeyRegister(k) => {
                assert_eq!(k.subject, "alice");
                assert_eq!(k.roles, vec!["User"]);
            }
            other => panic!("expected ApiKeyRegister, got {other:?}"),
        },
        other => panic!("expected Request, got {other:?}"),
    }

    // Register response
    let resp = Response::new(
        RequestId::new(1),
        ResponsePayload::ApiKeyRegister(ApiKeyRegisterResponse {
            key: "kmb_live_abcdef12345".into(),
            info: ApiKeyInfo {
                key_id: "abcdef12".into(),
                subject: "alice".into(),
                tenant_id: TenantId::new(1),
                roles: vec!["User".into()],
                expires_at_nanos: None,
            },
        }),
    );
    let frame = Message::Response(resp).to_frame().unwrap();
    match Message::from_frame(&frame).unwrap() {
        Message::Response(r) => match r.payload {
            ResponsePayload::ApiKeyRegister(k) => {
                assert!(k.key.starts_with("kmb_"));
                assert_eq!(k.info.key_id, "abcdef12");
            }
            other => panic!("expected ApiKeyRegister response, got {other:?}"),
        },
        other => panic!("expected Response, got {other:?}"),
    }

    // List
    let resp = Response::new(
        RequestId::new(2),
        ResponsePayload::ApiKeyList(ApiKeyListResponse {
            keys: vec![ApiKeyInfo {
                key_id: "abcdef12".into(),
                subject: "alice".into(),
                tenant_id: TenantId::new(1),
                roles: vec!["User".into()],
                expires_at_nanos: None,
            }],
        }),
    );
    let frame = Message::Response(resp).to_frame().unwrap();
    match Message::from_frame(&frame).unwrap() {
        Message::Response(r) => match r.payload {
            ResponsePayload::ApiKeyList(l) => {
                assert_eq!(l.keys.len(), 1);
                // List responses never carry plaintext — the `key_id` is a prefix only.
                assert_eq!(l.keys[0].key_id.len(), 8);
            }
            other => panic!("expected ApiKeyList, got {other:?}"),
        },
        other => panic!("expected Response, got {other:?}"),
    }

    // Rotate
    let req = Request::new(
        RequestId::new(3),
        TenantId::new(1),
        RequestPayload::ApiKeyRotate(ApiKeyRotateRequest {
            old_key: "kmb_live_abcdef12345".into(),
        }),
    );
    let frame = Message::Request(req).to_frame().unwrap();
    assert!(matches!(
        Message::from_frame(&frame).unwrap(),
        Message::Request(r) if matches!(r.payload, RequestPayload::ApiKeyRotate(_))
    ));

    let resp = Response::new(
        RequestId::new(3),
        ResponsePayload::ApiKeyRotate(ApiKeyRotateResponse {
            new_key: "kmb_live_xyz98765".into(),
            info: ApiKeyInfo {
                key_id: "xyz98765".into(),
                subject: "alice".into(),
                tenant_id: TenantId::new(1),
                roles: vec!["User".into()],
                expires_at_nanos: None,
            },
        }),
    );
    let frame = Message::Response(resp).to_frame().unwrap();
    assert!(matches!(
        Message::from_frame(&frame).unwrap(),
        Message::Response(r) if matches!(r.payload, ResponsePayload::ApiKeyRotate(_))
    ));
}

#[test]
fn server_info_roundtrip() {
    let req = Request::new(
        RequestId::new(1),
        TenantId::new(1),
        RequestPayload::GetServerInfo(GetServerInfoRequest::default()),
    );
    let frame = Message::Request(req).to_frame().unwrap();
    assert!(matches!(
        Message::from_frame(&frame).unwrap(),
        Message::Request(r) if matches!(r.payload, RequestPayload::GetServerInfo(_))
    ));

    let resp = Response::new(
        RequestId::new(1),
        ResponsePayload::ServerInfo(ServerInfoResponse {
            build_version: "0.5.0".into(),
            protocol_version: 4,
            capabilities: vec![
                "query".into(),
                "append".into(),
                "subscribe.v2".into(),
                "admin.v1".into(),
                "audit.v1".into(),
            ],
            uptime_secs: 3600,
            cluster_mode: ClusterMode::Standalone,
            tenant_count: 3,
        }),
    );
    let frame = Message::Response(resp).to_frame().unwrap();
    match Message::from_frame(&frame).unwrap() {
        Message::Response(r) => match r.payload {
            ResponsePayload::ServerInfo(info) => {
                assert_eq!(info.build_version, "0.5.0");
                assert_eq!(info.protocol_version, 4);
                assert_eq!(info.cluster_mode, ClusterMode::Standalone);
                assert_eq!(info.tenant_count, 3);
                assert!(info.capabilities.iter().any(|c| c == "admin.v1"));
                assert!(info.capabilities.iter().any(|c| c == "audit.v1"));
            }
            other => panic!("expected ServerInfo, got {other:?}"),
        },
        other => panic!("expected Response, got {other:?}"),
    }
}

// ============================================================================
// Protocol v3 ↔ v4 back-compat matrix
// ============================================================================
//
// v4 introduces a trailing `Option<ConsentBasis>` on `ConsentGrantRequest`
// and `ConsentRecord`. Four cells to cover:
//
//   1. v4 client → v4 server — basis round-trips.
//   2. v3 client → v3 server — unchanged baseline (bytewise identical to
//      a v4 payload with `basis = None` because postcard encodes the
//      trailing `Option` as a single `0x00` tag byte).
//   3. v3 client → v4 server — v3-shape payload (no `basis` field) decodes
//      cleanly against the v4 struct *iff* the producer emits the trailing
//      `None` tag byte; otherwise the frame-header version check rejects
//      the connection before payload decoding runs.
//   4. v4 client → v3 server — basis=None payload decodes fine; basis=Some
//      requires a `ServerTooOld` signal because a v3 server won't emit the
//      `basis` field on read. The frame version mismatch handles this at
//      the handshake layer; these tests codify the payload-level behaviour.

mod v3_v4_compat {
    use super::*;
    use crate::message::{
        ConsentBasis, ConsentGrantRequest, ConsentPurpose, ConsentRecord, ConsentScope,
        GdprArticle,
    };

    /// v4 shape with a v3-shaped "preimage" struct used to craft a
    /// payload that was produced by a pre-v4 client. Field order
    /// MUST mirror `ConsentGrantRequest` up to — but not including
    /// — the new `basis` tail field, since postcard is positional.
    #[derive(serde::Serialize)]
    struct V3ConsentGrantRequest {
        subject_id: String,
        purpose: ConsentPurpose,
        scope: Option<ConsentScope>,
    }

    /// Reserved for future record-shape assertions (e.g., v3 server
    /// serialising back to a v4 client that ignores unknown fields).
    /// Kept here so the back-compat story is expressed in one file.
    #[allow(dead_code)]
    #[derive(serde::Serialize)]
    struct V3ConsentRecord {
        consent_id: String,
        subject_id: String,
        purpose: ConsentPurpose,
        scope: ConsentScope,
        granted_at_nanos: u64,
        withdrawn_at_nanos: Option<u64>,
        expires_at_nanos: Option<u64>,
        notes: Option<String>,
    }

    // ----- Cell 1: v4 ↔ v4 — basis round-trips -----------------------

    #[test]
    fn v4_v4_basis_roundtrips_through_grant_request() {
        let req = ConsentGrantRequest {
            subject_id: "alice".into(),
            purpose: ConsentPurpose::Marketing,
            scope: Some(ConsentScope::AllData),
            basis: Some(ConsentBasis {
                article: GdprArticle::Consent,
                justification: Some("opt-in at signup".into()),
            }),
        };
        let bytes = postcard::to_allocvec(&req).unwrap();
        let decoded: ConsentGrantRequest = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.subject_id, "alice");
        let basis = decoded.basis.expect("basis must round-trip");
        assert_eq!(basis.article, GdprArticle::Consent);
        assert_eq!(basis.justification.as_deref(), Some("opt-in at signup"));
    }

    #[test]
    fn v4_v4_basis_roundtrips_through_record() {
        let rec = ConsentRecord {
            consent_id: "00000000-0000-0000-0000-000000000001".into(),
            subject_id: "bob".into(),
            purpose: ConsentPurpose::Research,
            scope: ConsentScope::AllData,
            granted_at_nanos: 1_700_000_000_000_000_000,
            withdrawn_at_nanos: None,
            expires_at_nanos: None,
            notes: None,
            basis: Some(ConsentBasis {
                article: GdprArticle::LegitimateInterests,
                justification: None,
            }),
        };
        let bytes = postcard::to_allocvec(&rec).unwrap();
        let decoded: ConsentRecord = postcard::from_bytes(&bytes).unwrap();
        let basis = decoded.basis.expect("basis must round-trip");
        assert_eq!(basis.article, GdprArticle::LegitimateInterests);
        assert!(basis.justification.is_none());
    }

    // ----- Cell 2: v3 ↔ v3 — unchanged baseline ----------------------
    //
    // A v3-shaped payload equals a v4 payload with `basis = None` modulo
    // the trailing `None` tag byte, which postcard emits as a single
    // `0x00`. This test confirms that a v4 producer writing
    // `basis = None` is bytewise equivalent to the v3 producer PLUS the
    // terminating zero byte.

    #[test]
    fn v3_v3_baseline_equals_v4_with_basis_none_plus_tag_byte() {
        let v3 = V3ConsentGrantRequest {
            subject_id: "alice".into(),
            purpose: ConsentPurpose::Marketing,
            scope: Some(ConsentScope::AllData),
        };
        let v4 = ConsentGrantRequest {
            subject_id: "alice".into(),
            purpose: ConsentPurpose::Marketing,
            scope: Some(ConsentScope::AllData),
            basis: None,
        };
        let v3_bytes = postcard::to_allocvec(&v3).unwrap();
        let v4_bytes = postcard::to_allocvec(&v4).unwrap();
        // v4 with basis=None = v3 bytes + single 0x00 tag byte.
        assert_eq!(v4_bytes.len(), v3_bytes.len() + 1);
        assert_eq!(&v4_bytes[..v3_bytes.len()], &v3_bytes[..]);
        assert_eq!(v4_bytes[v3_bytes.len()], 0x00);
    }

    // ----- Cell 3: v3 client → v4 server -----------------------------
    //
    // The v3 client frames the request as `version = 3`. The v4 server's
    // FrameHeader::validate() rejects it with UnsupportedVersion BEFORE
    // attempting payload decoding. This is the correct behaviour for
    // major-version bumps; clients/servers negotiate via handshake and
    // fail fast on mismatch.
    //
    // The payload-level story: even if we stripped the frame and fed a
    // v3-shaped payload to the v4 decoder, postcard would fail with
    // UnexpectedEnd because the `basis` tag byte is missing. We codify
    // both behaviours so regressions are caught.

    #[test]
    fn v3_client_to_v4_server_frame_header_rejects() {
        // Craft a frame that claims version 3 but contains a v4-encoded
        // payload. The validator rejects on version alone.
        use bytes::BytesMut;

        let payload = postcard::to_allocvec(&V3ConsentGrantRequest {
            subject_id: "alice".into(),
            purpose: ConsentPurpose::Marketing,
            scope: None,
        })
        .unwrap();

        let mut buf = BytesMut::new();
        let v3_header = FrameHeader {
            magic: crate::frame::MAGIC,
            version: 3,
            length: payload.len() as u32,
            checksum: crate::frame::compute_checksum(&payload),
        };
        v3_header.encode(&mut buf);
        buf.extend_from_slice(&payload);

        let mut read = buf.clone();
        let decoded_header = FrameHeader::decode(&mut read).unwrap();
        match decoded_header.validate() {
            Err(WireError::UnsupportedVersion(v)) => assert_eq!(v, 3),
            other => panic!("expected UnsupportedVersion(3), got {other:?}"),
        }
    }

    #[test]
    fn v3_payload_fed_to_v4_decoder_errors_with_unexpected_end() {
        // Bypass the frame validator and stress postcard directly. A
        // v3-shaped payload is missing the trailing `basis` tag byte;
        // postcard must error instead of silently defaulting.
        let v3 = V3ConsentGrantRequest {
            subject_id: "alice".into(),
            purpose: ConsentPurpose::Marketing,
            scope: Some(ConsentScope::AllData),
        };
        let v3_bytes = postcard::to_allocvec(&v3).unwrap();
        let err = postcard::from_bytes::<ConsentGrantRequest>(&v3_bytes).unwrap_err();
        // Postcard reports `DeserializeUnexpectedEnd` when the buffer
        // runs out mid-decode. We match the variant name in Debug form
        // to stay stable across postcard minor bumps.
        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("UnexpectedEnd") || rendered.contains("Eof"),
            "expected UnexpectedEnd / Eof, got: {rendered}"
        );
    }

    // ----- Cell 4: v4 client → v3 server -----------------------------
    //
    // A v4 client that sends `basis = Some(...)` to a v3 server must
    // get a clean "server too old" signal. At the frame layer this is
    // the same UnsupportedVersion error as cell 3, produced by the v3
    // server's header validator when it sees `version = 4`. At the
    // payload layer (simulated by decoding v4 bytes into a v3 struct),
    // postcard reads the v3-shape fields and STOPS — the trailing
    // `basis` bytes linger in the buffer. Without `from_bytes_cobs` or
    // an explicit length prefix postcard's `from_bytes` tolerates
    // trailing input, so the v3 server would silently drop `basis`.
    // We guard against that by asserting the v4 client populates a
    // capability flag via the handshake before sending basis=Some.
    //
    // For this suite we codify two behaviours: (a) basis=None
    // round-trips cleanly into a v3-shape decode, and (b) basis=Some
    // yields extra bytes a v3 server would ignore — which is exactly
    // why the handshake MUST reject v4-framed payloads at a v3 server.

    #[test]
    fn v4_client_basis_none_decodes_cleanly_into_v3_shape() {
        let v4 = ConsentGrantRequest {
            subject_id: "alice".into(),
            purpose: ConsentPurpose::Marketing,
            scope: Some(ConsentScope::AllData),
            basis: None,
        };
        let v4_bytes = postcard::to_allocvec(&v4).unwrap();

        // Simulate v3 server decoding: strip the trailing tag byte and
        // deserialize into the v3-shape struct (which lacks `basis`).
        // Using `take_from_bytes` we verify exactly one extra byte
        // remains — the `None` tag emitted by the v4 serializer.
        let (decoded_v3, rest): (V3Readable, _) =
            postcard::take_from_bytes(&v4_bytes).unwrap();
        assert_eq!(decoded_v3.subject_id, "alice");
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0], 0x00, "trailing byte must be the basis=None tag");
    }

    #[test]
    fn v4_client_basis_some_leaves_extra_bytes_for_v3_server() {
        let v4 = ConsentGrantRequest {
            subject_id: "alice".into(),
            purpose: ConsentPurpose::Marketing,
            scope: Some(ConsentScope::AllData),
            basis: Some(ConsentBasis {
                article: GdprArticle::Consent,
                justification: Some("clinical research opt-in".into()),
            }),
        };
        let v4_bytes = postcard::to_allocvec(&v4).unwrap();

        // A v3 server would decode the v3-shape prefix fine but leave
        // the basis bytes in the buffer — data loss. The real server
        // MUST reject v4-framed payloads at the header validator.
        let (decoded_v3, rest): (V3Readable, _) =
            postcard::take_from_bytes(&v4_bytes).unwrap();
        assert_eq!(decoded_v3.subject_id, "alice");
        assert!(
            rest.len() > 1,
            "basis=Some must leave more than just the tag byte: {}",
            rest.len()
        );
    }

    /// Read-only v3-shape mirror used by the compat tests. Separate
    /// from `V3ConsentGrantRequest` because `Deserialize` doesn't want
    /// the `#[derive(Serialize)]` bound.
    #[derive(serde::Deserialize)]
    struct V3Readable {
        subject_id: String,
        #[allow(dead_code)]
        purpose: ConsentPurpose,
        #[allow(dead_code)]
        scope: Option<ConsentScope>,
    }
}
