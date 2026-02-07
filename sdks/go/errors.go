package kimberlite

import "errors"

// Sentinel errors returned by the Kimberlite client.
var (
	// ErrNotConnected is returned when calling methods on a closed or uninitialized client.
	ErrNotConnected = errors.New("kimberlite: not connected")

	// ErrConnectionFailed is returned when the client cannot establish a connection.
	ErrConnectionFailed = errors.New("kimberlite: connection failed")

	// ErrQueryFailed is returned when a SQL query fails to execute.
	ErrQueryFailed = errors.New("kimberlite: query failed")

	// ErrStreamNotFound is returned when a stream does not exist.
	ErrStreamNotFound = errors.New("kimberlite: stream not found")

	// ErrTenantRequired is returned when a tenant ID is required but not provided.
	ErrTenantRequired = errors.New("kimberlite: tenant ID required")

	// ErrPermissionDenied is returned when the operation is not authorized.
	ErrPermissionDenied = errors.New("kimberlite: permission denied")

	// ErrTimeout is returned when an operation exceeds its deadline.
	ErrTimeout = errors.New("kimberlite: operation timed out")

	// ErrFFIUnavailable is returned when the native FFI library is not loaded.
	ErrFFIUnavailable = errors.New("kimberlite: FFI library not available (CGo required)")
)

// KimberliteError wraps an error with additional context from the server.
type KimberliteError struct {
	// Code is the server error code, if available.
	Code string
	// Message is the human-readable error message.
	Message string
	// Cause is the underlying error.
	Cause error
}

func (e *KimberliteError) Error() string {
	if e.Code != "" {
		return "kimberlite [" + e.Code + "]: " + e.Message
	}
	return "kimberlite: " + e.Message
}

func (e *KimberliteError) Unwrap() error {
	return e.Cause
}
