/**
 * Low-level FFI bindings to kimberlite-ffi library.
 */

import * as ffi from 'ffi-napi';
import * as ref from 'ref-napi';
import * as Struct from 'ref-struct-napi';
import * as path from 'path';
import * as fs from 'fs';

// Define C types
const uint64 = ref.types.uint64;
const size_t = ref.types.size_t;
const c_int = ref.types.int;

// Opaque client handle
const KmbClient = ref.refType(ref.types.void);

// KmbClientConfig structure
const KmbClientConfig = Struct({
  addresses: ref.refType(ref.types.CString),
  address_count: size_t,
  tenant_id: uint64,
  auth_token: ref.types.CString,
  client_name: ref.types.CString,
  client_version: ref.types.CString,
});

// KmbReadResult structure
const KmbReadResult = Struct({
  events: ref.refType(ref.refType(ref.types.uint8)),
  event_lengths: ref.refType(size_t),
  event_count: size_t,
});

// Error codes
export const KMB_OK = 0;
export const KMB_ERR_NULL_POINTER = 1;
export const KMB_ERR_INVALID_UTF8 = 2;
export const KMB_ERR_CONNECTION_FAILED = 3;
export const KMB_ERR_STREAM_NOT_FOUND = 4;
export const KMB_ERR_PERMISSION_DENIED = 5;
export const KMB_ERR_INVALID_DATA_CLASS = 6;
export const KMB_ERR_OFFSET_OUT_OF_RANGE = 7;
export const KMB_ERR_QUERY_SYNTAX = 8;
export const KMB_ERR_QUERY_EXECUTION = 9;
export const KMB_ERR_TENANT_NOT_FOUND = 10;
export const KMB_ERR_AUTH_FAILED = 11;
export const KMB_ERR_TIMEOUT = 12;
export const KMB_ERR_INTERNAL = 13;
export const KMB_ERR_CLUSTER_UNAVAILABLE = 14;
export const KMB_ERR_UNKNOWN = 15;

/**
 * Find the kimberlite-ffi shared library.
 */
function findLibrary(): string {
  // Determine library name based on platform
  let libName: string;
  if (process.platform === 'darwin') {
    libName = 'libkimberlite_ffi.dylib';
  } else if (process.platform === 'win32') {
    libName = 'kimberlite_ffi.dll';
  } else {
    libName = 'libkimberlite_ffi.so';
  }

  // Try development location
  const projectRoot = path.join(__dirname, '..', '..', '..');
  const devPath = path.join(projectRoot, 'target', 'debug', libName);
  if (fs.existsSync(devPath)) {
    return devPath;
  }

  // Try release location
  const releasePath = path.join(projectRoot, 'target', 'release', libName);
  if (fs.existsSync(releasePath)) {
    return releasePath;
  }

  // Try package bundled library
  const bundledPath = path.join(__dirname, '..', 'lib', libName);
  if (fs.existsSync(bundledPath)) {
    return bundledPath;
  }

  throw new Error(
    `Could not find ${libName}. ` +
      "Make sure to build kimberlite-ffi with 'cargo build -p kimberlite-ffi'"
  );
}

// Load the library
const libPath = findLibrary();

export const lib = ffi.Library(libPath, {
  kmb_client_connect: [c_int, [ref.refType(KmbClientConfig), ref.refType(KmbClient)]],
  kmb_client_disconnect: ['void', [KmbClient]],
  kmb_client_create_stream: [c_int, [KmbClient, 'string', c_int, ref.refType(uint64)]],
  kmb_client_append: [
    c_int,
    [
      KmbClient,
      uint64,
      ref.refType(ref.refType(ref.types.uint8)),
      ref.refType(size_t),
      size_t,
      ref.refType(uint64),
    ],
  ],
  kmb_client_read_events: [
    c_int,
    [KmbClient, uint64, uint64, uint64, ref.refType(ref.refType(KmbReadResult))],
  ],
  kmb_read_result_free: ['void', [ref.refType(KmbReadResult)]],
  kmb_error_message: ['string', [c_int]],
  kmb_error_is_retryable: [c_int, [c_int]],
});

export { KmbClient, KmbClientConfig, KmbReadResult, uint64, size_t };
