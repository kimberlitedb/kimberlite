package com.kimberlite;

import org.junit.jupiter.api.Test;

import java.util.Arrays;
import java.util.List;

import static org.junit.jupiter.api.Assertions.*;

/**
 * Unit tests for the Kimberlite Java SDK types.
 *
 * <p>These tests exercise the public API types (StreamId, Offset,
 * QueryValue, QueryResult) without requiring the native FFI library.
 * Tests that would require a running server are excluded.
 */
class KimberliteClientTest {

    // ========================================================================
    // StreamId tests
    // ========================================================================

    @Test
    void streamIdValidation() {
        StreamId id = new StreamId(42);
        assertEquals(42, id.getId());
    }

    @Test
    void streamIdRejectsNegative() {
        assertThrows(IllegalArgumentException.class, () -> new StreamId(-1));
    }

    @Test
    void streamIdAcceptsZero() {
        StreamId id = new StreamId(0);
        assertEquals(0, id.getId());
    }

    @Test
    void streamIdAcceptsMaxLong() {
        StreamId id = new StreamId(Long.MAX_VALUE);
        assertEquals(Long.MAX_VALUE, id.getId());
    }

    @Test
    void streamIdEquality() {
        StreamId a = new StreamId(42);
        StreamId b = new StreamId(42);
        StreamId c = new StreamId(99);

        assertEquals(a, b);
        assertNotEquals(a, c);
        assertEquals(a.hashCode(), b.hashCode());
    }

    @Test
    void streamIdToString() {
        StreamId id = new StreamId(42);
        assertEquals("StreamId(42)", id.toString());
    }

    @Test
    void streamIdNotEqualToOtherTypes() {
        StreamId id = new StreamId(42);
        assertNotEquals(id, 42L);
        assertNotEquals(id, "StreamId(42)");
        assertNotEquals(id, null);
    }

    // ========================================================================
    // Offset tests
    // ========================================================================

    @Test
    void offsetValidation() {
        Offset offset = new Offset(100);
        assertEquals(100, offset.getValue());
    }

    @Test
    void offsetRejectsNegative() {
        assertThrows(IllegalArgumentException.class, () -> new Offset(-1));
    }

    @Test
    void offsetAcceptsZero() {
        Offset offset = new Offset(0);
        assertEquals(0, offset.getValue());
    }

    @Test
    void offsetEquality() {
        Offset a = new Offset(100);
        Offset b = new Offset(100);
        Offset c = new Offset(200);

        assertEquals(a, b);
        assertNotEquals(a, c);
        assertEquals(a.hashCode(), b.hashCode());
    }

    @Test
    void offsetToString() {
        Offset offset = new Offset(100);
        assertEquals("Offset(100)", offset.toString());
    }

    @Test
    void offsetNotEqualToOtherTypes() {
        Offset offset = new Offset(100);
        assertNotEquals(offset, 100L);
        assertNotEquals(offset, null);
    }

    // ========================================================================
    // QueryValue tests
    // ========================================================================

    @Test
    void queryValueNull() {
        QueryValue v = QueryValue.ofNull();
        assertTrue(v.isNull());
        assertEquals(QueryValue.Type.NULL, v.getType());
        assertEquals("NULL", v.toString());
    }

    @Test
    void queryValueInteger() {
        QueryValue v = QueryValue.ofInteger(42);
        assertFalse(v.isNull());
        assertEquals(QueryValue.Type.INTEGER, v.getType());
        assertEquals(42, v.asInteger());
    }

    @Test
    void queryValueIntegerNegative() {
        QueryValue v = QueryValue.ofInteger(-1000);
        assertEquals(-1000, v.asInteger());
    }

    @Test
    void queryValueIntegerBoundary() {
        QueryValue vMin = QueryValue.ofInteger(Long.MIN_VALUE);
        assertEquals(Long.MIN_VALUE, vMin.asInteger());

        QueryValue vMax = QueryValue.ofInteger(Long.MAX_VALUE);
        assertEquals(Long.MAX_VALUE, vMax.asInteger());
    }

    @Test
    void queryValueFloat() {
        QueryValue v = QueryValue.ofFloat(3.14);
        assertEquals(QueryValue.Type.FLOAT, v.getType());
        assertEquals(3.14, v.asFloat(), 0.0001);
    }

    @Test
    void queryValueText() {
        QueryValue v = QueryValue.ofText("hello");
        assertEquals(QueryValue.Type.TEXT, v.getType());
        assertEquals("hello", v.asText());
    }

    @Test
    void queryValueTextEmpty() {
        QueryValue v = QueryValue.ofText("");
        assertEquals("", v.asText());
    }

    @Test
    void queryValueTextNullThrows() {
        assertThrows(NullPointerException.class, () -> QueryValue.ofText(null));
    }

    @Test
    void queryValueBoolean() {
        QueryValue vTrue = QueryValue.ofBoolean(true);
        assertTrue(vTrue.asBoolean());
        assertEquals(QueryValue.Type.BOOLEAN, vTrue.getType());

        QueryValue vFalse = QueryValue.ofBoolean(false);
        assertFalse(vFalse.asBoolean());
    }

    @Test
    void queryValueBytes() {
        byte[] data = {(byte) 0xDE, (byte) 0xAD, (byte) 0xBE, (byte) 0xEF};
        QueryValue v = QueryValue.ofBytes(data);
        assertEquals(QueryValue.Type.BYTES, v.getType());

        byte[] result = v.asBytes();
        assertArrayEquals(data, result);

        // Verify defensive copy (modifying original does not affect value)
        data[0] = 0x00;
        assertNotEquals(data[0], v.asBytes()[0]);
    }

    @Test
    void queryValueBytesNullThrows() {
        assertThrows(NullPointerException.class, () -> QueryValue.ofBytes(null));
    }

    @Test
    void queryValueBytesDefensiveCopyOnRead() {
        byte[] data = {0x01, 0x02};
        QueryValue v = QueryValue.ofBytes(data);

        byte[] first = v.asBytes();
        first[0] = 0x00;

        // Second read should be unaffected
        byte[] second = v.asBytes();
        assertEquals(0x01, second[0]);
    }

    @Test
    void queryValueWrongAccessorThrows() {
        QueryValue intValue = QueryValue.ofInteger(42);
        assertThrows(IllegalStateException.class, intValue::asText);
        assertThrows(IllegalStateException.class, intValue::asFloat);
        assertThrows(IllegalStateException.class, intValue::asBoolean);
        assertThrows(IllegalStateException.class, intValue::asBytes);

        QueryValue textValue = QueryValue.ofText("hello");
        assertThrows(IllegalStateException.class, textValue::asInteger);

        QueryValue nullValue = QueryValue.ofNull();
        assertThrows(IllegalStateException.class, nullValue::asInteger);
        assertThrows(IllegalStateException.class, nullValue::asText);
    }

    @Test
    void queryValueToString() {
        assertEquals("NULL", QueryValue.ofNull().toString());
        assertEquals("42", QueryValue.ofInteger(42).toString());
        assertEquals("3.14", QueryValue.ofFloat(3.14).toString());
        assertEquals("hello", QueryValue.ofText("hello").toString());
        assertEquals("true", QueryValue.ofBoolean(true).toString());
        assertTrue(QueryValue.ofBytes(new byte[5]).toString().startsWith("BYTES["));
    }

    // ========================================================================
    // QueryResult tests
    // ========================================================================

    @Test
    void queryResultStructure() {
        List<String> columns = Arrays.asList("id", "name");
        List<List<QueryValue>> rows = List.of(
            List.of(QueryValue.ofInteger(1), QueryValue.ofText("Alice")),
            List.of(QueryValue.ofInteger(2), QueryValue.ofText("Bob"))
        );

        QueryResult result = new QueryResult(columns, rows);

        assertEquals(2, result.getColumnCount());
        assertEquals(2, result.getRowCount());
        assertEquals("id", result.getColumns().get(0));
        assertEquals("name", result.getColumns().get(1));
        assertEquals(1, result.getRows().get(0).get(0).asInteger());
        assertEquals("Alice", result.getRows().get(0).get(1).asText());
        assertEquals(2, result.getRows().get(1).get(0).asInteger());
        assertEquals("Bob", result.getRows().get(1).get(1).asText());
    }

    @Test
    void queryResultEmpty() {
        QueryResult result = new QueryResult(List.of(), List.of());
        assertEquals(0, result.getColumnCount());
        assertEquals(0, result.getRowCount());
    }

    @Test
    void queryResultNullArguments() {
        QueryResult result = new QueryResult(null, null);
        assertEquals(0, result.getColumnCount());
        assertEquals(0, result.getRowCount());
    }

    @Test
    void queryResultColumnsUnmodifiable() {
        List<String> columns = Arrays.asList("a", "b");
        QueryResult result = new QueryResult(columns, List.of());

        assertThrows(UnsupportedOperationException.class,
            () -> result.getColumns().add("c"));
    }

    @Test
    void queryResultRowsUnmodifiable() {
        QueryResult result = new QueryResult(List.of("a"), List.of(
            List.of(QueryValue.ofInteger(1))
        ));

        assertThrows(UnsupportedOperationException.class,
            () -> result.getRows().add(List.of()));
    }

    @Test
    void queryResultToString() {
        QueryResult result = new QueryResult(
            List.of("id"), List.of(List.of(QueryValue.ofInteger(1)))
        );
        String str = result.toString();
        assertTrue(str.contains("columns="));
        assertTrue(str.contains("rowCount=1"));
    }

    // ========================================================================
    // KimberliteException tests
    // ========================================================================

    @Test
    void exceptionWithCodeAndMessage() {
        KimberliteException ex = new KimberliteException(
            KimberliteException.ErrorCode.QUERY_FAILED,
            "syntax error"
        );
        assertEquals(KimberliteException.ErrorCode.QUERY_FAILED, ex.getCode());
        assertEquals("syntax error", ex.getMessage());
        assertNull(ex.getCause());
    }

    @Test
    void exceptionWithCause() {
        RuntimeException cause = new RuntimeException("root cause");
        KimberliteException ex = new KimberliteException(
            KimberliteException.ErrorCode.CONNECTION_FAILED,
            "connection refused",
            cause
        );
        assertEquals(KimberliteException.ErrorCode.CONNECTION_FAILED, ex.getCode());
        assertEquals("connection refused", ex.getMessage());
        assertSame(cause, ex.getCause());
    }

    @Test
    void exceptionToString() {
        KimberliteException ex = new KimberliteException(
            KimberliteException.ErrorCode.UNAUTHORIZED,
            "bad token"
        );
        String str = ex.toString();
        assertTrue(str.contains("UNAUTHORIZED"));
        assertTrue(str.contains("bad token"));
    }

    @Test
    void allErrorCodesExist() {
        // Verify all expected error codes are defined
        KimberliteException.ErrorCode[] codes = KimberliteException.ErrorCode.values();
        assertEquals(7, codes.length);
    }

    // ========================================================================
    // Client static validation (no native lib needed)
    // ========================================================================

    @Test
    void clientVersionDefined() {
        assertNotNull(KimberliteClient.VERSION);
        assertFalse(KimberliteClient.VERSION.isEmpty());
    }
}
