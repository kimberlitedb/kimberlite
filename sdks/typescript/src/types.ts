/**
 * Type definitions for the Kimberlite TypeScript SDK.
 */

/** Stream identifier (u64 on the wire). */
export type StreamId = bigint;

/** Event offset within a stream (u64 on the wire). */
export type Offset = bigint;

/** Tenant identifier (u64 on the wire). */
export type TenantId = bigint;

/**
 * Data classification for a stream. Values match the Rust `DataClass` enum
 * 1:1 and are forwarded verbatim to the native addon.
 */
export enum DataClass {
  /** Protected Health Information — HIPAA-regulated. */
  PHI = 'PHI',
  /** De-identified health data (HIPAA Safe Harbor). */
  Deidentified = 'Deidentified',
  /** Personally Identifiable Information — GDPR Art. 4. */
  PII = 'PII',
  /** GDPR Article 9 special-category data. */
  Sensitive = 'Sensitive',
  /** Payment Card Industry data — PCI DSS. */
  PCI = 'PCI',
  /** Financial records — SOX. */
  Financial = 'Financial',
  /** Internal / confidential business data. */
  Confidential = 'Confidential',
  /** Publicly available data. */
  Public = 'Public',
}

/** Geographic placement policy for a stream. */
export enum Placement {
  Global = 'Global',
  UsEast1 = 'UsEast1',
  ApSoutheast2 = 'ApSoutheast2',
}

/** A single event read from a stream. */
export interface Event {
  /** Position of event in stream. */
  offset: Offset;
  /** Event payload bytes. */
  data: Buffer;
}

/** Result of a SQL query. */
export interface QueryResult {
  /** Column names in the result set. */
  columns: string[];
  /** Rows of data — each cell is a typed Value (see `./value`). */
  rows: import('./value').Value[][];
}

/** Client connection configuration. */
export interface ClientConfig {
  /**
   * Server address as "host:port". Legacy array form (first address used) is
   * also accepted for backwards compatibility.
   */
  addresses: string[] | string;
  /** Tenant identifier. */
  tenantId: TenantId;
  /** Optional bearer token for authenticated connections. */
  authToken?: string;
  /** Read timeout in milliseconds (default: 30_000). */
  readTimeoutMs?: number;
  /** Write timeout in milliseconds (default: 30_000). */
  writeTimeoutMs?: number;
  /** Internal read buffer size in bytes (default: 64 KiB). */
  bufferSizeBytes?: number;
  /**
   * Reconnect automatically on connection-level failures (default: `true`).
   *
   * When the native layer reports a connection fault — broken pipe, peer
   * reset, idle close — the `Client` transparently opens a fresh native
   * connection and retries the current call exactly once. The retry still
   * fails fast if the server itself is down.
   *
   * Set to `false` for strict single-connection semantics (for instance,
   * tests that inspect socket-close behaviour directly).
   *
   * Note: retries happen at the wire level. Mutations with server-side
   * idempotence (`append` with an expected offset, upsert-shaped SQL) are
   * safe under retry. Plain `INSERT`/`execute` without dedupe keys may
   * double-apply if the server processed the first attempt but the
   * response was lost; callers that can't tolerate that should pair
   * `autoReconnect: false` with their own retry policy.
   */
  autoReconnect?: boolean;
}
