//! Integration tests for the wire protocol.

use bytes::BytesMut;
use kimberlite_types::{DataClass, Offset, Placement, StreamId, TenantId};

use crate::frame::{FRAME_HEADER_SIZE, Frame, PROTOCOL_VERSION};
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
fn protocol_version_is_three() {
    // v3 bump — Request now carries optional SDK audit attribution.
    // v1 and v2 clients must fail the handshake cleanly.
    assert_eq!(PROTOCOL_VERSION, 3);
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
    assert_eq!(decoded.audit.as_ref().unwrap().actor.as_deref(), Some("user-42"));
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
            protocol_version: 3,
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
                assert_eq!(info.protocol_version, 3);
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
