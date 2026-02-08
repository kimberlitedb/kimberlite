package com.kimberlite;

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
 * }
 * }</pre>
 */
public class KimberliteException extends Exception {

    private final ErrorCode code;

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
    }

    /**
     * Returns the error classification code.
     *
     * @return the error code
     */
    public ErrorCode getCode() {
        return code;
    }

    @Override
    public String toString() {
        return "KimberliteException[" + code + "]: " + getMessage();
    }
}
