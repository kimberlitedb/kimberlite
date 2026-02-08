package com.kimberlite;

/**
 * Type-safe wrapper for a position in the append-only log.
 *
 * <p>Offsets are monotonically increasing and represent the position
 * of an event in a stream. Values must be non-negative.
 *
 * <p>This class is immutable and safe for use as a map key.
 */
public final class Offset {

    private final long value;

    /**
     * Creates a new Offset.
     *
     * @param value the offset value (must be non-negative)
     * @throws IllegalArgumentException if value is negative
     */
    public Offset(long value) {
        if (value < 0) {
            throw new IllegalArgumentException("Offset must be non-negative, got: " + value);
        }
        this.value = value;
    }

    /**
     * Returns the underlying offset value.
     *
     * @return the offset
     */
    public long getValue() {
        return value;
    }

    @Override
    public boolean equals(Object o) {
        if (this == o) {
            return true;
        }
        if (!(o instanceof Offset other)) {
            return false;
        }
        return value == other.value;
    }

    @Override
    public int hashCode() {
        return Long.hashCode(value);
    }

    @Override
    public String toString() {
        return "Offset(" + value + ")";
    }
}
