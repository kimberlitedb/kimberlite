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
use std::slice;
use std::time::Duration;

use kimberlite_client::{Client, ClientConfig, ClientError, Pool, PoolConfig, PooledClient};
use kimberlite_types::{DataClass, Offset, Placement, Region, StreamId, TenantId};
use kimberlite_wire::{QueryParam, QueryResponse, QueryValue};

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
///
/// `#[repr(transparent)]` guarantees ABI identity with `Client`, which lets
/// `kmb_pooled_client_as_client` return a `*mut Client` cast as
/// `*mut KmbClient` and have the existing `kmb_client_*` handlers work on
/// it unchanged.
#[repr(transparent)]
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
///
/// Variants 0-2 are the original (Phi/NonPhi/Deidentified) ABI; variants 3-7
/// extend the enum to cover every `kimberlite_types::DataClass` value.
/// Old callers that only set 0/1/2 remain binary-compatible.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KmbDataClass {
    /// Protected Health Information (HIPAA-regulated)
    KmbDataClassPhi = 0,
    /// Non-PHI data (alias for Public).
    KmbDataClassNonPhi = 1,
    /// De-identified data
    KmbDataClassDeidentified = 2,
    /// Personally Identifiable Information (GDPR / CCPA)
    KmbDataClassPii = 3,
    /// Sensitive personal data (religion, health, sexual orientation, ...)
    KmbDataClassSensitive = 4,
    /// Payment Card Industry data (PCI DSS)
    KmbDataClassPci = 5,
    /// Financial records (SOX / GLBA)
    KmbDataClassFinancial = 6,
    /// Confidential business data
    KmbDataClassConfidential = 7,
    /// Public / unclassified data
    KmbDataClassPublic = 8,
}

/// Placement policy for a stream.
///
/// `KmbPlacementCustom` reads the placement name from the `custom_region`
/// argument of `kmb_client_create_stream_with_placement`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KmbPlacement {
    /// Global replication across all regions (default).
    KmbPlacementGlobal = 0,
    /// US East (N. Virginia) — us-east-1
    KmbPlacementUsEast1 = 1,
    /// Asia Pacific (Sydney) — ap-southeast-2
    KmbPlacementApSoutheast2 = 2,
    /// Custom region identifier (string supplied separately).
    KmbPlacementCustom = 3,
}

/// Result of `kmb_client_execute()` — analogous to a DML acknowledgement.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KmbExecuteResult {
    /// Number of rows inserted / updated / deleted.
    pub rows_affected: u64,
    /// Log offset at which the change was committed.
    pub log_offset: u64,
}

// ============================================================================
// Connection pool
// ============================================================================

/// Opaque handle to a connection pool.
///
/// Created with `kmb_pool_create()`, destroyed with `kmb_pool_destroy()`.
/// Pools are thread-safe; a single handle can be shared across threads.
#[repr(C)]
pub struct KmbPool {
    _private: [u8; 0],
}

/// Opaque handle to a pool-borrowed client. Dropping the handle returns the
/// connection to the pool. Free with `kmb_pool_release()` or pass the handle
/// to other `kmb_client_*` functions exactly like a regular `KmbClient`.
#[repr(C)]
pub struct KmbPooledClient {
    _private: [u8; 0],
}

/// Pool configuration.
///
/// `acquire_timeout_ms = 0` blocks forever.
/// `idle_timeout_ms = 0` disables idle eviction.
#[repr(C)]
pub struct KmbPoolConfig {
    /// Array of "host:port" strings (only the first is used).
    pub addresses: *const *const c_char,
    pub address_count: usize,
    /// Tenant ID
    pub tenant_id: u64,
    /// Authentication token (NULL-terminated, may be NULL or empty)
    pub auth_token: *const c_char,
    /// Maximum concurrent connections (must be > 0).
    pub max_size: usize,
    /// Milliseconds a caller will wait on `kmb_pool_acquire`; 0 = block forever.
    pub acquire_timeout_ms: u64,
    /// Milliseconds an idle connection stays in the pool before eviction; 0 = never.
    pub idle_timeout_ms: u64,
}

struct PoolWrapper {
    pool: Pool,
}

struct PooledClientWrapper {
    guard: PooledClient,
}

fn pool_client_error(err: ClientError) -> KmbError {
    map_error(err)
}

/// Create a connection pool.
///
/// # Safety
/// - `config` must point to a valid `KmbPoolConfig`
/// - `pool_out` must be non-null
/// - Call `kmb_pool_destroy` to free the returned handle
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmb_pool_create(
    config: *const KmbPoolConfig,
    pool_out: *mut *mut KmbPool,
) -> KmbError {
    unsafe {
        if config.is_null() || pool_out.is_null() {
            return KmbError::KmbErrNullPointer;
        }

        let cfg = &*config;
        if cfg.max_size == 0 || cfg.address_count == 0 || cfg.addresses.is_null() {
            return KmbError::KmbErrNullPointer;
        }

        let addr_ptr = *cfg.addresses;
        if addr_ptr.is_null() {
            return KmbError::KmbErrNullPointer;
        }
        let addr = match CStr::from_ptr(addr_ptr).to_str() {
            Ok(s) => s.to_string(),
            Err(_) => return KmbError::KmbErrInvalidUtf8,
        };

        let auth_token = if cfg.auth_token.is_null() {
            None
        } else {
            match CStr::from_ptr(cfg.auth_token).to_str() {
                Ok(s) if !s.is_empty() => Some(s.to_string()),
                Ok(_) => None,
                Err(_) => return KmbError::KmbErrInvalidUtf8,
            }
        };

        let client_config = ClientConfig {
            read_timeout: Some(Duration::from_secs(30)),
            write_timeout: Some(Duration::from_secs(30)),
            buffer_size: 64 * 1024,
            auth_token,
        };

        let pool_config = PoolConfig {
            max_size: cfg.max_size,
            acquire_timeout: if cfg.acquire_timeout_ms == 0 {
                None
            } else {
                Some(Duration::from_millis(cfg.acquire_timeout_ms))
            },
            idle_timeout: if cfg.idle_timeout_ms == 0 {
                None
            } else {
                Some(Duration::from_millis(cfg.idle_timeout_ms))
            },
            client_config,
        };

        let pool = match Pool::new(addr.as_str(), TenantId::new(cfg.tenant_id), pool_config) {
            Ok(p) => p,
            Err(e) => return pool_client_error(e),
        };

        let wrapper = Box::new(PoolWrapper { pool });
        *pool_out = Box::into_raw(wrapper) as *mut KmbPool;
        KmbError::KmbOk
    }
}

/// Acquire a client from the pool.
///
/// The returned `KmbPooledClient` can be passed to `kmb_client_*` functions
/// by casting via `kmb_pooled_client_as_client`. Release with
/// `kmb_pool_release` to return it to the pool; `kmb_pool_discard` closes
/// the underlying connection instead.
///
/// # Safety
/// - `pool` must be a valid handle from `kmb_pool_create`
/// - `client_out` must be non-null
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmb_pool_acquire(
    pool: *mut KmbPool,
    client_out: *mut *mut KmbPooledClient,
) -> KmbError {
    unsafe {
        if pool.is_null() || client_out.is_null() {
            return KmbError::KmbErrNullPointer;
        }
        let wrapper = &*(pool as *const PoolWrapper);
        match wrapper.pool.acquire() {
            Ok(guard) => {
                let boxed = Box::new(PooledClientWrapper { guard });
                *client_out = Box::into_raw(boxed) as *mut KmbPooledClient;
                KmbError::KmbOk
            }
            Err(e) => pool_client_error(e),
        }
    }
}

/// Return a pooled client to the pool.
///
/// # Safety
/// - `client` must be a valid handle from `kmb_pool_acquire`
/// - After this call the handle is invalid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmb_pool_release(client: *mut KmbPooledClient) {
    unsafe {
        if client.is_null() {
            return;
        }
        let _ = Box::from_raw(client as *mut PooledClientWrapper);
        // Drop runs PooledClient::drop which returns the connection to the pool.
    }
}

/// Discard a pooled client (drop the underlying TCP connection instead of
/// returning it to the pool). Use after an unrecoverable protocol error.
///
/// # Safety
/// - `client` must be a valid handle from `kmb_pool_acquire`
/// - After this call the handle is invalid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmb_pool_discard(client: *mut KmbPooledClient) {
    unsafe {
        if client.is_null() {
            return;
        }
        let boxed = Box::from_raw(client as *mut PooledClientWrapper);
        boxed.guard.discard();
    }
}

/// View a pooled-client handle as a regular `KmbClient` pointer.
///
/// The returned pointer remains valid until the `KmbPooledClient` is
/// released or discarded. Do NOT pass the returned pointer to
/// `kmb_client_disconnect` — it does not own the connection.
///
/// Internally the existing `kmb_client_*` functions interpret `*mut KmbClient`
/// as `*mut ClientWrapper` where `ClientWrapper` has a single `Client` field
/// at offset 0, so a `*mut Client` is ABI-compatible with `*mut KmbClient`.
///
/// # Safety
/// - `client` must be a valid handle from `kmb_pool_acquire`
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmb_pooled_client_as_client(
    client: *mut KmbPooledClient,
) -> *mut KmbClient {
    unsafe {
        if client.is_null() {
            return std::ptr::null_mut();
        }
        let wrapper = &mut *(client as *mut PooledClientWrapper);
        // Deref the PooledClient guard to &mut Client, then cast to
        // *mut KmbClient. Caller must keep the pooled handle alive while
        // the returned pointer is in use.
        let c: &mut Client = &mut wrapper.guard;
        std::ptr::from_mut(c).cast::<KmbClient>()
    }
}

/// Current pool statistics. Writes `max_size`, `open`, `idle`, `in_use`,
/// and `shutdown` (0/1) via out-parameters. Any out-pointer may be NULL
/// to skip that field.
///
/// # Safety
/// - `pool` must be a valid handle from `kmb_pool_create`
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmb_pool_stats(
    pool: *mut KmbPool,
    max_size_out: *mut usize,
    open_out: *mut usize,
    idle_out: *mut usize,
    in_use_out: *mut usize,
    shutdown_out: *mut c_int,
) -> KmbError {
    unsafe {
        if pool.is_null() {
            return KmbError::KmbErrNullPointer;
        }
        let wrapper = &*(pool as *const PoolWrapper);
        let stats = wrapper.pool.stats();
        if !max_size_out.is_null() {
            *max_size_out = stats.max_size;
        }
        if !open_out.is_null() {
            *open_out = stats.open;
        }
        if !idle_out.is_null() {
            *idle_out = stats.idle;
        }
        if !in_use_out.is_null() {
            *in_use_out = stats.in_use;
        }
        if !shutdown_out.is_null() {
            *shutdown_out = c_int::from(stats.shutdown);
        }
        KmbError::KmbOk
    }
}

/// Shut down a pool. Idle connections close immediately; in-flight clients
/// close when released. Subsequent acquires fail with
/// `KMB_ERR_CONNECTION_FAILED`.
///
/// # Safety
/// - `pool` must be a valid handle from `kmb_pool_create`
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmb_pool_shutdown(pool: *mut KmbPool) {
    unsafe {
        if pool.is_null() {
            return;
        }
        let wrapper = &*(pool as *const PoolWrapper);
        wrapper.pool.shutdown();
    }
}

/// Destroy a pool, freeing all remaining resources. Implicitly calls shutdown.
///
/// # Safety
/// - `pool` must be a valid handle from `kmb_pool_create`
/// - After this call the handle is invalid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmb_pool_destroy(pool: *mut KmbPool) {
    unsafe {
        if pool.is_null() {
            return;
        }
        let wrapper = Box::from_raw(pool as *mut PoolWrapper);
        wrapper.pool.shutdown();
        // Dropping the PoolWrapper drops the Arc — when the last clone is
        // gone, idle connections are dropped too.
    }
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

/// Query parameter type.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KmbQueryParamType {
    /// Null value
    KmbParamNull = 0,
    /// 64-bit integer
    KmbParamBigInt = 1,
    /// Text string
    KmbParamText = 2,
    /// Boolean
    KmbParamBoolean = 3,
    /// Timestamp (nanoseconds since epoch)
    KmbParamTimestamp = 4,
}

/// Query parameter value (input to query).
#[repr(C)]
pub struct KmbQueryParam {
    /// Parameter type
    pub param_type: KmbQueryParamType,
    /// BigInt value (used when param_type == KmbParamBigInt)
    pub bigint_val: i64,
    /// Text value (NULL-terminated, used when param_type == KmbParamText)
    pub text_val: *const c_char,
    /// Boolean value (used when param_type == KmbParamBoolean)
    pub bool_val: c_int,
    /// Timestamp value (used when param_type == KmbParamTimestamp)
    pub timestamp_val: i64,
}

/// Query value type (output from query).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KmbQueryValueType {
    /// Null value
    KmbValueNull = 0,
    /// 64-bit integer
    KmbValueBigInt = 1,
    /// Text string
    KmbValueText = 2,
    /// Boolean
    KmbValueBoolean = 3,
    /// Timestamp (nanoseconds since epoch)
    KmbValueTimestamp = 4,
}

/// Query value (output from query).
#[repr(C)]
pub struct KmbQueryValue {
    /// Value type
    pub value_type: KmbQueryValueType,
    /// BigInt value (used when value_type == KmbValueBigInt)
    pub bigint_val: i64,
    /// Text value (NULL-terminated, owned by result, used when value_type == KmbValueText)
    pub text_val: *mut c_char,
    /// Boolean value (used when value_type == KmbValueBoolean)
    pub bool_val: c_int,
    /// Timestamp value (used when value_type == KmbValueTimestamp)
    pub timestamp_val: i64,
}

/// Query result (2D array of values).
#[repr(C)]
pub struct KmbQueryResult {
    /// Array of column names (NULL-terminated C strings, owned by result)
    pub columns: *mut *mut c_char,
    /// Number of columns
    pub column_count: usize,
    /// Array of rows (each row is an array of KmbQueryValue)
    pub rows: *mut *mut KmbQueryValue,
    /// Array of row lengths (number of values in each row)
    pub row_lengths: *mut usize,
    /// Number of rows
    pub row_count: usize,
}

// Helper functions

/// Convert Rust ClientError to FFI error code
fn map_error(err: ClientError) -> KmbError {
    match err {
        ClientError::Connection(_) => KmbError::KmbErrConnectionFailed,
        ClientError::HandshakeFailed(_) => KmbError::KmbErrAuthFailed,
        ClientError::Timeout => KmbError::KmbErrTimeout,
        ClientError::Server { code, .. } => {
            use kimberlite_wire::ErrorCode;
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
        KmbDataClass::KmbDataClassNonPhi => Ok(DataClass::Public),
        KmbDataClass::KmbDataClassDeidentified => Ok(DataClass::Deidentified),
        KmbDataClass::KmbDataClassPii => Ok(DataClass::PII),
        KmbDataClass::KmbDataClassSensitive => Ok(DataClass::Sensitive),
        KmbDataClass::KmbDataClassPci => Ok(DataClass::PCI),
        KmbDataClass::KmbDataClassFinancial => Ok(DataClass::Financial),
        KmbDataClass::KmbDataClassConfidential => Ok(DataClass::Confidential),
        KmbDataClass::KmbDataClassPublic => Ok(DataClass::Public),
    }
}

/// Convert FFI placement to Rust Placement.
///
/// Returns `KmbErrNullPointer` if `placement` is `KmbPlacementCustom` and
/// `custom_region` is null or not valid UTF-8.
unsafe fn map_placement(
    placement: KmbPlacement,
    custom_region: *const c_char,
) -> Result<Placement, KmbError> {
    match placement {
        KmbPlacement::KmbPlacementGlobal => Ok(Placement::Global),
        KmbPlacement::KmbPlacementUsEast1 => Ok(Placement::Region(Region::USEast1)),
        KmbPlacement::KmbPlacementApSoutheast2 => Ok(Placement::Region(Region::APSoutheast2)),
        KmbPlacement::KmbPlacementCustom => {
            if custom_region.is_null() {
                return Err(KmbError::KmbErrNullPointer);
            }
            let name = match unsafe { CStr::from_ptr(custom_region) }.to_str() {
                Ok(s) => s.to_string(),
                Err(_) => return Err(KmbError::KmbErrInvalidUtf8),
            };
            Ok(Placement::Region(Region::custom(name)))
        }
    }
}

/// Convert FFI query parameter to Rust QueryParam
unsafe fn convert_query_param(param: &KmbQueryParam) -> Result<QueryParam, KmbError> {
    unsafe {
        match param.param_type {
            KmbQueryParamType::KmbParamNull => Ok(QueryParam::Null),
            KmbQueryParamType::KmbParamBigInt => Ok(QueryParam::BigInt(param.bigint_val)),
            KmbQueryParamType::KmbParamText => {
                if param.text_val.is_null() {
                    return Err(KmbError::KmbErrNullPointer);
                }
                let text = match CStr::from_ptr(param.text_val).to_str() {
                    Ok(s) => s.to_string(),
                    Err(_) => return Err(KmbError::KmbErrInvalidUtf8),
                };
                Ok(QueryParam::Text(text))
            }
            KmbQueryParamType::KmbParamBoolean => Ok(QueryParam::Boolean(param.bool_val != 0)),
            KmbQueryParamType::KmbParamTimestamp => Ok(QueryParam::Timestamp(param.timestamp_val)),
        }
    }
}

/// Convert Rust QueryValue to FFI KmbQueryValue
unsafe fn convert_query_value(value: QueryValue) -> Result<KmbQueryValue, KmbError> {
    match value {
        QueryValue::Null => Ok(KmbQueryValue {
            value_type: KmbQueryValueType::KmbValueNull,
            bigint_val: 0,
            text_val: std::ptr::null_mut(),
            bool_val: 0,
            timestamp_val: 0,
        }),
        QueryValue::BigInt(v) => Ok(KmbQueryValue {
            value_type: KmbQueryValueType::KmbValueBigInt,
            bigint_val: v,
            text_val: std::ptr::null_mut(),
            bool_val: 0,
            timestamp_val: 0,
        }),
        QueryValue::Text(s) => {
            let c_string = CString::new(s).map_err(|_| KmbError::KmbErrInvalidUtf8)?;
            Ok(KmbQueryValue {
                value_type: KmbQueryValueType::KmbValueText,
                bigint_val: 0,
                text_val: c_string.into_raw(),
                bool_val: 0,
                timestamp_val: 0,
            })
        }
        QueryValue::Boolean(v) => Ok(KmbQueryValue {
            value_type: KmbQueryValueType::KmbValueBoolean,
            bigint_val: 0,
            text_val: std::ptr::null_mut(),
            bool_val: if v { 1 } else { 0 },
            timestamp_val: 0,
        }),
        QueryValue::Timestamp(v) => Ok(KmbQueryValue {
            value_type: KmbQueryValueType::KmbValueTimestamp,
            bigint_val: 0,
            text_val: std::ptr::null_mut(),
            bool_val: 0,
            timestamp_val: v,
        }),
    }
}

/// Convert Rust QueryResponse to FFI KmbQueryResult
unsafe fn convert_query_response(response: QueryResponse) -> Result<KmbQueryResult, KmbError> {
    unsafe {
        let column_count = response.columns.len();
        let row_count = response.rows.len();

        // Allocate column names — clean up already-allocated pointers if any fail.
        let mut column_ptrs: Vec<*mut c_char> = Vec::with_capacity(column_count);
        for col_name in response.columns {
            match CString::new(col_name) {
                Ok(c_string) => column_ptrs.push(c_string.into_raw()),
                Err(_) => {
                    // Free every CString we already handed out before propagating the error.
                    for ptr in column_ptrs {
                        let _ = CString::from_raw(ptr);
                    }
                    return Err(KmbError::KmbErrInvalidUtf8);
                }
            }
        }

        // Allocate rows
        let mut row_ptrs: Vec<*mut KmbQueryValue> = Vec::with_capacity(row_count);
        let mut row_lens: Vec<usize> = Vec::with_capacity(row_count);

        for row in response.rows {
            let row_len = row.len();
            let mut row_values: Vec<KmbQueryValue> = Vec::with_capacity(row_len);

            for value in row {
                row_values.push(convert_query_value(value)?);
            }

            let row_ptr = row_values.as_mut_ptr();
            std::mem::forget(row_values); // Prevent drop, caller will free

            row_ptrs.push(row_ptr);
            row_lens.push(row_len);
        }

        let result = KmbQueryResult {
            columns: column_ptrs.as_mut_ptr(),
            column_count,
            rows: row_ptrs.as_mut_ptr(),
            row_lengths: row_lens.as_mut_ptr(),
            row_count,
        };

        std::mem::forget(column_ptrs); // Prevent drop
        std::mem::forget(row_ptrs); // Prevent drop
        std::mem::forget(row_lens); // Prevent drop

        Ok(result)
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmb_client_connect(
    config: *const KmbClientConfig,
    client_out: *mut *mut KmbClient,
) -> KmbError {
    unsafe {
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
}

/// Disconnect from cluster and free client.
///
/// # Arguments
/// - `client`: Client handle from `kmb_client_connect()`
///
/// # Safety
/// - `client` must be a valid handle from `kmb_client_connect()`
/// - After this call, `client` is invalid and must not be used
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmb_client_disconnect(client: *mut KmbClient) {
    unsafe {
        if client.is_null() {
            return;
        }

        // Convert back to Box and drop
        let _ = Box::from_raw(client as *mut ClientWrapper);
    }
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmb_client_create_stream(
    client: *mut KmbClient,
    name: *const c_char,
    data_class: KmbDataClass,
    stream_id_out: *mut u64,
) -> KmbError {
    unsafe {
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
}

/// Create a new stream with a specific placement policy.
///
/// # Arguments
/// - `client`: Client handle
/// - `name`: Stream name (NULL-terminated UTF-8)
/// - `data_class`: Data classification
/// - `placement`: Geographic placement policy
/// - `custom_region`: Custom region identifier (only read when
///   `placement == KmbPlacementCustom`; may be NULL otherwise)
/// - `stream_id_out`: Output parameter for stream ID
///
/// # Returns
/// - `KMB_OK` on success
/// - Error code on failure
///
/// # Safety
/// - `client` must be valid
/// - `name` must be valid NULL-terminated UTF-8 string
/// - If `placement == KmbPlacementCustom`, `custom_region` must be valid
///   NULL-terminated UTF-8
/// - `stream_id_out` must be valid pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmb_client_create_stream_with_placement(
    client: *mut KmbClient,
    name: *const c_char,
    data_class: KmbDataClass,
    placement: KmbPlacement,
    custom_region: *const c_char,
    stream_id_out: *mut u64,
) -> KmbError {
    unsafe {
        if client.is_null() || name.is_null() || stream_id_out.is_null() {
            return KmbError::KmbErrNullPointer;
        }

        let name_str = match CStr::from_ptr(name).to_str() {
            Ok(s) => s,
            Err(_) => return KmbError::KmbErrInvalidUtf8,
        };

        let dc = match map_data_class(data_class) {
            Ok(d) => d,
            Err(e) => return e,
        };

        let p = match map_placement(placement, custom_region) {
            Ok(p) => p,
            Err(e) => return e,
        };

        let wrapper = &mut *(client as *mut ClientWrapper);

        match wrapper
            .client
            .create_stream_with_placement(name_str, dc, p)
        {
            Ok(stream_id) => {
                *stream_id_out = stream_id.into();
                KmbError::KmbOk
            }
            Err(e) => map_error(e),
        }
    }
}

/// Returns the tenant ID this client is connected as.
///
/// # Safety
/// - `client` must be a valid handle
/// - `tenant_id_out` must be a valid pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmb_client_tenant_id(
    client: *mut KmbClient,
    tenant_id_out: *mut u64,
) -> KmbError {
    unsafe {
        if client.is_null() || tenant_id_out.is_null() {
            return KmbError::KmbErrNullPointer;
        }
        let wrapper = &*(client as *const ClientWrapper);
        *tenant_id_out = wrapper.client.tenant_id().into();
        KmbError::KmbOk
    }
}

/// Returns the wire request ID of the most recently sent request.
///
/// Useful for correlating client-side logs with server-side tracing output.
/// Writes `0` if no request has been sent yet.
///
/// # Safety
/// - `client` must be a valid handle
/// - `request_id_out` must be a valid pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmb_client_last_request_id(
    client: *mut KmbClient,
    request_id_out: *mut u64,
) -> KmbError {
    unsafe {
        if client.is_null() || request_id_out.is_null() {
            return KmbError::KmbErrNullPointer;
        }
        let wrapper = &*(client as *const ClientWrapper);
        *request_id_out = wrapper.client.last_request_id().unwrap_or(0);
        KmbError::KmbOk
    }
}

/// Execute a DML or DDL statement (INSERT / UPDATE / DELETE / CREATE / ALTER).
///
/// # Arguments
/// - `client`: Client handle
/// - `sql`: SQL statement (NULL-terminated UTF-8)
/// - `params`: Array of query parameters (may be NULL if `param_count == 0`)
/// - `param_count`: Number of parameters
/// - `result_out`: Output parameter for rows-affected and log offset
///
/// # Safety
/// - `client` must be valid
/// - `sql` must be valid NULL-terminated UTF-8 string
/// - `params` must be array of `param_count` valid parameters (or NULL if
///   `param_count == 0`)
/// - `result_out` must be valid pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmb_client_execute(
    client: *mut KmbClient,
    sql: *const c_char,
    params: *const KmbQueryParam,
    param_count: usize,
    result_out: *mut KmbExecuteResult,
) -> KmbError {
    unsafe {
        if client.is_null() || sql.is_null() || result_out.is_null() {
            return KmbError::KmbErrNullPointer;
        }
        if param_count > 0 && params.is_null() {
            return KmbError::KmbErrNullPointer;
        }

        let sql_str = match CStr::from_ptr(sql).to_str() {
            Ok(s) => s,
            Err(_) => return KmbError::KmbErrInvalidUtf8,
        };

        let mut rust_params = Vec::with_capacity(param_count);
        if param_count > 0 {
            let param_slice = slice::from_raw_parts(params, param_count);
            for param in param_slice {
                match convert_query_param(param) {
                    Ok(p) => rust_params.push(p),
                    Err(e) => return e,
                }
            }
        }

        let wrapper = &mut *(client as *mut ClientWrapper);

        match wrapper.client.execute(sql_str, &rust_params) {
            Ok((rows, offset)) => {
                *result_out = KmbExecuteResult {
                    rows_affected: rows,
                    log_offset: offset,
                };
                KmbError::KmbOk
            }
            Err(e) => map_error(e),
        }
    }
}

/// Append events to a stream.
///
/// # Arguments
/// - `client`: Client handle
/// - `stream_id`: Stream ID
/// - `expected_offset`: Expected current stream offset (optimistic concurrency)
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmb_client_append(
    client: *mut KmbClient,
    stream_id: u64,
    expected_offset: u64,
    events: *const *const u8,
    event_lengths: *const usize,
    event_count: usize,
    first_offset_out: *mut u64,
) -> KmbError {
    unsafe {
        if client.is_null()
            || events.is_null()
            || event_lengths.is_null()
            || first_offset_out.is_null()
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
        match wrapper.client.append(
            StreamId::from(stream_id),
            rust_events,
            Offset::from(expected_offset),
        ) {
            Ok(offset) => {
                *first_offset_out = offset.into();
                KmbError::KmbOk
            }
            Err(e) => map_error(e),
        }
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmb_client_read_events(
    client: *mut KmbClient,
    stream_id: u64,
    from_offset: u64,
    max_bytes: u64,
    result_out: *mut *mut KmbReadResult,
) -> KmbError {
    unsafe {
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
}

/// Free read result.
///
/// # Arguments
/// - `result`: Result from `kmb_client_read_events()`
///
/// # Safety
/// - `result` must be a valid result from `kmb_client_read_events()`
/// - After this call, `result` is invalid and must not be used
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmb_read_result_free(result: *mut KmbReadResult) {
    unsafe {
        if result.is_null() {
            return;
        }

        let r = Box::from_raw(result);

        // Free event arrays — must use correct lengths since buffers were allocated
        // via into_boxed_slice() with their true length as capacity.
        if !r.events.is_null() && !r.event_lengths.is_null() {
            // Reconstruct both arrays so we can pair each pointer with its length.
            let event_lens = Vec::from_raw_parts(r.event_lengths, r.event_count, r.event_count);
            let event_ptrs = Vec::from_raw_parts(r.events, r.event_count, r.event_count);
            for (ptr, len) in event_ptrs.iter().zip(event_lens.iter()) {
                if !ptr.is_null() {
                    // Reconstruct boxed slice with the correct length/capacity to free it.
                    let _ = Vec::from_raw_parts(*ptr, *len, *len);
                }
            }
            // event_ptrs and event_lens are dropped here, freeing both arrays.
        } else {
            // Handle the degenerate case where only one pointer is non-null.
            if !r.events.is_null() {
                let _ = Vec::from_raw_parts(r.events, r.event_count, r.event_count);
            }
            if !r.event_lengths.is_null() {
                let _ = Vec::from_raw_parts(r.event_lengths, r.event_count, r.event_count);
            }
        }
    }
}

/// Execute a SQL query against current state.
///
/// # Arguments
/// - `client`: Client handle
/// - `sql`: SQL query string (NULL-terminated UTF-8)
/// - `params`: Array of query parameters (may be NULL if param_count == 0)
/// - `param_count`: Number of parameters
/// - `result_out`: Output parameter for query result
///
/// # Returns
/// - `KMB_OK` on success
/// - Error code on failure
///
/// # Safety
/// - `client` must be valid
/// - `sql` must be valid NULL-terminated UTF-8 string
/// - `params` must be array of `param_count` valid parameters (or NULL if param_count == 0)
/// - `result_out` must be valid pointer
/// - Caller must call `kmb_query_result_free()` to free result
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmb_client_query(
    client: *mut KmbClient,
    sql: *const c_char,
    params: *const KmbQueryParam,
    param_count: usize,
    result_out: *mut *mut KmbQueryResult,
) -> KmbError {
    unsafe {
        if client.is_null() || sql.is_null() || result_out.is_null() {
            return KmbError::KmbErrNullPointer;
        }

        if param_count > 0 && params.is_null() {
            return KmbError::KmbErrNullPointer;
        }

        // Extract SQL string
        let sql_str = match CStr::from_ptr(sql).to_str() {
            Ok(s) => s,
            Err(_) => return KmbError::KmbErrInvalidUtf8,
        };

        // Convert parameters
        let mut rust_params = Vec::with_capacity(param_count);
        if param_count > 0 {
            let param_slice = slice::from_raw_parts(params, param_count);
            for param in param_slice {
                match convert_query_param(param) {
                    Ok(p) => rust_params.push(p),
                    Err(e) => return e,
                }
            }
        }

        // Get mutable reference to client
        let wrapper = &mut *(client as *mut ClientWrapper);

        // Execute query
        let response = match wrapper.client.query(sql_str, &rust_params) {
            Ok(r) => r,
            Err(e) => return map_error(e),
        };

        // Convert response to FFI format
        let ffi_result = match convert_query_response(response) {
            Ok(r) => r,
            Err(e) => return e,
        };

        *result_out = Box::into_raw(Box::new(ffi_result));
        KmbError::KmbOk
    }
}

/// Execute a SQL query at a specific log position (point-in-time query).
///
/// # Arguments
/// - `client`: Client handle
/// - `sql`: SQL query string (NULL-terminated UTF-8)
/// - `params`: Array of query parameters (may be NULL if param_count == 0)
/// - `param_count`: Number of parameters
/// - `position`: Log position (offset) to query at
/// - `result_out`: Output parameter for query result
///
/// # Returns
/// - `KMB_OK` on success
/// - Error code on failure
///
/// # Safety
/// - `client` must be valid
/// - `sql` must be valid NULL-terminated UTF-8 string
/// - `params` must be array of `param_count` valid parameters (or NULL if param_count == 0)
/// - `result_out` must be valid pointer
/// - Caller must call `kmb_query_result_free()` to free result
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmb_client_query_at(
    client: *mut KmbClient,
    sql: *const c_char,
    params: *const KmbQueryParam,
    param_count: usize,
    position: u64,
    result_out: *mut *mut KmbQueryResult,
) -> KmbError {
    unsafe {
        if client.is_null() || sql.is_null() || result_out.is_null() {
            return KmbError::KmbErrNullPointer;
        }

        if param_count > 0 && params.is_null() {
            return KmbError::KmbErrNullPointer;
        }

        // Extract SQL string
        let sql_str = match CStr::from_ptr(sql).to_str() {
            Ok(s) => s,
            Err(_) => return KmbError::KmbErrInvalidUtf8,
        };

        // Convert parameters
        let mut rust_params = Vec::with_capacity(param_count);
        if param_count > 0 {
            let param_slice = slice::from_raw_parts(params, param_count);
            for param in param_slice {
                match convert_query_param(param) {
                    Ok(p) => rust_params.push(p),
                    Err(e) => return e,
                }
            }
        }

        // Get mutable reference to client
        let wrapper = &mut *(client as *mut ClientWrapper);

        // Execute query at position
        let response = match wrapper
            .client
            .query_at(sql_str, &rust_params, Offset::new(position))
        {
            Ok(r) => r,
            Err(e) => return map_error(e),
        };

        // Convert response to FFI format
        let ffi_result = match convert_query_response(response) {
            Ok(r) => r,
            Err(e) => return e,
        };

        *result_out = Box::into_raw(Box::new(ffi_result));
        KmbError::KmbOk
    }
}

/// Free query result.
///
/// # Arguments
/// - `result`: Result from `kmb_client_query()` or `kmb_client_query_at()`
///
/// # Safety
/// - `result` must be a valid result from `kmb_client_query()` or `kmb_client_query_at()`
/// - After this call, `result` is invalid and must not be used
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmb_query_result_free(result: *mut KmbQueryResult) {
    unsafe {
        if result.is_null() {
            return;
        }

        let r = Box::from_raw(result);

        // Free column names
        if !r.columns.is_null() {
            let column_ptrs = Vec::from_raw_parts(r.columns, r.column_count, r.column_count);
            for ptr in column_ptrs {
                if !ptr.is_null() {
                    let _ = CString::from_raw(ptr);
                }
            }
        }

        // Free rows
        if !r.rows.is_null() {
            let row_ptrs = Vec::from_raw_parts(r.rows, r.row_count, r.row_count);
            let row_lens = if !r.row_lengths.is_null() {
                Vec::from_raw_parts(r.row_lengths, r.row_count, r.row_count)
            } else {
                vec![0; r.row_count]
            };

            for (row_ptr, row_len) in row_ptrs.into_iter().zip(row_lens.iter()) {
                if !row_ptr.is_null() {
                    let row_values = Vec::from_raw_parts(row_ptr, *row_len, *row_len);
                    // Free text values in each cell
                    for value in row_values {
                        if value.value_type == KmbQueryValueType::KmbValueText
                            && !value.text_val.is_null()
                        {
                            let _ = CString::from_raw(value.text_val);
                        }
                    }
                }
            }
        }
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
    use std::ffi::CString;

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

    #[test]
    fn test_convert_query_param_null() {
        unsafe {
            let param = KmbQueryParam {
                param_type: KmbQueryParamType::KmbParamNull,
                bigint_val: 0,
                text_val: std::ptr::null(),
                bool_val: 0,
                timestamp_val: 0,
            };
            let result = convert_query_param(&param).unwrap();
            assert!(matches!(result, QueryParam::Null));
        }
    }

    #[test]
    fn test_convert_query_param_bigint() {
        unsafe {
            let param = KmbQueryParam {
                param_type: KmbQueryParamType::KmbParamBigInt,
                bigint_val: 42,
                text_val: std::ptr::null(),
                bool_val: 0,
                timestamp_val: 0,
            };
            let result = convert_query_param(&param).unwrap();
            assert!(matches!(result, QueryParam::BigInt(42)));
        }
    }

    #[test]
    fn test_convert_query_param_text() {
        unsafe {
            let text = CString::new("hello").unwrap();
            let param = KmbQueryParam {
                param_type: KmbQueryParamType::KmbParamText,
                bigint_val: 0,
                text_val: text.as_ptr(),
                bool_val: 0,
                timestamp_val: 0,
            };
            let result = convert_query_param(&param).unwrap();
            if let QueryParam::Text(s) = result {
                assert_eq!(s, "hello");
            } else {
                panic!("expected text param");
            }
        }
    }

    #[test]
    fn test_convert_query_param_boolean() {
        unsafe {
            let param = KmbQueryParam {
                param_type: KmbQueryParamType::KmbParamBoolean,
                bigint_val: 0,
                text_val: std::ptr::null(),
                bool_val: 1,
                timestamp_val: 0,
            };
            let result = convert_query_param(&param).unwrap();
            assert!(matches!(result, QueryParam::Boolean(true)));
        }
    }

    #[test]
    fn test_convert_query_param_timestamp() {
        unsafe {
            let param = KmbQueryParam {
                param_type: KmbQueryParamType::KmbParamTimestamp,
                bigint_val: 0,
                text_val: std::ptr::null(),
                bool_val: 0,
                timestamp_val: 1234567890,
            };
            let result = convert_query_param(&param).unwrap();
            assert!(matches!(result, QueryParam::Timestamp(1234567890)));
        }
    }

    #[test]
    fn test_convert_query_value_null() {
        unsafe {
            let value = QueryValue::Null;
            let result = convert_query_value(value).unwrap();
            assert_eq!(result.value_type, KmbQueryValueType::KmbValueNull);
        }
    }

    #[test]
    fn test_convert_query_value_bigint() {
        unsafe {
            let value = QueryValue::BigInt(42);
            let result = convert_query_value(value).unwrap();
            assert_eq!(result.value_type, KmbQueryValueType::KmbValueBigInt);
            assert_eq!(result.bigint_val, 42);
        }
    }

    #[test]
    fn test_convert_query_value_text() {
        unsafe {
            let value = QueryValue::Text("world".to_string());
            let result = convert_query_value(value).unwrap();
            assert_eq!(result.value_type, KmbQueryValueType::KmbValueText);
            assert!(!result.text_val.is_null());
            let text = CStr::from_ptr(result.text_val).to_str().unwrap();
            assert_eq!(text, "world");
            // Clean up
            let _ = CString::from_raw(result.text_val);
        }
    }

    #[test]
    fn test_convert_query_value_boolean() {
        unsafe {
            let value = QueryValue::Boolean(true);
            let result = convert_query_value(value).unwrap();
            assert_eq!(result.value_type, KmbQueryValueType::KmbValueBoolean);
            assert_eq!(result.bool_val, 1);
        }
    }

    #[test]
    fn test_convert_query_value_timestamp() {
        unsafe {
            let value = QueryValue::Timestamp(9876543210);
            let result = convert_query_value(value).unwrap();
            assert_eq!(result.value_type, KmbQueryValueType::KmbValueTimestamp);
            assert_eq!(result.timestamp_val, 9876543210);
        }
    }

    // ========================================================================
    // NULL Pointer Validation Tests
    // ========================================================================

    #[test]
    fn test_connect_null_config() {
        unsafe {
            let mut client_out: *mut KmbClient = std::ptr::null_mut();
            let result = kmb_client_connect(std::ptr::null(), &mut client_out);
            assert_eq!(result, KmbError::KmbErrNullPointer);
        }
    }

    #[test]
    fn test_connect_null_client_out() {
        unsafe {
            let addr = CString::new("localhost:5000").unwrap();
            let addr_ptr = addr.as_ptr();
            let client_name = CString::new("test").unwrap();
            let client_version = CString::new("0.1.0").unwrap();
            let config = KmbClientConfig {
                addresses: &addr_ptr,
                address_count: 1,
                tenant_id: 1,
                auth_token: std::ptr::null(),
                client_name: client_name.as_ptr(),
                client_version: client_version.as_ptr(),
            };

            let result = kmb_client_connect(&config, std::ptr::null_mut());
            assert_eq!(result, KmbError::KmbErrNullPointer);
        }
    }

    #[test]
    fn test_connect_null_addresses() {
        unsafe {
            let mut client_out: *mut KmbClient = std::ptr::null_mut();
            let client_name = CString::new("test").unwrap();
            let client_version = CString::new("0.1.0").unwrap();
            let config = KmbClientConfig {
                addresses: std::ptr::null(),
                address_count: 1,
                tenant_id: 1,
                auth_token: std::ptr::null(),
                client_name: client_name.as_ptr(),
                client_version: client_version.as_ptr(),
            };

            let result = kmb_client_connect(&config, &mut client_out);
            assert_eq!(result, KmbError::KmbErrNullPointer);
        }
    }

    #[test]
    fn test_connect_zero_address_count() {
        unsafe {
            let mut client_out: *mut KmbClient = std::ptr::null_mut();
            let addr = CString::new("localhost:5000").unwrap();
            let addr_ptr = addr.as_ptr();
            let client_name = CString::new("test").unwrap();
            let client_version = CString::new("0.1.0").unwrap();
            let config = KmbClientConfig {
                addresses: &addr_ptr,
                address_count: 0,
                tenant_id: 1,
                auth_token: std::ptr::null(),
                client_name: client_name.as_ptr(),
                client_version: client_version.as_ptr(),
            };

            let result = kmb_client_connect(&config, &mut client_out);
            assert_eq!(result, KmbError::KmbErrNullPointer);
        }
    }

    #[test]
    fn test_disconnect_null_client() {
        unsafe {
            // Should not crash
            kmb_client_disconnect(std::ptr::null_mut());
        }
    }

    #[test]
    fn test_read_result_free_null() {
        unsafe {
            // Should not crash
            kmb_read_result_free(std::ptr::null_mut());
        }
    }

    #[test]
    fn test_query_result_free_null() {
        unsafe {
            // Should not crash
            kmb_query_result_free(std::ptr::null_mut());
        }
    }

    // ========================================================================
    // UTF-8 Validation Tests
    // ========================================================================

    #[test]
    fn test_connect_invalid_utf8_address() {
        unsafe {
            let mut client_out: *mut KmbClient = std::ptr::null_mut();

            // Create invalid UTF-8 sequence
            let invalid_bytes = [0xC3, 0x28, 0x00]; // Invalid UTF-8
            let addr_ptr = invalid_bytes.as_ptr() as *const c_char;
            let client_name = CString::new("test").unwrap();
            let client_version = CString::new("0.1.0").unwrap();

            let config = KmbClientConfig {
                addresses: &addr_ptr,
                address_count: 1,
                tenant_id: 1,
                auth_token: std::ptr::null(),
                client_name: client_name.as_ptr(),
                client_version: client_version.as_ptr(),
            };

            let result = kmb_client_connect(&config, &mut client_out);
            assert_eq!(result, KmbError::KmbErrInvalidUtf8);
        }
    }

    #[test]
    fn test_convert_query_param_text_null_pointer() {
        unsafe {
            let param = KmbQueryParam {
                param_type: KmbQueryParamType::KmbParamText,
                bigint_val: 0,
                text_val: std::ptr::null(),
                bool_val: 0,
                timestamp_val: 0,
            };
            let result = convert_query_param(&param);
            assert!(matches!(result, Err(KmbError::KmbErrNullPointer)));
        }
    }

    #[test]
    fn test_convert_query_param_text_invalid_utf8() {
        unsafe {
            // Create invalid UTF-8 sequence
            let invalid_bytes = [0xFF, 0xFE, 0xFD, 0x00];
            let param = KmbQueryParam {
                param_type: KmbQueryParamType::KmbParamText,
                bigint_val: 0,
                text_val: invalid_bytes.as_ptr() as *const c_char,
                bool_val: 0,
                timestamp_val: 0,
            };
            let result = convert_query_param(&param);
            assert!(matches!(result, Err(KmbError::KmbErrInvalidUtf8)));
        }
    }

    // ========================================================================
    // Data Class Conversion Tests
    // ========================================================================

    use test_case::test_case;

    #[test_case(KmbDataClass::KmbDataClassPhi, DataClass::PHI; "PHI")]
    #[test_case(KmbDataClass::KmbDataClassNonPhi, DataClass::Public; "NonPHI")]
    #[test_case(KmbDataClass::KmbDataClassDeidentified, DataClass::Deidentified; "Deidentified")]
    fn test_data_class_conversion(ffi_class: KmbDataClass, expected: DataClass) {
        let result = map_data_class(ffi_class).unwrap();
        assert_eq!(result, expected);
    }

    // ========================================================================
    // Error Code Mapping Tests
    // ========================================================================

    #[test]
    fn test_map_error_connection() {
        let io_err =
            std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "connection failed");
        let err = ClientError::Connection(io_err);
        assert_eq!(map_error(err), KmbError::KmbErrConnectionFailed);
    }

    #[test]
    fn test_map_error_handshake() {
        let err = ClientError::HandshakeFailed("handshake failed".to_string());
        assert_eq!(map_error(err), KmbError::KmbErrAuthFailed);
    }

    #[test]
    fn test_map_error_timeout() {
        let err = ClientError::Timeout;
        assert_eq!(map_error(err), KmbError::KmbErrTimeout);
    }

    #[test]
    fn test_map_error_not_connected() {
        let err = ClientError::NotConnected;
        assert_eq!(map_error(err), KmbError::KmbErrConnectionFailed);
    }

    // ========================================================================
    // Error Message Tests
    // ========================================================================

    #[test_case(KmbError::KmbOk, "Success"; "ok")]
    #[test_case(KmbError::KmbErrNullPointer, "NULL"; "null pointer")]
    #[test_case(KmbError::KmbErrInvalidUtf8, "UTF"; "invalid utf8")]
    #[test_case(KmbError::KmbErrConnectionFailed, "Failed to connect"; "connection failed")]
    #[test_case(KmbError::KmbErrStreamNotFound, "Stream"; "stream not found")]
    #[test_case(KmbError::KmbErrTimeout, "timed out"; "timeout")]
    fn test_error_message_contains_expected(error: KmbError, expected_substring: &str) {
        unsafe {
            let msg_ptr = kmb_error_message(error);
            assert!(!msg_ptr.is_null(), "Error message should not be null");

            let msg = CStr::from_ptr(msg_ptr).to_str().unwrap();
            assert!(
                msg.contains(expected_substring),
                "Expected '{msg}' to contain '{expected_substring}'"
            );
        }
    }

    #[test]
    fn test_all_error_messages_valid_utf8() {
        unsafe {
            let errors = [
                KmbError::KmbOk,
                KmbError::KmbErrNullPointer,
                KmbError::KmbErrInvalidUtf8,
                KmbError::KmbErrConnectionFailed,
                KmbError::KmbErrStreamNotFound,
                KmbError::KmbErrPermissionDenied,
                KmbError::KmbErrInvalidDataClass,
                KmbError::KmbErrOffsetOutOfRange,
                KmbError::KmbErrQuerySyntax,
                KmbError::KmbErrQueryExecution,
                KmbError::KmbErrTenantNotFound,
                KmbError::KmbErrAuthFailed,
                KmbError::KmbErrTimeout,
                KmbError::KmbErrInternal,
                KmbError::KmbErrClusterUnavailable,
                KmbError::KmbErrUnknown,
            ];

            for error in errors {
                let msg_ptr = kmb_error_message(error);
                assert!(
                    !msg_ptr.is_null(),
                    "Error message for {error:?} should not be null"
                );

                let result = CStr::from_ptr(msg_ptr).to_str();
                assert!(
                    result.is_ok(),
                    "Error message for {error:?} should be valid UTF-8"
                );
            }
        }
    }

    // ========================================================================
    // Retryable Error Tests
    // ========================================================================

    #[test_case(KmbError::KmbOk, 0; "ok not retryable")]
    #[test_case(KmbError::KmbErrTimeout, 1; "timeout retryable")]
    #[test_case(KmbError::KmbErrClusterUnavailable, 1; "cluster unavailable retryable")]
    #[test_case(KmbError::KmbErrNullPointer, 0; "null pointer not retryable")]
    #[test_case(KmbError::KmbErrInvalidUtf8, 0; "invalid utf8 not retryable")]
    #[test_case(KmbError::KmbErrPermissionDenied, 0; "permission denied not retryable")]
    #[test_case(KmbError::KmbErrQuerySyntax, 0; "query syntax not retryable")]
    fn test_error_retryable_classification(error: KmbError, expected: c_int) {
        unsafe {
            assert_eq!(kmb_error_is_retryable(error), expected);
        }
    }

    // ========================================================================
    // Query Parameter Conversion Edge Cases
    // ========================================================================

    #[test]
    fn test_convert_query_param_boolean_false() {
        unsafe {
            let param = KmbQueryParam {
                param_type: KmbQueryParamType::KmbParamBoolean,
                bigint_val: 0,
                text_val: std::ptr::null(),
                bool_val: 0,
                timestamp_val: 0,
            };
            let result = convert_query_param(&param).unwrap();
            assert!(matches!(result, QueryParam::Boolean(false)));
        }
    }

    #[test]
    fn test_convert_query_param_boolean_nonzero() {
        unsafe {
            // Any non-zero value should be true
            let param = KmbQueryParam {
                param_type: KmbQueryParamType::KmbParamBoolean,
                bigint_val: 0,
                text_val: std::ptr::null(),
                bool_val: 42,
                timestamp_val: 0,
            };
            let result = convert_query_param(&param).unwrap();
            assert!(matches!(result, QueryParam::Boolean(true)));
        }
    }

    #[test]
    fn test_convert_query_param_text_empty_string() {
        unsafe {
            let text = CString::new("").unwrap();
            let param = KmbQueryParam {
                param_type: KmbQueryParamType::KmbParamText,
                bigint_val: 0,
                text_val: text.as_ptr(),
                bool_val: 0,
                timestamp_val: 0,
            };
            let result = convert_query_param(&param).unwrap();
            if let QueryParam::Text(s) = result {
                assert_eq!(s, "");
            } else {
                panic!("expected text param");
            }
        }
    }

    #[test]
    fn test_convert_query_param_text_unicode() {
        unsafe {
            let text = CString::new("Hello 世界 🌍").unwrap();
            let param = KmbQueryParam {
                param_type: KmbQueryParamType::KmbParamText,
                bigint_val: 0,
                text_val: text.as_ptr(),
                bool_val: 0,
                timestamp_val: 0,
            };
            let result = convert_query_param(&param).unwrap();
            if let QueryParam::Text(s) = result {
                assert_eq!(s, "Hello 世界 🌍");
            } else {
                panic!("expected text param");
            }
        }
    }

    #[test]
    fn test_convert_query_param_bigint_negative() {
        unsafe {
            let param = KmbQueryParam {
                param_type: KmbQueryParamType::KmbParamBigInt,
                bigint_val: -9223372036854775808, // i64::MIN
                text_val: std::ptr::null(),
                bool_val: 0,
                timestamp_val: 0,
            };
            let result = convert_query_param(&param).unwrap();
            assert!(matches!(result, QueryParam::BigInt(-9223372036854775808)));
        }
    }

    #[test]
    fn test_convert_query_param_bigint_max() {
        unsafe {
            let param = KmbQueryParam {
                param_type: KmbQueryParamType::KmbParamBigInt,
                bigint_val: 9223372036854775807, // i64::MAX
                text_val: std::ptr::null(),
                bool_val: 0,
                timestamp_val: 0,
            };
            let result = convert_query_param(&param).unwrap();
            assert!(matches!(result, QueryParam::BigInt(9223372036854775807)));
        }
    }

    #[test]
    fn test_convert_query_param_timestamp_zero() {
        unsafe {
            let param = KmbQueryParam {
                param_type: KmbQueryParamType::KmbParamTimestamp,
                bigint_val: 0,
                text_val: std::ptr::null(),
                bool_val: 0,
                timestamp_val: 0,
            };
            let result = convert_query_param(&param).unwrap();
            assert!(matches!(result, QueryParam::Timestamp(0)));
        }
    }

    // ========================================================================
    // Query Value Conversion Edge Cases
    // ========================================================================

    #[test]
    fn test_convert_query_value_boolean_false() {
        unsafe {
            let value = QueryValue::Boolean(false);
            let result = convert_query_value(value).unwrap();
            assert_eq!(result.value_type, KmbQueryValueType::KmbValueBoolean);
            assert_eq!(result.bool_val, 0);
        }
    }

    #[test]
    fn test_convert_query_value_text_empty() {
        unsafe {
            let value = QueryValue::Text(String::new());
            let result = convert_query_value(value).unwrap();
            assert_eq!(result.value_type, KmbQueryValueType::KmbValueText);
            assert!(!result.text_val.is_null());
            let text = CStr::from_ptr(result.text_val).to_str().unwrap();
            assert_eq!(text, "");
            // Clean up
            let _ = CString::from_raw(result.text_val);
        }
    }

    #[test]
    fn test_convert_query_value_text_unicode() {
        unsafe {
            let value = QueryValue::Text("Hello 世界 🌍".to_string());
            let result = convert_query_value(value).unwrap();
            assert_eq!(result.value_type, KmbQueryValueType::KmbValueText);
            assert!(!result.text_val.is_null());
            let text = CStr::from_ptr(result.text_val).to_str().unwrap();
            assert_eq!(text, "Hello 世界 🌍");
            // Clean up
            let _ = CString::from_raw(result.text_val);
        }
    }

    #[test]
    fn test_convert_query_value_bigint_negative() {
        unsafe {
            let value = QueryValue::BigInt(-1000);
            let result = convert_query_value(value).unwrap();
            assert_eq!(result.value_type, KmbQueryValueType::KmbValueBigInt);
            assert_eq!(result.bigint_val, -1000);
        }
    }

    #[test]
    fn test_convert_query_value_timestamp_negative() {
        unsafe {
            let value = QueryValue::Timestamp(-1000);
            let result = convert_query_value(value).unwrap();
            assert_eq!(result.value_type, KmbQueryValueType::KmbValueTimestamp);
            assert_eq!(result.timestamp_val, -1000);
        }
    }

    // ========================================================================
    // Memory Safety Tests
    // ========================================================================

    #[test]
    fn test_query_value_text_no_double_free() {
        unsafe {
            let value = QueryValue::Text("test".to_string());
            let result = convert_query_value(value).unwrap();

            // Take ownership and free
            let _ = CString::from_raw(result.text_val);

            // Should not double-free (test passes if no crash)
        }
    }

    #[test]
    fn test_multiple_query_values_independent() {
        unsafe {
            let value1 = QueryValue::Text("first".to_string());
            let value2 = QueryValue::Text("second".to_string());

            let result1 = convert_query_value(value1).unwrap();
            let result2 = convert_query_value(value2).unwrap();

            // Verify they're independent
            let text1 = CStr::from_ptr(result1.text_val).to_str().unwrap();
            let text2 = CStr::from_ptr(result2.text_val).to_str().unwrap();
            assert_eq!(text1, "first");
            assert_eq!(text2, "second");

            // Clean up
            let _ = CString::from_raw(result1.text_val);
            let _ = CString::from_raw(result2.text_val);
        }
    }

    // ========================================================================
    // Property-Based Tests
    // ========================================================================

    use proptest::prelude::*;

    proptest! {
        /// Property: Any valid UTF-8 string can be converted to query param and back
        #[test]
        fn prop_query_param_text_roundtrip(text in "\\PC{0,100}") {
            unsafe {
                let c_string = CString::new(text.clone()).unwrap();
                let param = KmbQueryParam {
                    param_type: KmbQueryParamType::KmbParamText,
                    bigint_val: 0,
                    text_val: c_string.as_ptr(),
                    bool_val: 0,
                    timestamp_val: 0,
                };

                let result = convert_query_param(&param).unwrap();
                if let QueryParam::Text(s) = result {
                    prop_assert_eq!(s, text);
                } else {
                    prop_assert!(false, "expected text param");
                }
            }
        }

        /// Property: Any i64 can be converted to BigInt param
        #[test]
        fn prop_query_param_bigint(value in any::<i64>()) {
            unsafe {
                let param = KmbQueryParam {
                    param_type: KmbQueryParamType::KmbParamBigInt,
                    bigint_val: value,
                    text_val: std::ptr::null(),
                    bool_val: 0,
                    timestamp_val: 0,
                };

                let result = convert_query_param(&param).unwrap();
                prop_assert!(matches!(result, QueryParam::BigInt(v) if v == value));
            }
        }

        /// Property: Any boolean can be converted
        #[test]
        fn prop_query_param_boolean(value in any::<bool>()) {
            unsafe {
                let param = KmbQueryParam {
                    param_type: KmbQueryParamType::KmbParamBoolean,
                    bigint_val: 0,
                    text_val: std::ptr::null(),
                    bool_val: if value { 1 } else { 0 },
                    timestamp_val: 0,
                };

                let result = convert_query_param(&param).unwrap();
                prop_assert!(matches!(result, QueryParam::Boolean(b) if b == value));
            }
        }

        /// Property: Any i64 timestamp can be converted
        #[test]
        fn prop_query_param_timestamp(value in any::<i64>()) {
            unsafe {
                let param = KmbQueryParam {
                    param_type: KmbQueryParamType::KmbParamTimestamp,
                    bigint_val: 0,
                    text_val: std::ptr::null(),
                    bool_val: 0,
                    timestamp_val: value,
                };

                let result = convert_query_param(&param).unwrap();
                prop_assert!(matches!(result, QueryParam::Timestamp(v) if v == value));
            }
        }

        /// Property: Query value BigInt preserves value
        #[test]
        fn prop_query_value_bigint(value in any::<i64>()) {
            unsafe {
                let qvalue = QueryValue::BigInt(value);
                let result = convert_query_value(qvalue).unwrap();

                prop_assert_eq!(result.value_type, KmbQueryValueType::KmbValueBigInt);
                prop_assert_eq!(result.bigint_val, value);
            }
        }

        /// Property: Query value Boolean preserves value
        #[test]
        fn prop_query_value_boolean(value in any::<bool>()) {
            unsafe {
                let qvalue = QueryValue::Boolean(value);
                let result = convert_query_value(qvalue).unwrap();

                prop_assert_eq!(result.value_type, KmbQueryValueType::KmbValueBoolean);
                prop_assert_eq!(result.bool_val, if value { 1 } else { 0 });
            }
        }

        /// Property: Query value Timestamp preserves value
        #[test]
        fn prop_query_value_timestamp(value in any::<i64>()) {
            unsafe {
                let qvalue = QueryValue::Timestamp(value);
                let result = convert_query_value(qvalue).unwrap();

                prop_assert_eq!(result.value_type, KmbQueryValueType::KmbValueTimestamp);
                prop_assert_eq!(result.timestamp_val, value);
            }
        }
    }
}
