package com.kimberlite;

/**
 * Data classification level for streams and events.
 *
 * <p>Matches the Rust {@code DataClass} enum in {@code kimberlite-types}.
 * Classification determines encryption, access control, and audit requirements.
 *
 * <ul>
 *   <li>{@link #PUBLIC} - Non-sensitive public data</li>
 *   <li>{@link #INTERNAL} - Organization-internal data</li>
 *   <li>{@link #CONFIDENTIAL} - Sensitive confidential data</li>
 *   <li>{@link #RESTRICTED} - Highly restricted data (e.g., PHI/PII)</li>
 * </ul>
 */
public enum DataClass {

    /** Non-sensitive public data. */
    PUBLIC(0),

    /** Organization-internal data. */
    INTERNAL(1),

    /** Sensitive confidential data. */
    CONFIDENTIAL(2),

    /** Highly restricted data (e.g., PHI/PII). */
    RESTRICTED(3);

    private final int value;

    DataClass(int value) {
        this.value = value;
    }

    /**
     * Returns the integer value used in the FFI layer.
     *
     * @return the numeric representation
     */
    public int getValue() {
        return value;
    }

    /**
     * Converts an integer value to the corresponding DataClass.
     *
     * @param value the numeric representation
     * @return the matching DataClass
     * @throws IllegalArgumentException if the value does not match any DataClass
     */
    public static DataClass fromValue(int value) {
        for (DataClass dc : values()) {
            if (dc.value == value) {
                return dc;
            }
        }
        throw new IllegalArgumentException("Unknown DataClass value: " + value);
    }
}
