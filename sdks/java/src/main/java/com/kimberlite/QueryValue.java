package com.kimberlite;

import java.util.Arrays;

/**
 * Represents a single value in a query result.
 *
 * <p>QueryValue is a tagged union: each instance has a {@link Type} that
 * indicates which accessor method should be used to retrieve the value.
 * Calling the wrong accessor throws {@link IllegalStateException}.
 *
 * <p>Factory methods:
 * <ul>
 *   <li>{@link #ofNull()} - NULL value</li>
 *   <li>{@link #ofInteger(long)} - 64-bit integer</li>
 *   <li>{@link #ofFloat(double)} - 64-bit floating point</li>
 *   <li>{@link #ofText(String)} - UTF-8 text</li>
 *   <li>{@link #ofBoolean(boolean)} - Boolean</li>
 *   <li>{@link #ofBytes(byte[])} - Binary data</li>
 * </ul>
 */
public final class QueryValue {

    /**
     * The type of value held by a QueryValue.
     */
    public enum Type {
        /** SQL NULL. */
        NULL,
        /** 64-bit signed integer. */
        INTEGER,
        /** 64-bit IEEE 754 floating point. */
        FLOAT,
        /** UTF-8 text string. */
        TEXT,
        /** Boolean (true/false). */
        BOOLEAN,
        /** Binary byte array. */
        BYTES
    }

    private static final QueryValue NULL_VALUE = new QueryValue(Type.NULL, null);

    private final Type type;
    private final Object value;

    private QueryValue(Type type, Object value) {
        this.type = type;
        this.value = value;
    }

    /**
     * Creates a NULL value.
     *
     * @return a QueryValue representing SQL NULL
     */
    public static QueryValue ofNull() {
        return NULL_VALUE;
    }

    /**
     * Creates an integer value.
     *
     * @param value the 64-bit integer
     * @return a QueryValue holding the integer
     */
    public static QueryValue ofInteger(long value) {
        return new QueryValue(Type.INTEGER, value);
    }

    /**
     * Creates a floating-point value.
     *
     * @param value the 64-bit double
     * @return a QueryValue holding the float
     */
    public static QueryValue ofFloat(double value) {
        return new QueryValue(Type.FLOAT, value);
    }

    /**
     * Creates a text value.
     *
     * @param value the text string (must not be null)
     * @return a QueryValue holding the text
     * @throws NullPointerException if value is null
     */
    public static QueryValue ofText(String value) {
        if (value == null) {
            throw new NullPointerException("Text value must not be null; use ofNull() for NULL");
        }
        return new QueryValue(Type.TEXT, value);
    }

    /**
     * Creates a boolean value.
     *
     * @param value the boolean
     * @return a QueryValue holding the boolean
     */
    public static QueryValue ofBoolean(boolean value) {
        return new QueryValue(Type.BOOLEAN, value);
    }

    /**
     * Creates a binary value.
     *
     * @param value the byte array (must not be null)
     * @return a QueryValue holding a copy of the bytes
     * @throws NullPointerException if value is null
     */
    public static QueryValue ofBytes(byte[] value) {
        if (value == null) {
            throw new NullPointerException("Bytes value must not be null; use ofNull() for NULL");
        }
        return new QueryValue(Type.BYTES, value.clone());
    }

    /**
     * Returns the type of this value.
     *
     * @return the value type
     */
    public Type getType() {
        return type;
    }

    /**
     * Returns whether this value is NULL.
     *
     * @return true if the value is NULL
     */
    public boolean isNull() {
        return type == Type.NULL;
    }

    /**
     * Returns the value as a long integer.
     *
     * @return the integer value
     * @throws IllegalStateException if the type is not INTEGER
     */
    public long asInteger() {
        if (type != Type.INTEGER) {
            throw new IllegalStateException(
                "Cannot read " + type + " as INTEGER"
            );
        }
        return (long) value;
    }

    /**
     * Returns the value as a double.
     *
     * @return the float value
     * @throws IllegalStateException if the type is not FLOAT
     */
    public double asFloat() {
        if (type != Type.FLOAT) {
            throw new IllegalStateException(
                "Cannot read " + type + " as FLOAT"
            );
        }
        return (double) value;
    }

    /**
     * Returns the value as a String.
     *
     * @return the text value
     * @throws IllegalStateException if the type is not TEXT
     */
    public String asText() {
        if (type != Type.TEXT) {
            throw new IllegalStateException(
                "Cannot read " + type + " as TEXT"
            );
        }
        return (String) value;
    }

    /**
     * Returns the value as a boolean.
     *
     * @return the boolean value
     * @throws IllegalStateException if the type is not BOOLEAN
     */
    public boolean asBoolean() {
        if (type != Type.BOOLEAN) {
            throw new IllegalStateException(
                "Cannot read " + type + " as BOOLEAN"
            );
        }
        return (boolean) value;
    }

    /**
     * Returns the value as a byte array.
     *
     * <p>Returns a copy of the internal byte array for safety.
     *
     * @return a copy of the byte array
     * @throws IllegalStateException if the type is not BYTES
     */
    public byte[] asBytes() {
        if (type != Type.BYTES) {
            throw new IllegalStateException(
                "Cannot read " + type + " as BYTES"
            );
        }
        return ((byte[]) value).clone();
    }

    @Override
    public String toString() {
        if (type == Type.NULL) {
            return "NULL";
        }
        if (type == Type.BYTES) {
            return "BYTES[" + ((byte[]) value).length + "]";
        }
        return String.valueOf(value);
    }
}
