/**
 * Type definitions for Kimberlite TypeScript SDK.
 */

/**
 * Stream identifier (u64).
 */
export type StreamId = bigint;

/**
 * Event offset within a stream (u64).
 */
export type Offset = bigint;

/**
 * Tenant identifier (u64).
 */
export type TenantId = bigint;

/**
 * Data classification for streams.
 */
export enum DataClass {
  /** Protected Health Information (HIPAA-regulated) */
  PHI = 0,
  /** Non-PHI data */
  NonPHI = 1,
  /** De-identified data */
  Deidentified = 2,
}

/**
 * A single event read from a stream.
 */
export interface Event {
  /** Position of event in stream */
  offset: Offset;
  /** Event payload bytes */
  data: Buffer;
}

/**
 * Result of a SQL query.
 */
export interface QueryResult {
  /** Column names in result set */
  columns: string[];
  /** Rows of data (each row contains Value objects matching columns) */
  rows: import('./value').Value[][];
}

/**
 * Client connection configuration.
 */
export interface ClientConfig {
  /** List of "host:port" server addresses */
  addresses: string[];
  /** Tenant identifier */
  tenantId: TenantId;
  /** Optional authentication token */
  authToken?: string;
  /** Client name (for server logs) */
  clientName?: string;
  /** Client version string */
  clientVersion?: string;
}
