//! # Kimberlite FFI
//!
//! C-compatible Foreign Function Interface for Kimberlite database.
//!
//! This crate provides a stable C ABI for language-specific SDK wrappers.
//! All functions use C-compatible types and follow these conventions:
//!
//! - Return `KmbError` (error code)
//! - Use out-parameters for results (e.g., `stream_id_out`)
//! - NULL-check all pointers
//! - UTF-8 validate all strings
//! - Bounds-check all arrays
//!
//! ## Memory Management
//!
//! - **Client-owned**: Caller must free with `kmb_*_free()` functions
//! - **Library-owned**: Valid until next call or client disconnect
//!
//! ## Thread Safety
//!
//! `KmbClient` is NOT thread-safe. Callers must synchronize access or use
//! separate client instances per thread.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::slice;

/// Error codes returned by all FFI functions.
///
/// Error code 0 (`KMB_OK`) indicates success.
/// All other codes indicate various failure modes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KmbError {
    /// Success (no error)
    KmbOk = 0,
    /// NULL pointer passed where non-NULL required
    KmbErrNullPointer = 1,
    /// String is not valid UTF-8
    KmbErrInvalidUtf8 = 2,
    /// Failed to connect to server
    KmbErrConnectionFailed = 3,
    /// Stream ID does not exist
    KmbErrStreamNotFound = 4,
    /// Operation not permitted for this tenant
    KmbErrPermissionDenied = 5,
    /// Invalid data class value
    KmbErrInvalidDataClass = 6,
    /// Offset is beyond stream end
    KmbErrOffsetOutOfRange = 7,
    /// SQL syntax error
    KmbErrQuerySyntax = 8,
    /// Query execution error
    KmbErrQueryExecution = 9,
    /// Tenant ID does not exist
    KmbErrTenantNotFound = 10,
    /// Authentication failed
    KmbErrAuthFailed = 11,
    /// Operation timed out
    KmbErrTimeout = 12,
    /// Internal server error
    KmbErrInternal = 13,
    /// No cluster replicas available
    KmbErrClusterUnavailable = 14,
    /// Unknown error
    KmbErrUnknown = 15,
}

/// Opaque handle to a client connection.
///
/// Created by `kmb_client_connect()`, freed by `kmb_client_disconnect()`.
#[repr(C)]
pub struct KmbClient {
    _private: [u8; 0],
}

/// Client connection configuration.
#[repr(C)]
pub struct KmbClientConfig {
    /// Array of "host:port" strings (NULL-terminated)
    pub addresses: *const *const c_char,
    /// Number of addresses
    pub address_count: usize,
    /// Tenant ID
    pub tenant_id: u64,
    /// Authentication token (NULL-terminated, may be empty)
    pub auth_token: *const c_char,
    /// Client name (e.g., "kimberlite-python")
    pub client_name: *const c_char,
    /// Client version (e.g., "0.1.0")
    pub client_version: *const c_char,
}

/// Data classification for streams.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KmbDataClass {
    /// Protected Health Information (HIPAA-regulated)
    KmbDataClassPhi = 0,
    /// Non-PHI data
    KmbDataClassNonPhi = 1,
    /// De-identified data
    KmbDataClassDeidentified = 2,
}

// Placeholder implementations - will be implemented in Phase 11.6
// These are stubs to make the crate compile and generate header

/// Connect to Kimberlite cluster.
///
/// # Arguments
/// - `config`: Connection configuration
/// - `client_out`: Output parameter for client handle
///
/// # Returns
/// - `KMB_OK` on success
/// - Error code on failure
///
/// # Safety
/// - `config` must be valid
/// - All string pointers in config must be valid NULL-terminated C strings
/// - Caller must call `kmb_client_disconnect()` to free client
#[no_mangle]
pub unsafe extern "C" fn kmb_client_connect(
    config: *const KmbClientConfig,
    client_out: *mut *mut KmbClient,
) -> KmbError {
    if config.is_null() || client_out.is_null() {
        return KmbError::KmbErrNullPointer;
    }

    // TODO: Implement actual connection logic
    // For now, return error to indicate not implemented
    KmbError::KmbErrInternal
}

/// Disconnect from cluster and free client.
///
/// # Arguments
/// - `client`: Client handle from `kmb_client_connect()`
///
/// # Safety
/// - `client` must be a valid handle from `kmb_client_connect()`
/// - After this call, `client` is invalid and must not be used
#[no_mangle]
pub unsafe extern "C" fn kmb_client_disconnect(client: *mut KmbClient) {
    if client.is_null() {
        return;
    }

    // TODO: Implement disconnect logic
}

/// Create a new stream.
///
/// # Arguments
/// - `client`: Client handle
/// - `name`: Stream name (NULL-terminated UTF-8)
/// - `data_class`: Data classification
/// - `stream_id_out`: Output parameter for stream ID
///
/// # Returns
/// - `KMB_OK` on success
/// - Error code on failure
///
/// # Safety
/// - `client` must be valid
/// - `name` must be valid NULL-terminated UTF-8 string
/// - `stream_id_out` must be valid pointer
#[no_mangle]
pub unsafe extern "C" fn kmb_client_create_stream(
    client: *mut KmbClient,
    name: *const c_char,
    data_class: KmbDataClass,
    stream_id_out: *mut u64,
) -> KmbError {
    if client.is_null() || name.is_null() || stream_id_out.is_null() {
        return KmbError::KmbErrNullPointer;
    }

    // Validate UTF-8
    let name_cstr = match CStr::from_ptr(name).to_str() {
        Ok(s) => s,
        Err(_) => return KmbError::KmbErrInvalidUtf8,
    };

    // TODO: Implement create stream logic
    KmbError::KmbErrInternal
}

/// Append events to a stream.
///
/// # Arguments
/// - `client`: Client handle
/// - `stream_id`: Stream ID
/// - `events`: Array of byte buffers
/// - `event_lengths`: Parallel array of buffer lengths
/// - `event_count`: Number of events
/// - `first_offset_out`: Output parameter for first offset
///
/// # Returns
/// - `KMB_OK` on success
/// - Error code on failure
///
/// # Safety
/// - `client` must be valid
/// - `events` must be array of `event_count` valid pointers
/// - `event_lengths` must be array of `event_count` lengths
/// - `first_offset_out` must be valid pointer
#[no_mangle]
pub unsafe extern "C" fn kmb_client_append(
    client: *mut KmbClient,
    stream_id: u64,
    events: *const *const u8,
    event_lengths: *const usize,
    event_count: usize,
    first_offset_out: *mut u64,
) -> KmbError {
    if client.is_null() || events.is_null() || event_lengths.is_null() || first_offset_out.is_null()
    {
        return KmbError::KmbErrNullPointer;
    }

    // TODO: Implement append logic
    KmbError::KmbErrInternal
}

/// Get human-readable error message for error code.
///
/// # Arguments
/// - `error`: Error code
///
/// # Returns
/// - Static NULL-terminated string (do not free)
///
/// # Safety
/// - Always safe to call
/// - Returned string is valid for lifetime of program
#[no_mangle]
pub unsafe extern "C" fn kmb_error_message(error: KmbError) -> *const c_char {
    let msg = match error {
        KmbError::KmbOk => "Success\0",
        KmbError::KmbErrNullPointer => "NULL pointer argument\0",
        KmbError::KmbErrInvalidUtf8 => "String is not valid UTF-8\0",
        KmbError::KmbErrConnectionFailed => "Failed to connect to server\0",
        KmbError::KmbErrStreamNotFound => "Stream ID does not exist\0",
        KmbError::KmbErrPermissionDenied => "Operation not permitted\0",
        KmbError::KmbErrInvalidDataClass => "Invalid data class value\0",
        KmbError::KmbErrOffsetOutOfRange => "Offset is beyond stream end\0",
        KmbError::KmbErrQuerySyntax => "SQL syntax error\0",
        KmbError::KmbErrQueryExecution => "Query execution error\0",
        KmbError::KmbErrTenantNotFound => "Tenant ID does not exist\0",
        KmbError::KmbErrAuthFailed => "Authentication failed\0",
        KmbError::KmbErrTimeout => "Operation timed out\0",
        KmbError::KmbErrInternal => "Internal server error\0",
        KmbError::KmbErrClusterUnavailable => "No cluster replicas available\0",
        KmbError::KmbErrUnknown => "Unknown error\0",
    };

    msg.as_ptr() as *const c_char
}

/// Check if an error code indicates a retryable failure.
///
/// # Arguments
/// - `error`: Error code
///
/// # Returns
/// - 1 if retryable, 0 otherwise
///
/// # Safety
/// - Always safe to call
#[no_mangle]
pub unsafe extern "C" fn kmb_error_is_retryable(error: KmbError) -> c_int {
    match error {
        KmbError::KmbErrTimeout
        | KmbError::KmbErrClusterUnavailable
        | KmbError::KmbErrInternal => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_messages() {
        unsafe {
            let msg = kmb_error_message(KmbError::KmbOk);
            assert!(!msg.is_null());

            let msg = kmb_error_message(KmbError::KmbErrNullPointer);
            assert!(!msg.is_null());
        }
    }

    #[test]
    fn test_error_retryable() {
        unsafe {
            assert_eq!(kmb_error_is_retryable(KmbError::KmbOk), 0);
            assert_eq!(kmb_error_is_retryable(KmbError::KmbErrTimeout), 1);
            assert_eq!(
                kmb_error_is_retryable(KmbError::KmbErrClusterUnavailable),
                1
            );
            assert_eq!(kmb_error_is_retryable(KmbError::KmbErrNullPointer), 0);
        }
    }
}
