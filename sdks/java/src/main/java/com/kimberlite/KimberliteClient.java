package com.kimberlite;

import com.kimberlite.internal.NativeBridge;
import com.kimberlite.internal.NativeLoader;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Objects;

/**
 * Main client for interacting with a Kimberlite database.
 *
 * <p>This client wraps the native Kimberlite FFI library via JNI.
 * It is thread-safe: all operations synchronize on the client handle.
 * Implements {@link AutoCloseable} for use with try-with-resources.
 *
 * <p>Example usage:
 * <pre>{@code
 * try (KimberliteClient client = KimberliteClient.connect("127.0.0.1:5432")) {
 *     QueryResult result = client.query("SELECT * FROM patients");
 *     for (List<QueryValue> row : result.getRows()) {
 *         System.out.println(row);
 *     }
 * }
 * }</pre>
 */
public class KimberliteClient implements AutoCloseable {

    /** Current SDK version. */
    public static final String VERSION = "0.4.0";

    private long handle;
    private boolean closed;
    private final Object lock = new Object();

    static {
        NativeLoader.load();
    }

    private KimberliteClient(long handle) {
        if (handle == 0) {
            throw new IllegalArgumentException("Invalid native handle");
        }
        this.handle = handle;
        this.closed = false;
    }

    /**
     * Connects to a Kimberlite database server.
     *
     * @param address the server address in "host:port" format
     * @return a connected client instance
     * @throws KimberliteException if the connection fails
     * @throws NullPointerException if address is null
     */
    public static KimberliteClient connect(String address) throws KimberliteException {
        Objects.requireNonNull(address, "address must not be null");
        if (address.isEmpty()) {
            throw new KimberliteException(
                KimberliteException.ErrorCode.INVALID_ARGUMENT,
                "address must not be empty"
            );
        }

        try {
            long nativeHandle = NativeBridge.connect(address);
            return new KimberliteClient(nativeHandle);
        } catch (Exception e) {
            throw new KimberliteException(
                KimberliteException.ErrorCode.CONNECTION_FAILED,
                "Failed to connect to " + address,
                e
            );
        }
    }

    /**
     * Executes a SQL query and returns the results.
     *
     * @param sql the SQL query string
     * @return the query result containing columns and rows
     * @throws KimberliteException if the query fails or the client is closed
     * @throws NullPointerException if sql is null
     */
    public QueryResult query(String sql) throws KimberliteException {
        Objects.requireNonNull(sql, "sql must not be null");

        synchronized (lock) {
            ensureOpen();

            try {
                Object[] rawResult = NativeBridge.query(handle, sql);
                return parseQueryResult(rawResult);
            } catch (KimberliteException e) {
                throw e;
            } catch (Exception e) {
                throw new KimberliteException(
                    KimberliteException.ErrorCode.QUERY_FAILED,
                    "Query execution failed: " + e.getMessage(),
                    e
                );
            }
        }
    }

    /**
     * Appends data to a stream with the default data classification (PUBLIC).
     *
     * @param streamId the target stream identifier
     * @param payload the event payload bytes
     * @throws KimberliteException if the append fails or the client is closed
     * @throws NullPointerException if streamId or payload is null
     */
    public void append(StreamId streamId, byte[] payload) throws KimberliteException {
        append(streamId, payload, DataClass.PUBLIC);
    }

    /**
     * Appends data to a stream with a specified data classification.
     *
     * @param streamId the target stream identifier
     * @param payload the event payload bytes
     * @param dataClass the data classification level
     * @throws KimberliteException if the append fails or the client is closed
     * @throws NullPointerException if any argument is null
     */
    public void append(StreamId streamId, byte[] payload, DataClass dataClass)
            throws KimberliteException {
        Objects.requireNonNull(streamId, "streamId must not be null");
        Objects.requireNonNull(payload, "payload must not be null");
        Objects.requireNonNull(dataClass, "dataClass must not be null");

        synchronized (lock) {
            ensureOpen();

            try {
                NativeBridge.append(handle, streamId.getId(), payload, dataClass.getValue());
            } catch (KimberliteException e) {
                throw e;
            } catch (Exception e) {
                throw new KimberliteException(
                    KimberliteException.ErrorCode.APPEND_FAILED,
                    "Append failed: " + e.getMessage(),
                    e
                );
            }
        }
    }

    /**
     * Returns whether the client is currently connected.
     *
     * @return true if connected, false otherwise
     */
    public boolean isConnected() {
        synchronized (lock) {
            if (closed) {
                return false;
            }
            return NativeBridge.isConnected(handle);
        }
    }

    /**
     * Closes the client connection and releases all native resources.
     *
     * <p>This method is idempotent: calling it multiple times has no effect
     * beyond the first call.
     *
     * @throws KimberliteException if the disconnect fails
     */
    @Override
    public void close() throws KimberliteException {
        synchronized (lock) {
            if (closed) {
                return;
            }
            closed = true;
            NativeBridge.close(handle);
            handle = 0;
        }
    }

    /**
     * Checks that the client has not been closed.
     *
     * @throws KimberliteException if the client is closed
     */
    private void ensureOpen() throws KimberliteException {
        if (closed) {
            throw new KimberliteException(
                KimberliteException.ErrorCode.CONNECTION_FAILED,
                "Client is closed"
            );
        }
    }

    /**
     * Parses the raw native query result into a typed {@link QueryResult}.
     *
     * <p>The native bridge returns an Object array where:
     * <ul>
     *   <li>Element 0: String[] of column names</li>
     *   <li>Element 1: Object[][] of row data (each cell is a native value)</li>
     * </ul>
     *
     * @param rawResult the raw result from JNI
     * @return a typed QueryResult
     */
    private static QueryResult parseQueryResult(Object[] rawResult) {
        if (rawResult == null || rawResult.length < 2) {
            return new QueryResult(List.of(), List.of());
        }

        String[] columnArray = (String[]) rawResult[0];
        List<String> columns = columnArray != null
            ? Arrays.asList(columnArray)
            : List.of();

        Object[][] rowArray = (Object[][]) rawResult[1];
        List<List<QueryValue>> rows = new ArrayList<>();

        if (rowArray != null) {
            for (Object[] rawRow : rowArray) {
                List<QueryValue> row = new ArrayList<>();
                if (rawRow != null) {
                    for (Object cell : rawRow) {
                        row.add(convertToQueryValue(cell));
                    }
                }
                rows.add(row);
            }
        }

        return new QueryResult(columns, rows);
    }

    /**
     * Converts a raw native value to a typed {@link QueryValue}.
     *
     * @param value the raw value from JNI
     * @return the typed QueryValue
     */
    private static QueryValue convertToQueryValue(Object value) {
        if (value == null) {
            return QueryValue.ofNull();
        }
        if (value instanceof Long longVal) {
            return QueryValue.ofInteger(longVal);
        }
        if (value instanceof Integer intVal) {
            return QueryValue.ofInteger(intVal.longValue());
        }
        if (value instanceof Double doubleVal) {
            return QueryValue.ofFloat(doubleVal);
        }
        if (value instanceof Float floatVal) {
            return QueryValue.ofFloat(floatVal.doubleValue());
        }
        if (value instanceof String stringVal) {
            return QueryValue.ofText(stringVal);
        }
        if (value instanceof Boolean boolVal) {
            return QueryValue.ofBoolean(boolVal);
        }
        if (value instanceof byte[] bytesVal) {
            return QueryValue.ofBytes(bytesVal);
        }
        // Fallback: convert to text
        return QueryValue.ofText(value.toString());
    }
}
