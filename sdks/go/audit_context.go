package kimberlite

// AUDIT-2026-04 S3.9 — audit context propagation for the Go SDK.
//
// AuditContext carries caller attribution (actor, reason, optional
// correlation id and idempotency key) to the server so every
// compliance-tracked mutation is recorded with the operator's
// identity — not just the service account that holds the API key.
//
// Usage mirrors notebar's React Router v7 pattern: wrap each request
// handler in WithAudit so every SDK call in the downstream chain
// picks up the context transparently.
//
//	func chartHandler(w http.ResponseWriter, r *http.Request) {
//	    ctx := kimberlite.WithAudit(r.Context(), kimberlite.AuditContext{
//	        Actor:         session.UserID(r),
//	        Reason:        "patient-chart-view",
//	        CorrelationID: r.Header.Get("X-Request-Id"),
//	    })
//	    rows, err := client.QueryContext(ctx, "SELECT * FROM patients WHERE id = $1", id)
//	    ...
//	}

/*
#include <stdint.h>
#include <stdlib.h>

// Thread-local audit hooks exposed by libkimberlite_ffi.
// See crates/kimberlite-ffi/src/lib.rs.
extern int kmb_audit_set(
    const char* actor,
    const char* reason,
    const char* correlation_id,
    const char* idempotency_key
);
extern int kmb_audit_clear(void);
*/
import "C"

import (
	"context"
	"sync"
	"unsafe"
)

// AuditContext holds caller attribution for a single logical operation.
// Both Actor and Reason are mandatory in regulated-industry apps
// (HIPAA minimum-necessary, GDPR purpose limitation, FedRAMP
// audit-trail completeness).
type AuditContext struct {
	// Actor is an opaque identifier for the principal (user UUID,
	// email, role name). Required.
	Actor string
	// Reason is the "why" of the access — critical for break-glass
	// reads and HIPAA minimum-necessary justification. Required.
	Reason string
	// CorrelationID ties together a span of related calls (typically
	// an HTTP X-Request-Id). Optional.
	CorrelationID string
	// IdempotencyKey lets servers deduplicate retries sharing the
	// same key. Optional.
	IdempotencyKey string
}

type auditKey struct{}

// WithAudit returns a derived context carrying ctx. Every Kimberlite
// client call that takes a context.Context will pick this up and
// attach it to the outgoing wire Request.audit.
func WithAudit(parent context.Context, ctx AuditContext) context.Context {
	return context.WithValue(parent, auditKey{}, ctx)
}

// AuditFromContext extracts the AuditContext from ctx, or (zero, false)
// if none is set.
func AuditFromContext(ctx context.Context) (AuditContext, bool) {
	if ctx == nil {
		return AuditContext{}, false
	}
	v, ok := ctx.Value(auditKey{}).(AuditContext)
	return v, ok
}

// ffiAuditMu serialises access to the process-wide FFI audit
// thread-local. Go routines may be migrated between OS threads; we
// take the mutex, set, call, and clear atomically per SDK method so
// attribution never leaks across calls.
var ffiAuditMu sync.Mutex

// withFFIAudit installs ctx on the FFI thread-local for the duration
// of fn. No-op if ctx is the zero value / missing.
//
// Internal helper — every exported client method wraps its CGo call
// in this so callers don't need to thread anything manually.
func withFFIAudit(ctx context.Context, fn func() error) error {
	audit, ok := AuditFromContext(ctx)
	if !ok {
		return fn()
	}

	ffiAuditMu.Lock()
	defer ffiAuditMu.Unlock()

	cActor := cStringOrNil(audit.Actor)
	cReason := cStringOrNil(audit.Reason)
	cCorr := cStringOrNil(audit.CorrelationID)
	cIdem := cStringOrNil(audit.IdempotencyKey)
	defer func() {
		if cActor != nil {
			C.free(unsafe.Pointer(cActor))
		}
		if cReason != nil {
			C.free(unsafe.Pointer(cReason))
		}
		if cCorr != nil {
			C.free(unsafe.Pointer(cCorr))
		}
		if cIdem != nil {
			C.free(unsafe.Pointer(cIdem))
		}
	}()

	C.kmb_audit_set(cActor, cReason, cCorr, cIdem)
	defer C.kmb_audit_clear()
	return fn()
}

// cStringOrNil returns a freshly-allocated C string, or nil for empty.
// Caller must free the returned pointer.
func cStringOrNil(s string) *C.char {
	if s == "" {
		return nil
	}
	return C.CString(s)
}
