package com.kimberlite.internal;

/**
 * JNI native method declarations for the Kimberlite FFI library.
 *
 * <p>This class maps directly to the C functions exported by
 * {@code libkimberlite_ffi}. All methods are package-private to
 * the internal package; public API users should interact with
 * {@link com.kimberlite.KimberliteClient} instead.
 *
 * <p>Thread safety: The native client handle is NOT thread-safe.
 * Callers (i.e., {@code KimberliteClient}) must synchronize access.
 *
 * <h3>FFI Function Mapping</h3>
 * <pre>
 * connect()     -> kmb_client_connect
 * query()       -> kmb_client_query
 * append()      -> kmb_client_append
 * close()       -> kmb_client_disconnect
 * isConnected() -> (handle validity check)
 * </pre>
 */
public final class NativeBridge {

    private NativeBridge() {
        // Utility class; prevent instantiation
    }

    /**
     * Connects to a Kimberlite server and returns a native handle.
     *
     * <p>Maps to {@code kmb_client_connect} in the FFI layer.
     *
     * @param address the server address in "host:port" format
     * @return a native handle (opaque pointer as long)
     * @throws Exception if the connection fails
     */
    public static native long connect(String address) throws Exception;

    /**
     * Executes a SQL query against the connected server.
     *
     * <p>Maps to {@code kmb_client_query} in the FFI layer.
     * Returns an Object array where element 0 is a String[] of column names
     * and element 1 is an Object[][] of row values.
     *
     * @param handle the native client handle from {@link #connect}
     * @param sql the SQL query string
     * @return the query result as a raw Object array
     * @throws Exception if the query fails
     */
    public static native Object[] query(long handle, String sql) throws Exception;

    /**
     * Appends an event to a stream.
     *
     * <p>Maps to {@code kmb_client_append} in the FFI layer.
     *
     * @param handle the native client handle from {@link #connect}
     * @param streamId the target stream ID
     * @param payload the event payload bytes
     * @param dataClass the data classification level (integer value)
     * @throws Exception if the append fails
     */
    public static native void append(long handle, long streamId, byte[] payload, int dataClass)
            throws Exception;

    /**
     * Disconnects from the server and frees the native client handle.
     *
     * <p>Maps to {@code kmb_client_disconnect} in the FFI layer.
     * After this call, the handle is invalid and must not be used.
     *
     * @param handle the native client handle from {@link #connect}
     */
    public static native void close(long handle);

    /**
     * Checks whether the native client handle represents an active connection.
     *
     * @param handle the native client handle from {@link #connect}
     * @return true if the handle is valid and connected
     */
    public static native boolean isConnected(long handle);
}
