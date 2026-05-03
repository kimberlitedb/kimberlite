"""Low-level FFI bindings to kimberlite-ffi library."""

import ctypes
import os
import sys
from pathlib import Path
from typing import Optional

# Find the FFI library
def _find_library() -> Path:
    """Locate the kimberlite-ffi shared library.

    Returns:
        Path to the shared library

    Raises:
        RuntimeError: If library cannot be found
    """
    # Check if running from development tree
    sdk_dir = Path(__file__).parent.parent.parent.parent
    target_dir = sdk_dir / "target" / "debug"

    # Platform-specific library names
    if sys.platform == "darwin":
        lib_name = "libkimberlite_ffi.dylib"
    elif sys.platform == "win32":
        lib_name = "kimberlite_ffi.dll"
    else:  # Linux
        lib_name = "libkimberlite_ffi.so"

    # Try development location
    dev_path = target_dir / lib_name
    if dev_path.exists():
        return dev_path

    # Try release location
    release_path = sdk_dir / "target" / "release" / lib_name
    if release_path.exists():
        return release_path

    # Try package bundled library
    bundled_path = Path(__file__).parent / "lib" / lib_name
    if bundled_path.exists():
        return bundled_path

    raise RuntimeError(
        f"Could not find {lib_name}. "
        "Make sure to build kimberlite-ffi with 'cargo build -p kimberlite-ffi'"
    )


# Load the library
_lib_path = _find_library()
_lib = ctypes.CDLL(str(_lib_path))

# Define C types
class KmbClientConfig(ctypes.Structure):
    """FFI client configuration structure."""
    _fields_ = [
        ("addresses", ctypes.POINTER(ctypes.c_char_p)),
        ("address_count", ctypes.c_size_t),
        ("tenant_id", ctypes.c_uint64),
        ("auth_token", ctypes.c_char_p),
        ("client_name", ctypes.c_char_p),
        ("client_version", ctypes.c_char_p),
    ]


class KmbReadResult(ctypes.Structure):
    """FFI read result structure."""
    _fields_ = [
        ("events", ctypes.POINTER(ctypes.POINTER(ctypes.c_uint8))),
        ("event_lengths", ctypes.POINTER(ctypes.c_size_t)),
        ("event_count", ctypes.c_size_t),
    ]


class KmbQueryParam(ctypes.Structure):
    """FFI query parameter structure."""
    _fields_ = [
        ("param_type", ctypes.c_int),
        ("bigint_val", ctypes.c_int64),
        ("text_val", ctypes.c_char_p),
        ("bool_val", ctypes.c_int),
        ("timestamp_val", ctypes.c_int64),
    ]


class KmbQueryValue(ctypes.Structure):
    """FFI query value structure."""
    _fields_ = [
        ("value_type", ctypes.c_int),
        ("bigint_val", ctypes.c_int64),
        ("text_val", ctypes.c_char_p),
        ("bool_val", ctypes.c_int),
        ("timestamp_val", ctypes.c_int64),
    ]


class KmbQueryResult(ctypes.Structure):
    """FFI query result structure."""
    _fields_ = [
        ("columns", ctypes.POINTER(ctypes.c_char_p)),
        ("column_count", ctypes.c_size_t),
        ("rows", ctypes.POINTER(ctypes.POINTER(KmbQueryValue))),
        ("row_lengths", ctypes.POINTER(ctypes.c_size_t)),
        ("row_count", ctypes.c_size_t),
    ]


class KmbExecuteResult(ctypes.Structure):
    """FFI execute result structure (DML / DDL acknowledgement)."""
    _fields_ = [
        ("rows_affected", ctypes.c_uint64),
        ("log_offset", ctypes.c_uint64),
    ]


class KmbSubscribeResult(ctypes.Structure):
    """FFI subscribe result (subscription_id + start_offset + initial_credits)."""
    _fields_ = [
        ("subscription_id", ctypes.c_uint64),
        ("start_offset", ctypes.c_uint64),
        ("initial_credits", ctypes.c_uint32),
    ]


class KmbSubscriptionEvent(ctypes.Structure):
    """FFI subscription event (single event OR close marker)."""
    _fields_ = [
        ("offset", ctypes.c_uint64),
        ("data", ctypes.POINTER(ctypes.c_uint8)),
        ("data_len", ctypes.c_size_t),
        ("closed", ctypes.c_int),
        ("close_reason", ctypes.c_int),
    ]


class KmbAdminJson(ctypes.Structure):
    """JSON-shaped admin result (library-owned C string)."""
    _fields_ = [("json", ctypes.c_char_p)]


# Subscription close-reason enum values (matches KmbSubscriptionCloseReason in lib.rs).
KMB_CLOSE_CLIENT_CANCELLED = 0
KMB_CLOSE_SERVER_SHUTDOWN = 1
KMB_CLOSE_STREAM_DELETED = 2
KMB_CLOSE_BACKPRESSURE_TIMEOUT = 3
KMB_CLOSE_PROTOCOL_ERROR = 4

_CLOSE_REASON_NAMES = {
    KMB_CLOSE_CLIENT_CANCELLED: "ClientCancelled",
    KMB_CLOSE_SERVER_SHUTDOWN: "ServerShutdown",
    KMB_CLOSE_STREAM_DELETED: "StreamDeleted",
    KMB_CLOSE_BACKPRESSURE_TIMEOUT: "BackpressureTimeout",
    KMB_CLOSE_PROTOCOL_ERROR: "ProtocolError",
}

def close_reason_name(code: int) -> str:
    return _CLOSE_REASON_NAMES.get(code, f"Unknown({code})")


class KmbPoolConfig(ctypes.Structure):
    """FFI pool configuration structure."""
    _fields_ = [
        ("addresses", ctypes.POINTER(ctypes.c_char_p)),
        ("address_count", ctypes.c_size_t),
        ("tenant_id", ctypes.c_uint64),
        ("auth_token", ctypes.c_char_p),
        ("max_size", ctypes.c_size_t),
        ("acquire_timeout_ms", ctypes.c_uint64),
        ("idle_timeout_ms", ctypes.c_uint64),
    ]


# Opaque client handle
KmbClient = ctypes.c_void_p
# Opaque pool handle
KmbPool = ctypes.c_void_p
# Opaque pooled-client handle
KmbPooledClient = ctypes.c_void_p

# Error codes enum
KMB_OK = 0
KMB_ERR_NULL_POINTER = 1
KMB_ERR_INVALID_UTF8 = 2
KMB_ERR_CONNECTION_FAILED = 3
KMB_ERR_STREAM_NOT_FOUND = 4
KMB_ERR_PERMISSION_DENIED = 5
KMB_ERR_INVALID_DATA_CLASS = 6
KMB_ERR_OFFSET_OUT_OF_RANGE = 7
KMB_ERR_QUERY_SYNTAX = 8
KMB_ERR_QUERY_EXECUTION = 9
KMB_ERR_TENANT_NOT_FOUND = 10
KMB_ERR_AUTH_FAILED = 11
KMB_ERR_TIMEOUT = 12
KMB_ERR_INTERNAL = 13
KMB_ERR_CLUSTER_UNAVAILABLE = 14
KMB_ERR_UNKNOWN = 15

# Define function signatures

# kmb_client_connect
_lib.kmb_client_connect.argtypes = [
    ctypes.POINTER(KmbClientConfig),
    ctypes.POINTER(KmbClient),
]
_lib.kmb_client_connect.restype = ctypes.c_int

# kmb_client_disconnect
_lib.kmb_client_disconnect.argtypes = [KmbClient]
_lib.kmb_client_disconnect.restype = None

# kmb_client_create_stream
_lib.kmb_client_create_stream.argtypes = [
    KmbClient,
    ctypes.c_char_p,
    ctypes.c_int,
    ctypes.POINTER(ctypes.c_uint64),
]
_lib.kmb_client_create_stream.restype = ctypes.c_int

# kmb_client_create_stream_with_placement
_lib.kmb_client_create_stream_with_placement.argtypes = [
    KmbClient,
    ctypes.c_char_p,      # name
    ctypes.c_int,         # KmbDataClass
    ctypes.c_int,         # KmbPlacement
    ctypes.c_char_p,      # custom_region (nullable)
    ctypes.POINTER(ctypes.c_uint64),
]
_lib.kmb_client_create_stream_with_placement.restype = ctypes.c_int

# kmb_client_tenant_id
_lib.kmb_client_tenant_id.argtypes = [
    KmbClient,
    ctypes.POINTER(ctypes.c_uint64),
]
_lib.kmb_client_tenant_id.restype = ctypes.c_int

# kmb_client_last_request_id
_lib.kmb_client_last_request_id.argtypes = [
    KmbClient,
    ctypes.POINTER(ctypes.c_uint64),
]
_lib.kmb_client_last_request_id.restype = ctypes.c_int

# kmb_client_execute
_lib.kmb_client_execute.argtypes = [
    KmbClient,
    ctypes.c_char_p,
    ctypes.POINTER(KmbQueryParam),
    ctypes.c_size_t,
    ctypes.POINTER(KmbExecuteResult),
]
_lib.kmb_client_execute.restype = ctypes.c_int

# kmb_pool_create
_lib.kmb_pool_create.argtypes = [
    ctypes.POINTER(KmbPoolConfig),
    ctypes.POINTER(KmbPool),
]
_lib.kmb_pool_create.restype = ctypes.c_int

# kmb_pool_acquire
_lib.kmb_pool_acquire.argtypes = [
    KmbPool,
    ctypes.POINTER(KmbPooledClient),
]
_lib.kmb_pool_acquire.restype = ctypes.c_int

# kmb_pool_release
_lib.kmb_pool_release.argtypes = [KmbPooledClient]
_lib.kmb_pool_release.restype = None

# kmb_pool_discard
_lib.kmb_pool_discard.argtypes = [KmbPooledClient]
_lib.kmb_pool_discard.restype = None

# kmb_pool_stats
_lib.kmb_pool_stats.argtypes = [
    KmbPool,
    ctypes.POINTER(ctypes.c_size_t),   # max_size_out
    ctypes.POINTER(ctypes.c_size_t),   # open_out
    ctypes.POINTER(ctypes.c_size_t),   # idle_out
    ctypes.POINTER(ctypes.c_size_t),   # in_use_out
    ctypes.POINTER(ctypes.c_int),      # shutdown_out
]
_lib.kmb_pool_stats.restype = ctypes.c_int

# kmb_pool_shutdown
_lib.kmb_pool_shutdown.argtypes = [KmbPool]
_lib.kmb_pool_shutdown.restype = None

# kmb_pool_destroy
_lib.kmb_pool_destroy.argtypes = [KmbPool]
_lib.kmb_pool_destroy.restype = None

# kmb_pooled_client_as_client — returns a borrowed *mut KmbClient; NOT owned.
_lib.kmb_pooled_client_as_client.argtypes = [KmbPooledClient]
_lib.kmb_pooled_client_as_client.restype = KmbClient

# kmb_subscribe
_lib.kmb_subscribe.argtypes = [
    KmbClient,
    ctypes.c_uint64,   # stream_id
    ctypes.c_uint64,   # from_offset
    ctypes.c_uint32,   # initial_credits
    ctypes.POINTER(KmbSubscribeResult),
]
_lib.kmb_subscribe.restype = ctypes.c_int

# kmb_subscription_grant_credits
_lib.kmb_subscription_grant_credits.argtypes = [
    KmbClient,
    ctypes.c_uint64,   # subscription_id
    ctypes.c_uint32,   # additional_credits
    ctypes.POINTER(ctypes.c_uint32),  # new_balance_out
]
_lib.kmb_subscription_grant_credits.restype = ctypes.c_int

# kmb_subscription_unsubscribe
_lib.kmb_subscription_unsubscribe.argtypes = [
    KmbClient,
    ctypes.c_uint64,   # subscription_id
]
_lib.kmb_subscription_unsubscribe.restype = ctypes.c_int

# kmb_subscription_next
_lib.kmb_subscription_next.argtypes = [
    KmbClient,
    ctypes.c_uint64,   # subscription_id
    ctypes.POINTER(KmbSubscriptionEvent),
]
_lib.kmb_subscription_next.restype = ctypes.c_int

# kmb_subscription_event_free
_lib.kmb_subscription_event_free.argtypes = [ctypes.POINTER(KmbSubscriptionEvent)]
_lib.kmb_subscription_event_free.restype = None

# --- Phase 4 admin FFI signatures -----------------------------------------

# kmb_admin_json_free
_lib.kmb_admin_json_free.argtypes = [ctypes.POINTER(KmbAdminJson)]
_lib.kmb_admin_json_free.restype = None

# Each admin call shares the shape (client, ...inputs..., *KmbAdminJson) -> c_int.

_lib.kmb_admin_list_tables.argtypes = [KmbClient, ctypes.POINTER(KmbAdminJson)]
_lib.kmb_admin_list_tables.restype = ctypes.c_int

_lib.kmb_admin_describe_table.argtypes = [
    KmbClient,
    ctypes.c_char_p,
    ctypes.POINTER(KmbAdminJson),
]
_lib.kmb_admin_describe_table.restype = ctypes.c_int

_lib.kmb_admin_list_indexes.argtypes = [
    KmbClient,
    ctypes.c_char_p,
    ctypes.POINTER(KmbAdminJson),
]
_lib.kmb_admin_list_indexes.restype = ctypes.c_int

_lib.kmb_admin_tenant_create.argtypes = [
    KmbClient,
    ctypes.c_uint64,  # tenant_id
    ctypes.c_char_p,  # name (nullable)
    ctypes.POINTER(KmbAdminJson),
]
_lib.kmb_admin_tenant_create.restype = ctypes.c_int

_lib.kmb_admin_tenant_list.argtypes = [KmbClient, ctypes.POINTER(KmbAdminJson)]
_lib.kmb_admin_tenant_list.restype = ctypes.c_int

_lib.kmb_admin_tenant_delete.argtypes = [
    KmbClient,
    ctypes.c_uint64,
    ctypes.POINTER(KmbAdminJson),
]
_lib.kmb_admin_tenant_delete.restype = ctypes.c_int

_lib.kmb_admin_tenant_get.argtypes = [
    KmbClient,
    ctypes.c_uint64,
    ctypes.POINTER(KmbAdminJson),
]
_lib.kmb_admin_tenant_get.restype = ctypes.c_int

_lib.kmb_admin_api_key_register.argtypes = [
    KmbClient,
    ctypes.c_char_p,  # subject
    ctypes.c_uint64,  # tenant_id
    ctypes.c_char_p,  # roles_json
    ctypes.c_uint64,  # expires_at_nanos (0 = no expiry)
    ctypes.POINTER(KmbAdminJson),
]
_lib.kmb_admin_api_key_register.restype = ctypes.c_int

_lib.kmb_admin_api_key_revoke.argtypes = [
    KmbClient,
    ctypes.c_char_p,
    ctypes.POINTER(KmbAdminJson),
]
_lib.kmb_admin_api_key_revoke.restype = ctypes.c_int

_lib.kmb_admin_api_key_list.argtypes = [
    KmbClient,
    ctypes.c_uint64,  # tenant_id filter (0 = all tenants)
    ctypes.POINTER(KmbAdminJson),
]
_lib.kmb_admin_api_key_list.restype = ctypes.c_int

_lib.kmb_admin_api_key_rotate.argtypes = [
    KmbClient,
    ctypes.c_char_p,
    ctypes.POINTER(KmbAdminJson),
]
_lib.kmb_admin_api_key_rotate.restype = ctypes.c_int

_lib.kmb_admin_server_info.argtypes = [KmbClient, ctypes.POINTER(KmbAdminJson)]
_lib.kmb_admin_server_info.restype = ctypes.c_int

# --- Phase 6: Masking policy catalogue (v0.6.0 Tier 2 #7) ---------------

_lib.kmb_admin_masking_policy_create.argtypes = [
    KmbClient,
    ctypes.c_char_p,  # name
    ctypes.c_char_p,  # strategy JSON
    ctypes.c_char_p,  # exempt_roles JSON array
]
_lib.kmb_admin_masking_policy_create.restype = ctypes.c_int

_lib.kmb_admin_masking_policy_drop.argtypes = [KmbClient, ctypes.c_char_p]
_lib.kmb_admin_masking_policy_drop.restype = ctypes.c_int

_lib.kmb_admin_masking_policy_attach.argtypes = [
    KmbClient,
    ctypes.c_char_p,  # table
    ctypes.c_char_p,  # column
    ctypes.c_char_p,  # policy name
]
_lib.kmb_admin_masking_policy_attach.restype = ctypes.c_int

_lib.kmb_admin_masking_policy_detach.argtypes = [
    KmbClient,
    ctypes.c_char_p,  # table
    ctypes.c_char_p,  # column
]
_lib.kmb_admin_masking_policy_detach.restype = ctypes.c_int

_lib.kmb_admin_masking_policy_list.argtypes = [
    KmbClient,
    ctypes.c_bool,  # include_attachments
    ctypes.POINTER(KmbAdminJson),
]
_lib.kmb_admin_masking_policy_list.restype = ctypes.c_int

# --- Phase 5 compliance (JSON-passthrough) -------------------------------

_lib.kmb_compliance_consent_grant.argtypes = [
    KmbClient,
    ctypes.c_char_p,  # subject_id
    ctypes.c_char_p,  # purpose (enum name)
    ctypes.c_char_p,  # basis_json (nullable UTF-8 JSON, wire v4)
    ctypes.c_char_p,  # options_json (nullable UTF-8 JSON, v0.6.2 / wire v5)
    ctypes.POINTER(KmbAdminJson),
]
_lib.kmb_compliance_consent_grant.restype = ctypes.c_int

_lib.kmb_compliance_consent_withdraw.argtypes = [
    KmbClient,
    ctypes.c_char_p,  # consent_id
    ctypes.POINTER(KmbAdminJson),
]
_lib.kmb_compliance_consent_withdraw.restype = ctypes.c_int

_lib.kmb_compliance_consent_check.argtypes = [
    KmbClient,
    ctypes.c_char_p,
    ctypes.c_char_p,
    ctypes.POINTER(KmbAdminJson),
]
_lib.kmb_compliance_consent_check.restype = ctypes.c_int

_lib.kmb_compliance_consent_list.argtypes = [
    KmbClient,
    ctypes.c_char_p,
    ctypes.c_int,  # valid_only (0/1)
    ctypes.POINTER(KmbAdminJson),
]
_lib.kmb_compliance_consent_list.restype = ctypes.c_int

_lib.kmb_compliance_erasure_request.argtypes = [
    KmbClient,
    ctypes.c_char_p,
    ctypes.POINTER(KmbAdminJson),
]
_lib.kmb_compliance_erasure_request.restype = ctypes.c_int

_lib.kmb_compliance_erasure_status.argtypes = [
    KmbClient,
    ctypes.c_char_p,
    ctypes.POINTER(KmbAdminJson),
]
_lib.kmb_compliance_erasure_status.restype = ctypes.c_int

_lib.kmb_compliance_erasure_complete.argtypes = [
    KmbClient,
    ctypes.c_char_p,
    ctypes.POINTER(KmbAdminJson),
]
_lib.kmb_compliance_erasure_complete.restype = ctypes.c_int

# kmb_compliance_erasure_mark_stream_erased — record per-stream
# progress on an in-flight erasure request. Mirrors TS
# client.compliance.erasure.markStreamErased.
_lib.kmb_compliance_erasure_mark_stream_erased.argtypes = [
    KmbClient,
    ctypes.c_char_p,  # request_id
    ctypes.c_uint64,  # stream_id
    ctypes.c_uint64,  # records_erased
    ctypes.POINTER(KmbAdminJson),
]
_lib.kmb_compliance_erasure_mark_stream_erased.restype = ctypes.c_int

# AUDIT-2026-04 S3.6 — audit-log query.
# Every filter optional: NULL string / 0 sentinel means
# unconstrained.
_lib.kmb_compliance_audit_query.argtypes = [
    KmbClient,
    ctypes.c_char_p,       # subject_id (nullable)
    ctypes.c_char_p,       # action_type (nullable)
    ctypes.c_uint64,       # time_from_nanos (0 = unbounded)
    ctypes.c_uint64,       # time_to_nanos (0 = unbounded)
    ctypes.c_char_p,       # actor (nullable)
    ctypes.c_uint32,       # limit (0 = server default)
    ctypes.POINTER(KmbAdminJson),
]
_lib.kmb_compliance_audit_query.restype = ctypes.c_int

# AUDIT-2026-04 S3.6 — GDPR Article 20 portability export.
_lib.kmb_compliance_export_subject.argtypes = [
    KmbClient,
    ctypes.c_char_p,  # subject_id
    ctypes.c_char_p,  # requester_id
    ctypes.c_char_p,  # format ("Json" | "Csv")
    ctypes.c_char_p,  # stream_ids_json (nullable JSON u64 array)
    ctypes.c_uint64,  # max_records_per_stream (0 = default)
    ctypes.POINTER(KmbAdminJson),
]
_lib.kmb_compliance_export_subject.restype = ctypes.c_int

_lib.kmb_compliance_erasure_exempt.argtypes = [
    KmbClient,
    ctypes.c_char_p,
    ctypes.c_char_p,  # basis
    ctypes.POINTER(KmbAdminJson),
]
_lib.kmb_compliance_erasure_exempt.restype = ctypes.c_int

_lib.kmb_compliance_erasure_list.argtypes = [KmbClient, ctypes.POINTER(KmbAdminJson)]
_lib.kmb_compliance_erasure_list.restype = ctypes.c_int

# kmb_client_append
_lib.kmb_client_append.argtypes = [
    KmbClient,
    ctypes.c_uint64,
    ctypes.c_uint64,  # expected_offset
    ctypes.POINTER(ctypes.POINTER(ctypes.c_uint8)),
    ctypes.POINTER(ctypes.c_size_t),
    ctypes.c_size_t,
    ctypes.POINTER(ctypes.c_uint64),
]
_lib.kmb_client_append.restype = ctypes.c_int

# kmb_client_read_events
_lib.kmb_client_read_events.argtypes = [
    KmbClient,
    ctypes.c_uint64,
    ctypes.c_uint64,
    ctypes.c_uint64,
    ctypes.POINTER(ctypes.POINTER(KmbReadResult)),
]
_lib.kmb_client_read_events.restype = ctypes.c_int

# kmb_read_result_free
_lib.kmb_read_result_free.argtypes = [ctypes.POINTER(KmbReadResult)]
_lib.kmb_read_result_free.restype = None

# kmb_error_message
_lib.kmb_error_message.argtypes = [ctypes.c_int]
_lib.kmb_error_message.restype = ctypes.c_char_p

# kmb_error_is_retryable
_lib.kmb_error_is_retryable.argtypes = [ctypes.c_int]
_lib.kmb_error_is_retryable.restype = ctypes.c_int

# kmb_client_query
_lib.kmb_client_query.argtypes = [
    KmbClient,
    ctypes.c_char_p,
    ctypes.POINTER(KmbQueryParam),
    ctypes.c_size_t,
    ctypes.POINTER(ctypes.POINTER(KmbQueryResult)),
]
_lib.kmb_client_query.restype = ctypes.c_int

# kmb_client_query_at
_lib.kmb_client_query_at.argtypes = [
    KmbClient,
    ctypes.c_char_p,
    ctypes.POINTER(KmbQueryParam),
    ctypes.c_size_t,
    ctypes.c_uint64,
    ctypes.POINTER(ctypes.POINTER(KmbQueryResult)),
]
_lib.kmb_client_query_at.restype = ctypes.c_int

# kmb_query_result_free
_lib.kmb_query_result_free.argtypes = [ctypes.POINTER(KmbQueryResult)]
_lib.kmb_query_result_free.restype = None

# AUDIT-2026-04 S3.9 — thread-local audit context for FFI callers.
# Python wraps each SDK method with _with_audit_attached() in client.py
# so the Rust client attaches actor/reason to the wire Request.
#
# kmb_audit_set(actor, reason, correlation_id, idempotency_key) — any NULL
_lib.kmb_audit_set.argtypes = [
    ctypes.c_char_p,  # actor
    ctypes.c_char_p,  # reason
    ctypes.c_char_p,  # correlation_id
    ctypes.c_char_p,  # idempotency_key
]
_lib.kmb_audit_set.restype = ctypes.c_int

# kmb_audit_clear()
_lib.kmb_audit_clear.argtypes = []
_lib.kmb_audit_clear.restype = ctypes.c_int


def _check_error(code: int) -> None:
    """Check FFI error code and raise exception if needed.

    Args:
        code: FFI error code

    Raises:
        KimberliteError: If error code is non-zero
    """
    if code != KMB_OK:
        from .errors import raise_for_error_code
        raise_for_error_code(code)
