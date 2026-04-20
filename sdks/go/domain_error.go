package kimberlite

import (
	"errors"
	"strings"
)

// DomainError is a typed translation of wire-level errors into the
// categories application code actually wants to dispatch on (HTTP
// status, Slack alert, retry policy, etc.).
//
// AUDIT-2026-04 S4.2 — mirrors the TypeScript and Rust SDK
// DomainError surfaces so every downstream app that talks to
// Kimberlite from Go can do the same pattern-matching on outcomes
// without rewriting a translator.
type DomainError struct {
	// Kind classifies the failure.
	Kind DomainErrorKind
	// Message is a human-readable description.
	Message string
	// Reasons lists conflict-specific details (populated for
	// Kind=ConflictError — e.g. "stream already exists").
	Reasons []string
	// Name names the violated invariant (populated for
	// Kind=InvariantViolation).
	Name string
}

// DomainErrorKind enumerates the shapes consumers typically want to
// branch on. Deliberately coarse — fine-grained taxonomy lives in
// KimberliteError.Code.
type DomainErrorKind int

const (
	DomainKindUnavailable DomainErrorKind = iota
	DomainKindNotFound
	DomainKindForbidden
	DomainKindConcurrentModification
	DomainKindConflict
	DomainKindInvariantViolation
	DomainKindRateLimited
	DomainKindTimeout
	DomainKindValidation
)

func (e *DomainError) Error() string {
	return "kimberlite: " + e.Kind.String() + ": " + e.Message
}

// String returns a stable name suitable for logs / metrics tags.
func (k DomainErrorKind) String() string {
	switch k {
	case DomainKindNotFound:
		return "NotFound"
	case DomainKindForbidden:
		return "Forbidden"
	case DomainKindConcurrentModification:
		return "ConcurrentModification"
	case DomainKindConflict:
		return "Conflict"
	case DomainKindInvariantViolation:
		return "InvariantViolation"
	case DomainKindRateLimited:
		return "RateLimited"
	case DomainKindTimeout:
		return "Timeout"
	case DomainKindValidation:
		return "Validation"
	default:
		return "Unavailable"
	}
}

// MapKimberliteError translates any error returned by the Kimberlite
// client into a structured DomainError. Non-Kimberlite errors fall
// through to DomainKindUnavailable with the raw message — never
// opaque, never reveals stack frames.
//
//	rows, err := client.Query(...)
//	if err != nil {
//	    d := kimberlite.MapKimberliteError(err)
//	    switch d.Kind {
//	    case kimberlite.DomainKindConcurrentModification:
//	        return retry()
//	    case kimberlite.DomainKindForbidden:
//	        return http.StatusForbidden
//	    ...
//	    }
//	}
func MapKimberliteError(err error) *DomainError {
	if err == nil {
		return nil
	}

	// Sentinel errors take precedence.
	switch {
	case errors.Is(err, ErrStreamNotFound):
		return &DomainError{Kind: DomainKindNotFound, Message: err.Error()}
	case errors.Is(err, ErrPermissionDenied):
		return &DomainError{Kind: DomainKindForbidden, Message: err.Error()}
	case errors.Is(err, ErrTimeout):
		return &DomainError{Kind: DomainKindTimeout, Message: err.Error()}
	case errors.Is(err, ErrNotConnected), errors.Is(err, ErrConnectionFailed):
		return &DomainError{Kind: DomainKindUnavailable, Message: err.Error()}
	}

	// Rich KimberliteError from the wire.
	var ke *KimberliteError
	if errors.As(err, &ke) {
		return mapByCode(ke)
	}

	return &DomainError{Kind: DomainKindUnavailable, Message: err.Error()}
}

func mapByCode(e *KimberliteError) *DomainError {
	switch e.Code {
	case "OffsetMismatch":
		return &DomainError{Kind: DomainKindConcurrentModification, Message: e.Message}
	case "StreamNotFound", "TableNotFound", "TenantNotFound", "ApiKeyNotFound":
		return &DomainError{Kind: DomainKindNotFound, Message: e.Message}
	case "AuthenticationFailed":
		return &DomainError{Kind: DomainKindForbidden, Message: e.Message}
	case "RateLimited":
		return &DomainError{Kind: DomainKindRateLimited, Message: e.Message}
	case "Timeout":
		return &DomainError{Kind: DomainKindTimeout, Message: e.Message}
	case "QueryParseError", "InvalidRequest", "InvalidOffset":
		return &DomainError{Kind: DomainKindValidation, Message: e.Message}
	case "TenantAlreadyExists", "StreamAlreadyExists":
		return &DomainError{
			Kind:    DomainKindConflict,
			Message: e.Message,
			Reasons: []string{strings.TrimSpace(e.Message)},
		}
	default:
		return &DomainError{Kind: DomainKindUnavailable, Message: e.Message}
	}
}
