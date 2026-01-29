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

use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::slice;
use std::time::Duration;

use kmb_client::{Client, ClientConfig, ClientError};
use kmb_types::{DataClass, Offset, StreamId, TenantId};

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

/// Internal wrapper for the Rust client.
struct ClientWrapper {
    client: Client,
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

/// Result from read_events operation.
#[repr(C)]
pub struct KmbReadResult {
    /// Array of event data pointers
    pub events: *mut *mut u8,
    /// Parallel array of event lengths
    pub event_lengths: *mut usize,
    /// Number of events
    pub event_count: usize,
}

// Helper functions

/// Convert Rust ClientError to FFI error code
fn map_error(err: ClientError) -> KmbError {
    match err {
        ClientError::Connection(_) => KmbError::KmbErrConnectionFailed,
        ClientError::HandshakeFailed(_) => KmbError::KmbErrAuthFailed,
        ClientError::Timeout => KmbError::KmbErrTimeout,
        ClientError::Server { code, .. } => {
            use kmb_wire::ErrorCode;
            match code {
                ErrorCode::StreamNotFound => KmbError::KmbErrStreamNotFound,
                ErrorCode::TenantNotFound => KmbError::KmbErrTenantNotFound,
                ErrorCode::AuthenticationFailed => KmbError::KmbErrAuthFailed,
                ErrorCode::InvalidOffset => KmbError::KmbErrOffsetOutOfRange,
                ErrorCode::QueryParseError => KmbError::KmbErrQuerySyntax,
                ErrorCode::QueryExecutionError => KmbError::KmbErrQueryExecution,
                _ => KmbError::KmbErrInternal,
            }
        }
        ClientError::Wire(_) => KmbError::KmbErrInternal,
        ClientError::NotConnected => KmbError::KmbErrConnectionFailed,
        ClientError::ResponseMismatch { .. } => KmbError::KmbErrInternal,
        ClientError::UnexpectedResponse { .. } => KmbError::KmbErrInternal,
    }
}

/// Convert FFI data class to Rust DataClass
fn map_data_class(dc: KmbDataClass) -> Result<DataClass, KmbError> {
    match dc {
        KmbDataClass::KmbDataClassPhi => Ok(DataClass::PHI),
        KmbDataClass::KmbDataClassNonPhi => Ok(DataClass::NonPHI),
        KmbDataClass::KmbDataClassDeidentified => Ok(DataClass::Deidentified),
    }
}

// FFI functions

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

    let cfg = &*config;

    // Validate and extract address
    if cfg.address_count == 0 || cfg.addresses.is_null() {
        return KmbError::KmbErrNullPointer;
    }

    let addr_ptr = *cfg.addresses;
    if addr_ptr.is_null() {
        return KmbError::KmbErrNullPointer;
    }

    let addr = match CStr::from_ptr(addr_ptr).to_str() {
        Ok(s) => s,
        Err(_) => return KmbError::KmbErrInvalidUtf8,
    };

    // Extract auth token (optional)
    let auth_token = if !cfg.auth_token.is_null() {
        match CStr::from_ptr(cfg.auth_token).to_str() {
            Ok(s) if !s.is_empty() => Some(s.to_string()),
            Ok(_) => None,
            Err(_) => return KmbError::KmbErrInvalidUtf8,
        }
    } else {
        None
    };

    // Create client config
    let client_config = ClientConfig {
        read_timeout: Some(Duration::from_secs(30)),
        write_timeout: Some(Duration::from_secs(30)),
        buffer_size: 64 * 1024,
        auth_token,
    };

    // Connect
    let client = match Client::connect(addr, TenantId::new(cfg.tenant_id), client_config) {
        Ok(c) => c,
        Err(e) => return map_error(e),
    };

    // Box and cast to opaque pointer
    let wrapper = Box::new(ClientWrapper { client });
    *client_out = Box::into_raw(wrapper) as *mut KmbClient;

    KmbError::KmbOk
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

    // Convert back to Box and drop
    let _ = Box::from_raw(client as *mut ClientWrapper);
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

    // Extract name
    let name_str = match CStr::from_ptr(name).to_str() {
        Ok(s) => s,
        Err(_) => return KmbError::KmbErrInvalidUtf8,
    };

    // Convert data class
    let dc = match map_data_class(data_class) {
        Ok(d) => d,
        Err(e) => return e,
    };

    // Get mutable reference to client
    let wrapper = &mut *(client as *mut ClientWrapper);

    // Create stream
    match wrapper.client.create_stream(name_str, dc) {
        Ok(stream_id) => {
            *stream_id_out = stream_id.into();
            KmbError::KmbOk
        }
        Err(e) => map_error(e),
    }
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

    // Convert C arrays to Rust Vec
    let event_ptrs = slice::from_raw_parts(events, event_count);
    let lengths = slice::from_raw_parts(event_lengths, event_count);

    let mut rust_events = Vec::with_capacity(event_count);
    for i in 0..event_count {
        if event_ptrs[i].is_null() {
            return KmbError::KmbErrNullPointer;
        }
        let bytes = slice::from_raw_parts(event_ptrs[i], lengths[i]);
        rust_events.push(bytes.to_vec());
    }

    // Get mutable reference to client
    let wrapper = &mut *(client as *mut ClientWrapper);

    // Append events
    match wrapper
        .client
        .append(StreamId::from(stream_id), rust_events)
    {
        Ok(offset) => {
            *first_offset_out = offset.into();
            KmbError::KmbOk
        }
        Err(e) => map_error(e),
    }
}

/// Read events from a stream.
///
/// # Arguments
/// - `client`: Client handle
/// - `stream_id`: Stream ID
/// - `from_offset`: Starting offset
/// - `max_bytes`: Maximum bytes to read
/// - `result_out`: Output parameter for read result
///
/// # Returns
/// - `KMB_OK` on success
/// - Error code on failure
///
/// # Safety
/// - `client` must be valid
/// - `result_out` must be valid pointer
/// - Caller must call `kmb_read_result_free()` to free result
#[no_mangle]
pub unsafe extern "C" fn kmb_client_read_events(
    client: *mut KmbClient,
    stream_id: u64,
    from_offset: u64,
    max_bytes: u64,
    result_out: *mut *mut KmbReadResult,
) -> KmbError {
    if client.is_null() || result_out.is_null() {
        return KmbError::KmbErrNullPointer;
    }

    // Get mutable reference to client
    let wrapper = &mut *(client as *mut ClientWrapper);

    // Read events
    let response = match wrapper.client.read_events(
        StreamId::from(stream_id),
        Offset::new(from_offset),
        max_bytes,
    ) {
        Ok(r) => r,
        Err(e) => return map_error(e),
    };

    let event_count = response.events.len();

    // Allocate arrays
    let mut event_ptrs: Vec<*mut u8> = Vec::with_capacity(event_count);
    let mut event_lens: Vec<usize> = Vec::with_capacity(event_count);

    for event in response.events {
        let len = event.len();
        let mut boxed = event.into_boxed_slice();
        let ptr = boxed.as_mut_ptr();
        std::mem::forget(boxed); // Prevent drop, caller will free
        event_ptrs.push(ptr);
        event_lens.push(len);
    }

    // Create result struct
    let result = Box::new(KmbReadResult {
        events: event_ptrs.as_mut_ptr(),
        event_lengths: event_lens.as_mut_ptr(),
        event_count,
    });

    std::mem::forget(event_ptrs); // Prevent drop, caller will free
    std::mem::forget(event_lens);

    *result_out = Box::into_raw(result);
    KmbError::KmbOk
}

/// Free read result.
///
/// # Arguments
/// - `result`: Result from `kmb_client_read_events()`
///
/// # Safety
/// - `result` must be a valid result from `kmb_client_read_events()`
/// - After this call, `result` is invalid and must not be used
#[no_mangle]
pub unsafe extern "C" fn kmb_read_result_free(result: *mut KmbReadResult) {
    if result.is_null() {
        return;
    }

    let r = Box::from_raw(result);

    // Free event arrays
    if !r.events.is_null() {
        let event_ptrs = Vec::from_raw_parts(r.events, r.event_count, r.event_count);
        for ptr in event_ptrs {
            if !ptr.is_null() {
                // Reconstruct and drop the Vec
                let _ = Vec::from_raw_parts(ptr, 0, 0);
            }
        }
    }

    if !r.event_lengths.is_null() {
        let _ = Vec::from_raw_parts(r.event_lengths, r.event_count, r.event_count);
    }
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
        KmbError::KmbErrTimeout | KmbError::KmbErrClusterUnavailable | KmbError::KmbErrInternal => {
            1
        }
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
