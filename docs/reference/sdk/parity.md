# SDK parity matrix — v0.6.0

Source-of-truth for "does feature X exist in SDK Y?" across the three
supported language SDKs (Rust / TypeScript / Python). Update this table
whenever a new wire primitive lands.

> **Wire protocol**: v4 — `ConsentGrantRequest` / `ConsentRecord` carry an
> optional `ConsentBasis` (GDPR Article 6(1) lawful basis + justification).
> v3 clients must re-handshake; see `crates/kimberlite-wire/src/tests.rs`
> (`v3_v4_compat` module) for the back-compat test matrix.

## Core data plane

| Feature | Rust | TypeScript | Python |
|---|---|---|---|
| `connect` | ✅ | ✅ | ✅ |
| `create_stream` (with placement) | ✅ | ✅ | ✅ |
| `append` (optimistic concurrency) | ✅ | ✅ | ✅ |
| `read_events` | ✅ | ✅ | ✅ |
| `query` | ✅ | ✅ | ✅ |
| `query_at` (time-travel) | ✅ | ✅ | ✅ |
| `execute` (DML, returns ExecuteResult) | ✅ | ✅ | ✅ |
| `sync` | ✅ | ✅ | ✅ |
| `tenant_id` getter | ✅ | ✅ | ✅ |
| `last_request_id` (tracing correlation) | ✅ | ✅ | ✅ |
| Typed row mapping | ✅ (`query_typed<T>`) | ✅ (`queryRows<T>`) | ✅ (`query_model(model=…)`) |

## Connection pooling

| Feature | Rust | TypeScript | Python |
|---|---|---|---|
| `Pool` with idle eviction | ✅ | ✅ (via napi) | ✅ (via FFI) |
| RAII / context manager release | ✅ (`PooledClient` Drop) | ✅ (`withClient`) | ✅ (`with pool.acquire()`) |
| Stats (max/open/idle/in_use/shutdown) | ✅ | ✅ | ✅ |
| Cancellation-safe | ✅ | ✅ | ✅ |

## Real-time subscriptions (protocol v2)

| Feature | Rust | TypeScript | Python |
|---|---|---|---|
| `subscribe` | ✅ | ✅ | ✅ |
| `grant_credits` | ✅ | ✅ | ✅ |
| `unsubscribe` | ✅ | ✅ | ✅ |
| Iterator / AsyncIterator | ✅ (sync iter) | ✅ (`for await`) | ✅ (`for ev in sub`) |
| Auto-refill credits | ✅ | ✅ | ✅ |
| Close-reason surfacing | ✅ | ✅ | ✅ |

## Admin operations

| Feature | Rust | TypeScript | Python |
|---|---|---|---|
| `list_tables` | ✅ | ✅ (`admin.listTables`) | ✅ (`admin.list_tables`) |
| `describe_table` | ✅ | ✅ | ✅ |
| `list_indexes` | ✅ | ✅ | ✅ |
| `tenant_create` / `_list` / `_delete` / `_get` | ✅ | ✅ | ✅ |
| `api_key_register` / `_revoke` / `_list` / `_rotate` | ✅ | ✅ | ✅ |
| `server_info` | ✅ | ✅ | ✅ |

## Compliance — consent + erasure (Phase 5)

| Feature | Rust | TypeScript | Python |
|---|---|---|---|
| `consent.grant` | ✅ | ✅ (`compliance.consent.grant`) | ✅ |
| `consent.basis` (GDPR Art 6(1) lawful basis + justification, wire v4) | ✅ | ✅ | ✅ |
| `consent.withdraw` | ✅ | ✅ | ✅ |
| `consent.check` | ✅ | ✅ | ✅ |
| `consent.list` | ✅ | ✅ | ✅ |
| `erasure.request` | ✅ | ✅ | ✅ |
| `erasure.mark_progress` | ✅ | ✅ | ✅ |
| `erasure.mark_stream_erased` | ✅ | ✅ | ✅ |
| `erasure.complete` | ✅ | ✅ | ✅ |
| `erasure.exempt` | ✅ | ✅ | ✅ |
| `erasure.status` | ✅ | ✅ | ✅ |
| `erasure.list` | ✅ | ✅ | ✅ |

## Compliance — audit / export / breach (Phase 6)

Wire surface defined in v0.5.0; server-side handlers landed in v0.5.0
(ROADMAP item C). All seven endpoints are end-to-end in Rust; Rust has
in-process E2E test coverage (`crates/kimberlite-client/tests/e2e_compliance_phase6.rs`).
TypeScript and Python SDKs use the same wire surface and are covered by
their existing test harnesses.

| Feature | Rust | TypeScript | Python |
|---|---|---|---|
| `audit_query` | ✅ | ✅ via wire | ✅ via wire |
| `export_subject` | ✅ | ✅ | ✅ |
| `verify_export` | ✅ | ✅ | ✅ |
| `breach_report_indicator` | ✅ | ✅ | ✅ |
| `breach_query_status` | ✅ | ✅ | ✅ |
| `breach_confirm` | ✅ | ✅ | ✅ |
| `breach_resolve` | ✅ | ✅ | ✅ |
| Masking policy CRUD | 🚧 v0.6 | 🚧 v0.6 | 🚧 v0.6 |

Legend: ✅ shipped • 🚧 deferred to a later release.

## Ergonomics (Phase 7)

| Feature | Rust | TypeScript | Python |
|---|---|---|---|
| Fluent `Query` builder | ✅ | ✅ | ✅ |
| ESM + CJS dual exports | — | ✅ | — |
| Async client (event-loop-friendly) | ✅ (napi-rs under the hood) | ✅ (native Promise) | ✅ (`kimberlite.aio.AsyncClient`) |

## Observability

| Feature | Rust | TypeScript | Python |
|---|---|---|---|
| `tracing::instrument` / request-ID propagation | ✅ | ✅ | ✅ |
| Structured errors with `code: ErrorCode` | ✅ | ✅ | ✅ |
| `isRetryable()` / `is_retryable()` | ✅ | ✅ | ✅ (via `ClientError.code`) |

## Framework integration examples (Phase 8)

| Framework | Location |
|---|---|
| axum | `examples/rust/src/axum_app.rs` |
| actix-web | `examples/rust/src/actix_app.rs` |
| Express | `examples/typescript/express-app/` |
| Next.js | `examples/typescript/nextjs-app/` |
| FastAPI | `examples/python/fastapi-app/` |
| Django | `examples/python/django-app/` |
