/**
 * Type definitions for the Kimberlite TypeScript SDK.
 */

/**
 * Stream identifier (u64 on the wire).
 *
 * AUDIT-2026-04 S4.6 — branded nominal type. Rust / Python / Go /
 * Java all carry StreamId as a newtype; TS was the outlier, making
 * it trivial to concat a raw bigint into SQL. The `__brand` property
 * is a zero-runtime-cost phantom that blocks accidental mixing with
 * other bigint-shaped ids at compile time.
 */
export type StreamId = bigint & { readonly __brand: 'StreamId' };

/**
 * Builder + type guard for {@link StreamId}. Use `StreamId.from(v)`
 * to mint a branded id from a raw bigint, or read a raw `bigint`
 * back with `StreamId.raw(id)` when you need to log it.
 */
export const StreamId = {
  /** Mint a {@link StreamId} from a raw bigint or number. */
  from(v: bigint | number): StreamId {
    return BigInt(v) as StreamId;
  },
  /** Recover the underlying bigint — useful for logs. */
  raw(id: StreamId): bigint {
    return id as bigint;
  },
};

/** Event offset within a stream (u64 on the wire). */
export type Offset = bigint;

/**
 * Time-travel coordinate for `Client.queryAt()`.
 *
 * v0.6.0 Tier 2 #6 — the unambiguous discriminated-union form. Most
 * callers can pass a plain `Offset | Date | string | bigint` to
 * `queryAt` directly; use `AtClause` when you want to explicitly
 * disambiguate (e.g. a `bigint` that happens to fit both an offset
 * and a nanos timestamp).
 */
export type AtClause =
  | { kind: 'offset'; value: Offset }
  | { kind: 'timestampNs'; value: bigint };

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

/**
 * Column-keyed view over a single result-set row. Lighter than a
 * RowMapper when you only need to read a couple of columns by name.
 *
 * AUDIT-2026-04 S4.7 — resolves `row[columns.indexOf('id')]`
 * boilerplate. The underlying row is not copied — `get()` does a
 * single array lookup on the shared `columns` list.
 */
export interface RowView {
  /** Return the value in `column`, or `undefined` if no such column. */
  get(column: string): import('./value').Value | undefined;
  /** Raw positional access — equivalent to the underlying `Value[]`. */
  readonly values: ReadonlyArray<import('./value').Value>;
}

/** Result of a SQL query. */
export interface QueryResult {
  /** Column names in the result set. */
  columns: string[];
  /** Rows of data — each cell is a typed Value (see `./value`). */
  rows: import('./value').Value[][];
  /** Return a column-keyed view over the `index`-th row. */
  row(index: number): RowView;
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
