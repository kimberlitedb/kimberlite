package com.kimberlite;

/**
 * Type-safe wrapper for a stream identifier.
 *
 * <p>StreamId uniquely identifies a stream within a tenant.
 * Values must be non-negative.
 *
 * <p>This class is immutable and safe for use as a map key.
 */
public final class StreamId {

    private final long id;

    /**
     * Creates a new StreamId.
     *
     * @param id the stream identifier (must be non-negative)
     * @throws IllegalArgumentException if id is negative
     */
    public StreamId(long id) {
        if (id < 0) {
            throw new IllegalArgumentException("StreamId must be non-negative, got: " + id);
        }
        this.id = id;
    }

    /**
     * Returns the underlying stream identifier value.
     *
     * @return the stream ID
     */
    public long getId() {
        return id;
    }

    @Override
    public boolean equals(Object o) {
        if (this == o) {
            return true;
        }
        if (!(o instanceof StreamId other)) {
            return false;
        }
        return id == other.id;
    }

    @Override
    public int hashCode() {
        return Long.hashCode(id);
    }

    @Override
    public String toString() {
        return "StreamId(" + id + ")";
    }
}
