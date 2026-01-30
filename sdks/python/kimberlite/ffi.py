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


# Opaque client handle
KmbClient = ctypes.c_void_p

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

# kmb_client_append
_lib.kmb_client_append.argtypes = [
    KmbClient,
    ctypes.c_uint64,
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
