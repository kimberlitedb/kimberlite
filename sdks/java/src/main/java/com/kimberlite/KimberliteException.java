package com.kimberlite;

import java.util.OptionalLong;

/**
 * Exception thrown by the Kimberlite client for all operational errors.
 *
 * <p>Each exception carries an {@link ErrorCode} that classifies the failure,
 * making it possible to handle specific error conditions programmatically.
 *
 * <p>Example:
 * <pre>{@code
 * try {
 *     client.query("SELECT * FROM patients");
 * } catch (KimberliteException e) {
 *     if (e.getCode() == KimberliteException.ErrorCode.UNAUTHORIZED) {
 *         // Handle auth failure
 *     }
 *     e.getRequestId().ifPresent(id -> log.error("wire request {} failed", id));
 * }
 * }</pre>
 */
public class KimberliteException extends Exception {

    private final ErrorCode code;
    /**
     * Wire request id the server was processing when the error
     * occurred — -1 when not attributable (client-side error, etc.).
     * AUDIT-2026-04 S3.8 — enables log correlation with server-side
     * tracing without requiring the caller to read a separate
     * {@code client.lastRequestId} field at catch time.
     */
    private final long requestId;

    /**
     * Classifies the type of failure that occurred.
     */
    public enum ErrorCode {
        /** Failed to establish or maintain a connection to the server. */
        CONNECTION_FAILED,
        /** A SQL query failed to execute. */
        QUERY_FAILED,
        /** An append operation failed. */
        APPEND_FAILED,
        /** An argument was invalid (null, empty, out of range). */
        INVALID_ARGUMENT,
        /** The operation was not authorized (authentication or permission failure). */
        UNAUTHORIZED,
        /** The server is temporarily unavailable or overloaded. */
        SERVER_BUSY,
        /** An unknown or unexpected error occurred. */
        UNKNOWN
    }

    /**
     * Creates a new KimberliteException with an error code and message.
     *
     * @param code the error classification
     * @param message a human-readable description of the error
     */
    public KimberliteException(ErrorCode code, String message) {
        super(message);
        this.code = code;
        this.requestId = -1L;
    }

    /**
     * Creates a new KimberliteException with an error code, message, and cause.
     *
     * @param code the error classification
     * @param message a human-readable description of the error
     * @param cause the underlying exception that caused this failure
     */
    public KimberliteException(ErrorCode code, String message, Throwable cause) {
        super(message, cause);
        this.code = code;
        this.requestId = -1L;
    }

    /**
     * Creates a new KimberliteException tagged with the wire request id
     * the server was responding to. Prefer this constructor at the
     * boundary where {@code Response.request_id} becomes available so
     * catch-site callers can correlate logs without reaching back into
     * the client.
     *
     * @param code      the error classification
     * @param message   a human-readable description of the error
     * @param requestId the wire request id, or {@code -1L} if unknown
     */
    public KimberliteException(ErrorCode code, String message, long requestId) {
        super(message);
        this.code = code;
        this.requestId = requestId;
    }

    /**
     * Returns the error classification code.
     *
     * @return the error code
     */
    public ErrorCode getCode() {
        return code;
    }

    /**
     * Returns the wire request id the server was processing when the
     * error occurred, or {@link OptionalLong#empty()} if the error has
     * no attributable request id (client-side error, handshake failure,
     * etc.).
     */
    public OptionalLong getRequestId() {
        return requestId >= 0 ? OptionalLong.of(requestId) : OptionalLong.empty();
    }

    @Override
    public String toString() {
        if (requestId >= 0) {
            return "KimberliteException[" + code + ", requestId=" + requestId + "]: " + getMessage();
        }
        return "KimberliteException[" + code + "]: " + getMessage();
    }
}
